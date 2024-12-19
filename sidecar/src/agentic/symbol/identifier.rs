//! Identifier here represents how the code will look like if we have metadata and the
//! location for it
//! We can also use the tools along with this symbol to traverse the code graph

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use derivative::Derivative;
use futures::{lock::Mutex, stream, StreamExt};
use llm_client::{
    clients::types::LLMType,
    provider::{LLMProvider, LLMProviderAPIKeys},
};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    agentic::{
        symbol::events::initial_request::SymbolRequestHistoryItem,
        tool::{
            code_symbol::{new_sub_symbol::NewSymbol, probe::ProbeEnoughOrDeeperResponse},
            lsp::open_file::OpenFileResponse,
        },
    },
    chunking::{
        text_document::Range,
        types::{OutlineNodeContent, OutlineNodeType},
    },
};

use super::{
    errors::SymbolError,
    events::{
        edit::{SymbolToEdit, SymbolToEditRequest},
        initial_request::InitialRequestData,
        message_event::{SymbolEventMessage, SymbolEventMessageProperties},
        probe::{ProbeEnoughOrDeeperResponseParsed, SubSymbolToProbe},
        types::SymbolEvent,
    },
    tool_box::ToolBox,
    tool_properties::ToolProperties,
    types::{SymbolEventRequest, SymbolLocation},
    ui_event::UIEventWithID,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LLMProperties {
    llm: LLMType,
    provider: LLMProvider,
    api_key: LLMProviderAPIKeys,
}

impl LLMProperties {
    pub fn new(llm: LLMType, provider: LLMProvider, api_keys: LLMProviderAPIKeys) -> Self {
        Self {
            llm,
            provider,
            api_key: api_keys,
        }
    }

    pub fn llm(&self) -> &LLMType {
        &self.llm
    }

    pub fn provider(&self) -> &LLMProvider {
        &self.provider
    }

    pub fn api_key(&self) -> &LLMProviderAPIKeys {
        &self.api_key
    }

    pub fn upgrade_llm_to_gemini_pro(mut self) -> Self {
        self.llm = LLMType::GeminiPro;
        self
    }

    /// Only allow tool use when we are using anthropic since open-router does not
    /// support the str_replace_editor tool natively
    pub fn supports_midwit_and_tool_use(&self) -> bool {
        self.llm() == &LLMType::ClaudeSonnet && matches!(&self.provider, &LLMProvider::Anthropic)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, std::hash::Hash, serde::Serialize)]
pub struct Snippet {
    range: Range,
    symbol_name: String,
    fs_file_path: String,
    content: String,
    language: Option<String>,
    // this represents completely a snippet of code which is a logical symbol
    // so a class here will have the complete node (along with all the function inside it),
    // and if its a function then this will be the funciton by itself
    outline_node_content: OutlineNodeContent,
}

impl Snippet {
    pub fn new(
        symbol_name: String,
        range: Range,
        fs_file_path: String,
        content: String,
        outline_node_content: OutlineNodeContent,
    ) -> Self {
        Self {
            symbol_name,
            range,
            fs_file_path,
            content,
            language: None,
            outline_node_content,
        }
    }

    /// Figures out if the snippet belongs to a language which has single snippet
    /// location for the language
    pub fn is_single_block_language(&self) -> bool {
        match self.language.as_deref() {
            Some("python" | "typescript" | "javascript" | "ts" | "js" | "py") => true,
            _ => match self.outline_node_content().language() {
                "python" | "typescript" | "javascript" | "ts" | "js" | "py" => true,
                _ => false,
            },
        }
    }

    pub fn is_potential_match(&self, range: &Range, fs_file_path: &str, is_outline: bool) -> bool {
        if &self.range == range && self.fs_file_path == fs_file_path {
            if is_outline {
                if self.outline_node_content.is_class_type() {
                    true
                } else {
                    // TODO(skcd): This feels wrong, but I am not sure yet
                    false
                }
            } else {
                true
            }
        } else {
            false
        }
    }

    // TODO(skcd): Fix the language over here and make it not None
    pub fn language(&self) -> String {
        self.language.clone().unwrap_or("".to_owned()).to_owned()
    }

    pub fn node_type(&self) -> &OutlineNodeType {
        self.outline_node_content.outline_node_type()
    }

    pub fn file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn range(&self) -> &Range {
        &self.range
    }

    pub fn outline_node_content(&self) -> &OutlineNodeContent {
        &self.outline_node_content
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn to_prompt(&self) -> String {
        let file_path = self.file_path();
        let start_line = self.range().start_line();
        let end_line = self.range().end_line();
        let content = self.content();
        let language = self.language();
        format!(
            r#"
<file_path>
{file_path}:{start_line}-{end_line}
</file_path>
<content>
```{language}
{content}
```
</content>"#
        )
        .to_owned()
    }

    pub fn to_xml(&self) -> String {
        let name = &self.symbol_name;
        let file_path = self.file_path();
        let start_line = self.range().start_line();
        let end_line = self.range().end_line();
        let content = self.content();
        let language = self.language();
        format!(
            r#"<name>
{name}
</name>
<file_path>
{file_path}:{start_line}-{end_line}
</file_path>
<content>
```{language}
{content}
```
</content>"#
        )
        .to_owned()
    }

    pub fn symbol_name(&self) -> &str {
        &self.symbol_name
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, serde::Deserialize, serde::Serialize)]
pub struct SymbolIdentifier {
    symbol_name: String,
    fs_file_path: Option<String>,
}

impl SymbolIdentifier {
    pub fn new_symbol(symbol_name: &str) -> Self {
        Self {
            symbol_name: symbol_name.to_owned(),
            fs_file_path: None,
        }
    }

    pub fn fs_file_path(&self) -> Option<String> {
        self.fs_file_path.clone()
    }

    pub fn symbol_name(&self) -> &str {
        &self.symbol_name
    }

    pub fn with_file_path(symbol_name: &str, fs_file_path: &str) -> Self {
        Self {
            symbol_name: symbol_name.to_owned(),
            fs_file_path: Some(fs_file_path.to_owned()),
        }
    }
}

#[derive(Debug)]
pub struct SnippetReRankInformation {
    idx: usize,
    range: Range,
    fs_file_path: String,
    is_outline: bool,
}

impl SnippetReRankInformation {
    pub fn new(idx: usize, range: Range, fs_file_path: String) -> Self {
        Self {
            idx,
            range,
            fs_file_path,
            is_outline: false,
        }
    }

    pub fn idx(&self) -> usize {
        self.idx
    }

    pub fn range(&self) -> &Range {
        &self.range
    }

    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn is_outline(&self) -> bool {
        self.is_outline
    }

    pub fn set_is_outline(mut self) -> Self {
        self.is_outline = true;
        self
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct MechaCodeSymbolThinking {
    /// The name of the symbol being processed
    symbol_name: String,
    /// A list of steps taken during the thinking process, protected by a mutex for concurrent access
    steps: Mutex<Vec<String>>,
    /// Indicates whether this is a new symbol or an existing one
    is_new: bool,
    /// The file path where the symbol is located
    file_path: String,
    /// The code snippet associated with this symbol, wrapped in a mutex for thread-safe access
    snippet: Mutex<Option<Snippet>>,
    /// Contains all implementations of the symbol, including child elements (e.g., functions inside a class)
    /// These are flattened and stored in a mutex-protected vector for concurrent access
    implementations: Mutex<Vec<Snippet>>,
    /// The tool box containing all necessary tools for symbol processing
    /// Wrapped in an Arc for shared ownership and ignored in Debug output
    #[derivative(Debug = "ignore")]
    tool_box: Arc<ToolBox>,
}

impl MechaCodeSymbolThinking {
    pub fn new(
        symbol_name: String,
        steps: Vec<String>,
        is_new: bool,
        file_path: String,
        snippet: Option<Snippet>,
        implementations: Vec<Snippet>,
        tool_box: Arc<ToolBox>,
    ) -> Self {
        Self {
            symbol_name,
            steps: Mutex::new(steps),
            is_new,
            file_path,
            snippet: Mutex::new(snippet),
            implementations: Mutex::new(implementations),
            tool_box,
        }
    }

    pub fn symbol_name(&self) -> &str {
        &self.symbol_name
    }

    // we need to find the snippet in the code symbol in the file we are interested
    // in and then use that for providing answers
    pub async fn find_snippet_and_create(
        symbol_name: &str,
        steps: Vec<String>,
        file_path: &str,
        tools: Arc<ToolBox>,
        message_properties: SymbolEventMessageProperties,
    ) -> Option<Self> {
        let snippet_maybe = tools
            .find_snippet_for_symbol(file_path, symbol_name, message_properties)
            .await;
        match snippet_maybe {
            Ok(snippet) => Some(MechaCodeSymbolThinking::new(
                symbol_name.to_owned(),
                steps,
                false,
                file_path.to_owned(),
                Some(snippet),
                vec![],
                tools,
            )),
            Err(_) => None,
        }
    }

    // potentital issue here is that the ranges might change after an edit
    // has been made, we have to be careful about that, for now we ball
    pub async fn find_symbol_to_edit(
        &self,
        range: &Range,
        fs_file_path: &str,
        is_outline: bool,
    ) -> Option<Snippet> {
        if let Some(snippet) = self.snippet.lock().await.as_ref() {
            if snippet.is_potential_match(range, fs_file_path, is_outline) {
                return Some(snippet.clone());
            }
        }
        // now we look at the implementations and try to find the potential match
        // over here
        self.implementations
            .lock()
            .await
            .iter()
            .find(|snippet| snippet.is_potential_match(range, fs_file_path, is_outline))
            .map(|snippet| snippet.clone())
    }

    /// This finds the sub-symbol which we want to probe
    /// The sub-symbol can be a function inside the class or a identifier in
    /// the class if needs be or just the class/function itself
    pub async fn find_sub_symbol_in_range(
        &self,
        range: &Range,
        fs_file_path: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        let file_open_result = self
            .tool_box
            .file_open(fs_file_path.to_owned(), message_properties)
            .await?;
        let _ = self
            .tool_box
            .force_add_document(
                fs_file_path,
                file_open_result.contents_ref(),
                file_open_result.language(),
            )
            .await;
        let outline_node = self
            .tool_box
            .get_outline_nodes_grouped(fs_file_path)
            .await
            .ok_or(SymbolError::OutlineNodeNotFound(fs_file_path.to_owned()))?
            // Now we look inside the outline nodes and try to find the ones which contains this range
            // and then we will look into the children of it
            .into_iter()
            .filter(|outline_node| outline_node.range().contains_check_line(range))
            .next()
            .ok_or(SymbolError::NoOutlineNodeSatisfyPosition)?;
        let possible_child_node = outline_node
            .children()
            .into_iter()
            .find(|child_node| child_node.range().contains_check_line(range));
        if let Some(child_node) = possible_child_node {
            Ok(child_node.name().to_owned())
        } else {
            Ok(outline_node.name().to_owned())
        }
    }

    pub async fn find_symbol_in_range(&self, range: &Range, fs_file_path: &str) -> Option<String> {
        if let Some(snippet) = self.snippet.lock().await.as_ref() {
            if snippet.range.contains(range) && snippet.fs_file_path == fs_file_path {
                return Some(snippet.symbol_name.to_owned());
            }
        }
        self.implementations
            .lock()
            .await
            .iter()
            .find(|snippet| {
                if snippet.range.contains(range) && snippet.fs_file_path == fs_file_path {
                    true
                } else {
                    false
                }
            })
            .map(|snippet| snippet.symbol_name.to_owned())
    }

    pub async fn steps(&self) -> Vec<String> {
        let results = self
            .steps
            .lock()
            .await
            .iter()
            .map(|step| step.to_owned())
            .collect::<Vec<_>>();
        results
    }

    pub fn is_new(&self) -> bool {
        self.is_new
    }

    /// Populates the file path for the symbol identifier, not caring about
    /// the fact if the symbol exists on the file path or not
    pub fn to_symbol_identifier_with_file_path(&self) -> SymbolIdentifier {
        SymbolIdentifier::with_file_path(&self.symbol_name, &self.file_path)
    }

    pub fn to_symbol_identifier(&self) -> SymbolIdentifier {
        if self.is_new {
            SymbolIdentifier::new_symbol(&self.symbol_name)
        } else {
            SymbolIdentifier::with_file_path(&self.symbol_name, &self.file_path)
        }
    }

    pub async fn set_snippet(&self, snippet: Snippet) {
        let mut snippet_inside = self.snippet.lock().await;
        *snippet_inside = Some(snippet);
    }

    async fn is_function(&self) -> Option<Snippet> {
        let snippet = self.snippet.lock().await;
        if let Some(ref snippet) = *snippet {
            if snippet.outline_node_content.is_function_type() {
                Some(snippet.clone())
            } else {
                None
            }
        } else {
            None
        }
    }

    pub async fn is_snippet_present(&self) -> bool {
        self.snippet.lock().await.is_some()
    }

    pub async fn get_snippet(&self) -> Option<Snippet> {
        self.snippet.lock().await.clone()
    }

    pub async fn add_step(&self, step: &str) {
        self.steps.lock().await.push(step.to_owned());
    }

    pub fn fs_file_path(&self) -> &str {
        &self.file_path
    }

    pub async fn add_implementation(&self, implementation: Snippet) {
        self.implementations.lock().await.push(implementation);
    }

    /// Grabs all the implementations of this symbol, including the definition
    /// snippet
    pub async fn get_implementations(&self) -> Vec<Snippet> {
        let mut implementations = self
            .implementations
            .lock()
            .await
            .iter()
            .map(|snippet| snippet.clone())
            .collect::<Vec<_>>();
        println!(
            "mecha_code_symbol_thinking::get_implementations::get_snippet({})::get_implementations_len({})::implementations_range({})",
            &self.symbol_name(),
            implementations.len(),
            implementations.iter().map(|implementation| {
                let range = implementation.range();
                let start_line = range.start_line();
                let end_line = range.end_line();
                format!("[{}-{}]", start_line, end_line)
            }).collect::<Vec<_>>().join(",")
        );
        let self_implementation = self.get_snippet().await;
        if let Some(snippet) = self_implementation {
            if !implementations.iter().any(|implementation| {
                implementation
                    .range()
                    .check_equality_without_byte(snippet.range())
                    && &implementation.fs_file_path == &snippet.fs_file_path
            }) {
                implementations.push(snippet);
            }
        }
        implementations
    }

    pub async fn set_implementations(&self, snippets: Vec<Snippet>) {
        let mut implementations = self.implementations.lock().await;
        *implementations = snippets;
    }

    /// Handles if we have enough information to answer the user query or we need
    /// to look deeper in the symbol
    pub async fn probe_deeper_or_answer(
        &self,
        query: &str,
        llm_properties: LLMProperties,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<ProbeEnoughOrDeeperResponseParsed, SymbolError> {
        if self.is_snippet_present().await {
            if let Some((ranked_xml_list, reverse_lookup)) =
                self.to_llm_request(message_properties.clone()).await
            {
                let response = self
                    .tool_box
                    .probe_enough_or_deeper(
                        query.to_owned(),
                        ranked_xml_list,
                        self.symbol_name().to_owned(),
                        llm_properties.clone(),
                        message_properties.clone(),
                    )
                    .await;
                return match response {
                    Ok(ProbeEnoughOrDeeperResponse::AnswerUserQuery(answer)) => {
                        Ok(ProbeEnoughOrDeeperResponseParsed::AnswerUserQuery(answer))
                    }
                    Ok(ProbeEnoughOrDeeperResponse::ProbeDeeper(probe_deeper_list)) => {
                        let probe_deeper_list_ref = probe_deeper_list.get_snippets();
                        let sub_symbols_to_probe = stream::iter(
                            reverse_lookup
                                .into_iter()
                                .map(|data| (data, message_properties.clone())),
                        )
                        .filter_map(|(reverse_lookup, message_properties)| async move {
                            let idx = reverse_lookup.idx();
                            let range = reverse_lookup.range();
                            let fs_file_path = reverse_lookup.fs_file_path();
                            let outline = reverse_lookup.is_outline();
                            let found_reason_to_edit = probe_deeper_list_ref
                                .into_iter()
                                .find(|snippet| snippet.id() == idx)
                                .map(|snippet| snippet.reason_to_probe().to_owned());
                            match found_reason_to_edit {
                                Some(reason) => {
                                    // TODO(skcd): We need to get the sub-symbol over
                                    // here instead of the original symbol name which
                                    // would not work
                                    let symbol_in_range = self
                                        .find_sub_symbol_in_range(
                                            range,
                                            fs_file_path,
                                            message_properties.clone(),
                                        )
                                        .await;
                                    if let Ok(symbol) = symbol_in_range {
                                        Some(SubSymbolToProbe::new(
                                            symbol,
                                            range.clone(),
                                            fs_file_path.to_owned(),
                                            reason,
                                            outline,
                                        ))
                                    } else {
                                        None
                                    }
                                }
                                None => None,
                            }
                        })
                        .collect::<Vec<_>>()
                        .await;
                        Ok(ProbeEnoughOrDeeperResponseParsed::ProbeDeeperInSubSymbols(
                            sub_symbols_to_probe,
                        ))
                    }
                    Err(e) => Err(e),
                };
            }
        }
        return Err(SymbolError::ExpectedFileToExist);
    }

    /// Handles selecting the first sub-symbols in the main symbol which we should
    /// follow or look more deeply into to answer the user query
    pub async fn probe_sub_sybmols(
        &self,
        query: &str,
        llm_properties: LLMProperties,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Vec<SubSymbolToProbe>, SymbolError> {
        // early exit if this is a function
        if let Some(snippet) = self.is_function().await {
            println!(
                "mecha_code_symbol_thinking::probe_sub_symbol::is_function::({})",
                self.symbol_name()
            );
            return Ok(vec![SubSymbolToProbe::new(
                self.symbol_name().to_owned(),
                snippet.range().clone(),
                snippet.fs_file_path.to_owned(),
                query.to_owned(),
                false,
            )]);
        }
        if self.is_snippet_present().await {
            println!(
                "mecha_code_symbol_thinking::probe_sub_symbol::is_snippet_present::({})",
                self.symbol_name()
            );
            if let Some((ranked_xml_list, reverse_lookup)) =
                self.to_llm_request(message_properties.clone()).await
            {
                println!("mecha_code_symbol_thinking::probe_sub_symbol::filter_code_snippets_subsymbol_for_probing::({})", self.symbol_name());
                let filtered_list = self
                    .tool_box
                    .filter_code_snippets_subsymbol_for_probing(
                        ranked_xml_list,
                        query.to_owned(),
                        llm_properties.llm().clone(),
                        llm_properties.provider().clone(),
                        llm_properties.api_key().clone(),
                        message_properties.clone(),
                    )
                    .await?;

                let filtered_list_ref = &filtered_list;

                let sub_symbols_to_edit = stream::iter(
                    reverse_lookup
                        .into_iter()
                        .map(|data| (data, message_properties.clone())),
                )
                .filter_map(|(reverse_lookup, message_properties)| async move {
                    let idx = reverse_lookup.idx();
                    let range = reverse_lookup.range();
                    let fs_file_path = reverse_lookup.fs_file_path();
                    let outline = reverse_lookup.is_outline();
                    let found_reason_to_edit = filtered_list_ref
                        .code_to_probe_list()
                        .into_iter()
                        .find(|snippet| snippet.id() == idx)
                        .map(|snippet| snippet.reason_to_probe().to_owned());
                    match found_reason_to_edit {
                        Some(reason) => {
                            // TODO(skcd): We need to get the sub-symbol over
                            // here instead of the original symbol name which
                            // would not work
                            let symbol_in_range = self
                                .find_sub_symbol_in_range(
                                    range,
                                    fs_file_path,
                                    message_properties.clone(),
                                )
                                .await;
                            if let Ok(symbol) = symbol_in_range {
                                Some(SubSymbolToProbe::new(
                                    symbol,
                                    range.clone(),
                                    fs_file_path.to_owned(),
                                    reason,
                                    outline,
                                ))
                            } else {
                                None
                            }
                        }
                        None => None,
                    }
                })
                .collect::<Vec<_>>()
                .await;
                Ok(sub_symbols_to_edit)
            } else {
                Err(SymbolError::ExpectedFileToExist)
            }
        } else {
            println!(
                "mecha_code_symbol_thinking::probe_sub_symbols::empty::({})",
                self.symbol_name()
            );
            Err(SymbolError::ExpectedFileToExist)
        }
    }

    pub async fn grab_implementations(
        &self,
        tools: Arc<ToolBox>,
        symbol_identifier: SymbolIdentifier,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        let snippet_file_path: Option<String>;
        {
            snippet_file_path = self
                .get_snippet()
                .await
                .map(|snippet| snippet.file_path().to_owned());
        }
        if let Some(snippet_file_path) = snippet_file_path {
            // We first rerank the snippets and then ask the llm for which snippets
            // need to be edited
            // this is not perfect as there is heirarchy in the symbols which we might have
            // to model at some point (but not sure if we really need to do)
            // assuming: LLMs do not need more granular output per class (if there are functions
            // which need to change, we can catch them in the refine step)
            // we break this apart in pieces so the llm can do better
            // we iterate until the llm has listed out all the functions which
            // need to be changed
            // and we are anyways tracking the changes which are happening
            // in the first level of iteration
            // PS: we can ask for a refinement step after this which forces the
            // llm to generate more output for a step using the context it has
            let implementations = tools
                .go_to_implementation(
                    &snippet_file_path,
                    symbol_identifier.symbol_name(),
                    message_properties.clone(),
                )
                .await?;
            let unique_files = implementations
                .get_implementation_locations_vec()
                .iter()
                .map(|implementation| implementation.fs_file_path().to_owned())
                .collect::<HashSet<String>>();
            let cloned_tools = tools.clone();
            // once we have the unique files we have to request to open these locations
            let file_content_map = stream::iter(
                unique_files
                    .clone()
                    .into_iter()
                    .map(|fs_file_path| (fs_file_path, message_properties.clone())),
            )
            .map(|file_path| (file_path, cloned_tools.clone()))
            .map(|((file_path, message_properties), tool_box)| async move {
                let file_path = file_path.to_owned();
                let file_content = tool_box
                    .file_open(file_path.to_owned(), message_properties)
                    .await;
                // we will also force add the file to the symbol broker
                if let Ok(file_content) = &file_content {
                    let _ = tool_box
                        .force_add_document(
                            &file_path,
                            file_content.contents_ref(),
                            &file_content.language(),
                        )
                        .await;
                }
                (file_path, file_content)
            })
            // limit how many files we open in parallel
            .buffer_unordered(4)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<HashMap<String, Result<OpenFileResponse, SymbolError>>>();
            // grab the outline nodes as well
            let outline_nodes = stream::iter(unique_files)
                .map(|file_path| (file_path, cloned_tools.clone(), message_properties.clone()))
                .map(|(file_path, tool_box, message_properties)| async move {
                    (
                        file_path.to_owned(),
                        // TODO(skcd): One of the bugs here is that we are also
                        // returning the node containing the full outline
                        // which wins over any other node, so this breaks the rest of the
                        // flow, what should we do here??
                        tool_box
                            .get_outline_nodes(&file_path, message_properties)
                            .await,
                    )
                })
                .buffer_unordered(1)
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .collect::<HashMap<String, Option<Vec<OutlineNodeContent>>>>();
            // Once we have the file content map, we can read the ranges which we are
            // interested in and generate the implementation areas
            // we have to figure out how to handle updates etc as well, but we will get
            // to that later
            // TODO(skcd): This is probably wrong since we need to calculate the bounding box
            // for the function
            let implementation_content = implementations
                .get_implementation_locations_vec()
                .iter()
                .filter_map(|implementation| {
                    let file_path = implementation.fs_file_path().to_owned();
                    let range = implementation.range();
                    // if file content is empty, then we do not add this to our
                    // implementations
                    let file_content = file_content_map.get(&file_path);
                    if let Some(Ok(ref file_content)) = file_content {
                        let outline_node_for_range = outline_nodes
                            .get(&file_path)
                            .map(|outline_nodes| {
                                if let Some(outline_nodes) = outline_nodes {
                                    // grab the first outline node which we find which contains the range we are interested in
                                    // this will always give us the biggest range
                                    let first_outline_node = outline_nodes
                                        .iter()
                                        .filter(|outline_node| {
                                            outline_node.range().contains_check_line(range)
                                        })
                                        .next();
                                    first_outline_node.map(|outline_node| outline_node.clone())
                                } else {
                                    None
                                }
                            })
                            .flatten();
                        if let Some(outline_node) = outline_node_for_range {
                            if let Some(content) =
                                file_content.content_in_range(&outline_node.range())
                            {
                                Some(Snippet::new(
                                    symbol_identifier.symbol_name().to_owned(),
                                    outline_node.range().clone(),
                                    file_path,
                                    content,
                                    outline_node,
                                ))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();

            // We are de-duplicating the ranges over here since in rust, the derive
            // macros end up pointing to the same outline node over and over again
            let mut outline_ranges_accounted_for: HashSet<Range> = Default::default();
            let filtered_outline_nodes = implementation_content
                .into_iter()
                .filter_map(|snippet| {
                    if outline_ranges_accounted_for.contains(snippet.outline_node_content().range())
                    {
                        None
                    } else {
                        outline_ranges_accounted_for
                            .insert(snippet.outline_node_content().range().clone());
                        Some(snippet)
                    }
                })
                .collect::<Vec<_>>();
            println!(
                "symbol::grab_implementations::({})::len({})",
                self.symbol_name(),
                filtered_outline_nodes.len(),
            );
            // we update the snippets we have stored here into the symbol itself
            {
                self.set_implementations(filtered_outline_nodes).await;
            }
        }
        Ok(())
    }

    pub async fn refresh_state(&self, message_properties: SymbolEventMessageProperties) {
        let snippet = self
            .tool_box
            .find_snippet_for_symbol(
                self.fs_file_path(),
                self.symbol_name(),
                message_properties.clone(),
            )
            .await;

        println!(
            "refresh_state::snippet::details::snippet_is_ok({})",
            snippet.is_ok()
        );

        if let Ok(snippet) = snippet {
            self.set_snippet(snippet.clone()).await;
            let _ = message_properties
                .ui_sender()
                .send(UIEventWithID::symbol_location(
                    message_properties.request_id().request_id().to_owned(),
                    SymbolLocation::new(self.to_symbol_identifier().clone(), snippet.clone()),
                ));

            println!(
                "refresh_state::snippet::outline_node_type({:?})",
                snippet.outline_node_content().outline_node_type()
            );

            // Check if the snippet is of OutlineNodeType::File
            if snippet.outline_node_content().outline_node_type() == &OutlineNodeType::File {
                // Add the snippet to the implementations
                self.add_implementation(snippet).await;
            } else {
                // Grab the implementations again for non-File types
                let _ = self
                    .grab_implementations(
                        self.tool_box.clone(),
                        self.to_symbol_identifier(),
                        message_properties.clone(),
                    )
                    .await;
            }
        }
    }

    /// Grabs the list of new sub-symbols if any that we have to create
    pub async fn decide_new_sub_symbols(
        &self,
        original_request: &InitialRequestData,
        llm_properties: LLMProperties,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Option<Vec<NewSymbol>>, SymbolError> {
        if self.is_function().await.is_some() {
            return Ok(None);
        }
        // otherwise its a class and we might have to create a new symbol over here
        let all_contents = self
            .get_implementations()
            .await
            .into_iter()
            .map(|snippet| {
                let file_path = snippet.file_path();
                let content = snippet.content();
                format!(
                    r#"<file_path>
{file_path}
</file_path>
<content>
{content}
</content>"#
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let response = self
            .tool_box
            .check_new_sub_symbols_required(
                self.symbol_name(),
                all_contents,
                llm_properties,
                original_request.get_original_question(),
                original_request.get_plan(),
                message_properties,
            )
            .await?;

        // Now that we have output, if we do require a new symbol we will get
        // the name and thinking reason behind it over here, we can send that over
        // as an edit
        let new_sub_symbols = response.symbols();
        if new_sub_symbols.is_empty() {
            return Ok(None);
        } else {
            return Ok(Some(new_sub_symbols));
        }
    }

    /// Initiaes a full symbol initial request
    /// - Uses sonnet 3.5 to generate a <thinking> and outline of the changes which
    /// need to be applied
    /// - Grab context using sonnet 3.5 over here (toggle to faster models later on)
    /// - after the outline we generate a full code edit on the symbol
    pub async fn full_symbol_initial_request(
        &self,
        original_request: &InitialRequestData,
        tool_properties: &ToolProperties,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Option<SymbolEventRequest>, SymbolError> {
        println!(
            "mecha_code_symbol_thinking::full_symbol::symbol_name({})",
            self.symbol_name(),
        );
        let mut history = original_request.history().to_vec();

        if self.is_snippet_present().await {
            let outline_node_type;
            {
                let snippet = self.snippet.lock().await;
                outline_node_type = snippet
                    .as_ref()
                    .expect("is_snippet_present to not fail")
                    .outline_node_content
                    .outline_node_type()
                    .clone();
            }
            history.push(SymbolRequestHistoryItem::new(
                self.symbol_name().to_owned(),
                self.fs_file_path().to_owned(),
                original_request.get_plan().to_owned(),
                Some(outline_node_type),
            ));
            // if this is a big search request, we might have been over-eager and want
            // to edit more than required, the best thing to do is self-reflect and check
            // if we even need to edit this
            if original_request.is_big_search_request() {
                println!(
                    "mecha_code_symbol_thinking::full_symbol::initial_request::big_search::should_edit::symbol_name({})", self.symbol_name()
                );
                if let Some(prompt_string) = self.to_llm_request_full_prompt().await {
                    // if we do not have a need to edit, then bail early
                    if !self
                        .tool_box
                        .edits_required_full_symbol(
                            &prompt_string,
                            &original_request.get_plan(),
                            message_properties.clone(),
                        )
                        .await
                        .unwrap_or(true)
                    {
                        return Ok(None);
                    }
                }
            }

            // let _local_code_graph = self.tool_box.local_code_graph(self.fs_file_path(), request_id).await?;
            // now we want to only keep the snippets which we are interested in
            if let Some((_ranked_xml_list, mut reverse_lookup)) =
                self.to_llm_requet_full_listwise().await
            {
                // if we just have a single element over here, then we do not need
                // to do any lookups, especially if the code is in languages other than
                // rust, since for them we always have a single snippet
                if reverse_lookup.len() == 1 {
                    let snippet = self.get_snippet().await.expect("to be present");
                    if snippet.is_single_block_language() {
                        let step_instruction = original_request
                            .symbols_edited_list()
                            .map(|symbol_request_list| {
                                symbol_request_list
                                    .into_iter()
                                    .find(|symbol_request| {
                                        symbol_request.name() == self.symbol_name()
                                            && symbol_request.fs_file_path() == self.fs_file_path()
                                    })
                                    .map(|symbol_request| symbol_request.thinking().to_owned())
                            })
                            .flatten()
                            .into_iter()
                            .collect::<Vec<_>>();
                        return Ok(Some(SymbolEventRequest::new(
                            self.to_symbol_identifier(),
                            SymbolEvent::Edit(SymbolToEditRequest::new(
                                // figure out what to fill over here
                                vec![SymbolToEdit::new(
                                    self.symbol_name().to_owned(),
                                    reverse_lookup.remove(0).range().clone(),
                                    self.fs_file_path().to_owned(),
                                    step_instruction,
                                    false,
                                    false,
                                    true,
                                    original_request.get_original_question().to_owned(),
                                    original_request
                                        .symbols_edited_list()
                                        .map(|symbol_edited_list| symbol_edited_list.to_vec()),
                                    false,
                                    None,
                                    false, // should we disable the followups
                                    None,
                                    vec![],
                                    None,
                                )],
                                self.to_symbol_identifier(),
                                history,
                            )),
                            tool_properties.clone(),
                        )));
                    }
                }
                let original_request_ref = &original_request;
                let sub_symbols_to_edit = stream::iter(reverse_lookup)
                    .filter_map(|reverse_lookup| async move {
                        let range = reverse_lookup.range();
                        let fs_file_path = reverse_lookup.fs_file_path();
                        let outline = reverse_lookup.is_outline();
                        let symbol_in_range = self.find_symbol_in_range(range, fs_file_path).await;
                        if let Some(symbol) = symbol_in_range {
                            Some(SymbolToEdit::new(
                                symbol,
                                range.clone(),
                                fs_file_path.to_owned(),
                                vec![original_request_ref.get_plan()],
                                outline,
                                false,
                                true,
                                original_request_ref.get_original_question().to_owned(),
                                original_request_ref
                                    .symbols_edited_list()
                                    .map(|symbol_edited_list| symbol_edited_list.to_vec()),
                                false,
                                None,
                                false, // should we disable followups and correctness check
                                None,
                                vec![],
                                None,
                            ))
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .await;
                Ok(Some(SymbolEventRequest::new(
                    self.to_symbol_identifier(),
                    SymbolEvent::Edit(SymbolToEditRequest::new(
                        sub_symbols_to_edit,
                        self.to_symbol_identifier(),
                        history,
                    )),
                    tool_properties.clone(),
                )))
            } else {
                println!(
                    "mecha_code_symbol_thinking::missing_to_llm_request_full::({})",
                    self.symbol_name()
                );
                Err(SymbolError::SnippetNotFound)
            }
        } else {
            Err(SymbolError::SymbolError(self.symbol_name().to_owned()))
        }
    }

    /// Initial request follows the following flow:
    /// - COT + follow-along questions for any other symbols which might even lead to edits
    /// - Reranking the snippets for the symbol
    /// - Edit the current symbol
    pub async fn initial_request(
        &self,
        tool_box: Arc<ToolBox>,
        original_request: &InitialRequestData,
        llm_properties: LLMProperties,
        tool_properties: &ToolProperties,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<SymbolEventRequest, SymbolError> {
        println!(
            "mecha_code_symbol_thinking::symbol_name({})",
            self.symbol_name()
        );
        println!(
            "mecha_code_symbol_thinking::thinking_start::symbol_name({})",
            self.symbol_name()
        );

        // This history of the paths we have taken upto this point
        let mut history = original_request.history().to_vec();

        // First we need to verify if we even have to enter the coding loop, often
        // times thinking about this is better and solves generating a lot of code
        // for no reason
        if self.is_snippet_present().await {
            let outline_node_type;
            {
                let snippet = self.snippet.lock().await;
                outline_node_type = snippet
                    .as_ref()
                    .expect("is_snippet_present to work")
                    .outline_node_content()
                    .outline_node_type()
                    .clone();
            }
            // add the current symbol in the history list
            history.push(SymbolRequestHistoryItem::new(
                self.symbol_name().to_owned(),
                self.fs_file_path().to_owned(),
                original_request.get_original_question().to_owned(),
                Some(outline_node_type),
            ));
            // TODO(skcd) we also want to check if we need to generate a new symbol inside the current
            // symbol and if yes, then we need to ask for the name of it and reason behind
            // it, and it can only happen in a class:
            println!("mecha_code_symbol_thinking::checking_new_sub_symbols_required");
            let new_sub_symbols = self
                .decide_new_sub_symbols(
                    original_request,
                    llm_properties.clone(),
                    message_properties.clone(),
                )
                .await?;

            if let Some(new_sub_symbols) = new_sub_symbols {
                println!(
                    "{}::mecha_code_symbol_thinking::checking_new_sub_symbols_required::len({})",
                    self.symbol_name(),
                    new_sub_symbols.len()
                );
                // send over edit request for this
                let snippet_file_path = self
                    .snippet
                    .lock()
                    .await
                    .as_ref()
                    .expect("is_snippet_present to hold")
                    .fs_file_path
                    .to_owned();

                // TODO(skcd): This is clown-town heavy, if we are going to create
                // a new symbol, we need a better way to signal that, this is totally
                // wrong right now
                // clown-town vibes: for typescript this will break since we will
                // move to the end of the symbol which ends with `}` for python
                // we have an empty-line which is okay
                let end_position = self
                    .snippet
                    .lock()
                    .await
                    .as_ref()
                    .expect("is_snippet_present to hold")
                    .range()
                    .end_position()
                    .move_to_next_line();

                // we have to generate the new sub-symbol
                let new_symbol_request = SymbolEventRequest::new(
                    self.to_symbol_identifier(),
                    SymbolEvent::Edit(SymbolToEditRequest::new(
                        new_sub_symbols
                            .into_iter()
                            .map(|new_sub_symbol| {
                                SymbolToEdit::new(
                                    new_sub_symbol.symbol_name().to_owned(),
                                    // The range here looks really fucked lol
                                    Range::new(end_position.clone(), end_position.clone()),
                                    snippet_file_path.to_owned(),
                                    vec![
                                        {
                                            let original_user_quesiton = original_request.get_original_question().to_owned();
                                            format!(r#"original user request: {original_user_quesiton}"#)
                                        },
                                        {
                                            let sub_symbol_name = new_sub_symbol.symbol_name().to_owned();
                                            let reason_to_create = new_sub_symbol.reason_to_create().to_owned();
                                            format!(r#"instructions for {sub_symbol_name}: {reason_to_create}"#)
                                        },
                                    ],
                                    false,
                                    true,
                                    false,
                                    original_request.get_original_question().to_owned(),
                                    original_request.symbols_edited_list().map(|symbol_edited_list| symbol_edited_list.to_vec()),
                                    false,
                                    None,
                                    false, // should we disable followups and correctness check
                                    None,
                                    vec![],
                                    None,
                                )
                            })
                            .collect::<Vec<_>>(),
                        self.to_symbol_identifier(),
                        history.to_vec(),
                    )),
                    tool_properties.clone(),
                );
                let (sender, receiver) = tokio::sync::oneshot::channel();
                let event = SymbolEventMessage::message_with_properties(
                    new_symbol_request,
                    message_properties.clone(),
                    sender,
                );
                let _ = hub_sender.send(event);
                let _ = receiver.await;
            }
            println!(
                "mecha_code_symbol_thinking::llm_request::start({})",
                self.symbol_name()
            );
            // TODO(skcd): We have to refresh our state over here so we get the
            // latest implementation of the symbol
            let _ = self.refresh_state(message_properties.clone()).await;

            if let Some((ranked_xml_list, reverse_lookup)) =
                self.to_llm_request(message_properties.clone()).await
            {
                let is_too_long = if reverse_lookup.len() > 100 {
                    true
                } else {
                    true
                };
                let llm_properties_for_filtering = if is_too_long {
                    // keep using sonnet3.5 over here for now
                    llm_properties
                } else {
                    llm_properties
                };
                println!(
                    "mecha_code_symbol_thinking::reverse_lookup_list::({})::len({})",
                    self.symbol_name(),
                    reverse_lookup.len(),
                );
                // now we send it over to the LLM and register as a rearank operation
                // and then ask the llm to reply back to us
                println!(
                    "mecha_code_symbol_thinking::filter_code_snippets_in_symbol_for_editing::start({})",
                    self.symbol_name(),
                );
                let symbols_to_be_edited = original_request.symbols_edited_list();
                let filtered_list = tool_box
                    .filter_code_snippets_in_symbol_for_editing(
                        ranked_xml_list,
                        original_request.get_original_question().to_owned(),
                        llm_properties_for_filtering.llm().clone(),
                        llm_properties_for_filtering.provider().clone(),
                        llm_properties_for_filtering.api_key().clone(),
                        symbols_to_be_edited,
                        message_properties.clone(),
                    )
                    .await?;

                // We should do a COT over here for each of the individual
                // sub-symbols to check if we really want to edit the code
                // or we want to signal some other symbol for change before
                // making changes to ourselves

                // now we take this filtered list and try to generate back and figure out
                // the ranges which need to be edited
                let code_to_edit_list = filtered_list.code_to_edit_list();
                // we use this to map it back to the symbols which we should
                // be editing and then send those are requests to the hub
                // which will forward it to the right symbol
                let original_request_ref = &original_request;
                let sub_symbols_to_edit = stream::iter(reverse_lookup.into_iter().map(|data| (data, message_properties.clone())))
                    .filter_map(|(reverse_lookup, message_properties)| async move {
                        let idx = reverse_lookup.idx();
                        let range = reverse_lookup.range();
                        let fs_file_path = reverse_lookup.fs_file_path();
                        let outline = reverse_lookup.is_outline();
                        let found_reason_to_edit = code_to_edit_list
                            .snippets()
                            .into_iter()
                            .find(|snippet| snippet.id() == idx)
                            .map(|snippet| {
                                let original_question =
                                    original_request_ref.get_original_question();
                                let reason_to_edit = snippet.reason_to_edit().to_owned();
                                format!(
                                    r#"Original user query:
{original_question}

Reason to edit:
{reason_to_edit}"#
                                )
                            });
                        match found_reason_to_edit {
                            Some(reason) => {
                                // TODO(skcd): We need to get the sub-symbol over
                                // here instead of the original symbol name which
                                // would not work
                                println!("mecha_code_symbol_thinking::initial_request::reason_to_edit::({:?})::({:?})", &range, &fs_file_path);
                                // TODO(skcd): Shoudn't this use the search by name
                                // instead of the using the range for searching
                                let symbol_in_range = self.find_sub_symbol_in_range(
                                        range,
                                        fs_file_path,
                                        message_properties,
                                    )
                                    .await;
                                if let Ok(symbol) = symbol_in_range {
                                    Some(SymbolToEdit::new(
                                        symbol,
                                        range.clone(),
                                        fs_file_path.to_owned(),
                                        vec![reason],
                                        outline,
                                        false,
                                        false,
                                        original_request_ref.get_original_question().to_owned(),
                                        original_request_ref.symbols_edited_list().map(|symbol_edited_list| symbol_edited_list.to_vec()),
                                        false,
                                        None,
                                        false, // should we disable followups and correctness check
                                        None,
                                        vec![],
                                        None,
                                    ))
                                } else {
                                    println!("mecha_code_symbol_thinking::initial_request::no_symbol_found_in_range::({:?})::({:?})", &range, &fs_file_path);
                                    None
                                }
                            }
                            None => None,
                        }
                    })
                    .collect::<Vec<_>>()
                    .await;

                // The idea with the edit requests is that the symbol agent
                // will send this over and then act on it by itself
                // this case is peculiar cause we are editing our own state
                // so we have to think about what that will look like for the agent
                // should we start working on it just at that point, or send it over
                // and keep a tag of the request we are making?
                Ok(SymbolEventRequest::new(
                    self.to_symbol_identifier(),
                    SymbolEvent::Edit(SymbolToEditRequest::new(
                        sub_symbols_to_edit,
                        self.to_symbol_identifier(),
                        history,
                    )),
                    tool_properties.clone(),
                ))
            } else {
                todo!("what do we do over here")
            }
        } else {
            // we have to figure out the location for this symbol and understand
            // where we want to put this symbol at
            // what would be the best way to do this?
            // should we give the folder overview and then ask it
            // or assume that its already written out
            todo!("figure out what to do here");
        }
    }

    // We return an Option here because the symbol might not be present over here
    pub async fn get_symbol_content(&self) -> Option<Vec<String>> {
        let snippet_maybe = {
            self.snippet
                .lock()
                .await
                .as_ref()
                .map(|snippet| snippet.clone())
        };
        if let Some(snippet) = snippet_maybe {
            println!(
                "mecha_code_symbol_thinking::get_symbol_content::symbol_as_ref({})",
                &self.symbol_name()
            );
            let is_function = snippet
                .outline_node_content
                .outline_node_type()
                .is_function();
            let is_definition_assignment = snippet
                .outline_node_content
                .outline_node_type()
                .is_definition_assignment();
            if is_function || is_definition_assignment {
                let content = snippet.outline_node_content.content();
                let file_path = snippet.outline_node_content.fs_file_path();
                Some(vec![format!(
                    r#"<file_path>
{file_path}
</file_path>
<code_symbol>
{content}
</code_symbol>"#
                )])
            } else {
                let implementations = self.get_implementations().await;
                Some(
                    implementations
                        .into_iter()
                        .map(|implementation| {
                            let file_path = implementation.file_path();
                            let content = implementation.content();
                            format!(
                                r#"<file_path>
{file_path}
</file_path>
<code_symbol>
{content}
</code_symbol>"#
                            )
                        })
                        .collect::<Vec<_>>(),
                )
                // This is a class, so over here we have to grab all the implementations
                // as well as the current snippet and then send that over
            }
        } else {
            None
        }
    }

    /// Generates the full symbol which we can put in a prompt for full symbol
    /// analysis and decision making
    ///
    /// We do not generate a list of anything just the full symbol over here
    pub async fn to_llm_request_full_prompt(&self) -> Option<String> {
        let snippet_maybe = {
            self.snippet
                .lock()
                .await
                .as_ref()
                .map(|snippet| snippet.clone())
        };
        if let Some(snippet) = snippet_maybe {
            println!(
                "mecha_code_symbol_thinking::to_llm_request_full_prompt::symbol_as_ref({})",
                &self.symbol_name()
            );
            let is_function = snippet
                .outline_node_content
                .outline_node_type()
                .is_function();
            let is_definition_assignment = snippet
                .outline_node_content
                .outline_node_type()
                .is_definition_assignment();
            if is_function || is_definition_assignment {
                let function_body = snippet.to_prompt();
                Some(function_body)
            } else {
                let implementations = self.get_implementations().await;
                let snippets_ref = implementations.iter().collect::<Vec<_>>();
                // Now we need to format this properly and send it back over to the LLM
                let snippet_xml = snippets_ref
                    .iter()
                    .enumerate()
                    .map(|(_idx, snippet)| {
                        let file_path = snippet.file_path();
                        let start_line = snippet.range().start_line();
                        let end_line = snippet.range().end_line();
                        let location = format!("{}:{}-{}", file_path, start_line, end_line);
                        let language = snippet.language();
                        let content = self
                            .tool_box
                            .get_compressed_symbol_view(snippet.content(), snippet.file_path());
                        format!(
                            r#"
<file_path>
{location}
</file_path>
<content>
```{language}
{content}
```
</content>"#
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(snippet_xml)
            }
        } else {
            None
        }
    }

    /// We generate the sections of the symbols here in full, which implies
    /// that we are not going to create sections of the symbol per function
    /// or in other logical units but as complete
    pub async fn to_llm_requet_full_listwise(
        &self,
    ) -> Option<(String, Vec<SnippetReRankInformation>)> {
        let snippet_maybe = {
            self.snippet
                .lock()
                .await
                .as_ref()
                .map(|snippet| snippet.clone())
        };
        if let Some(snippet) = snippet_maybe {
            println!(
                "mecha_code_symbol_thinking::to_llm_request_full::symbol_as_ref({})",
                &self.symbol_name()
            );
            let is_function = snippet
                .outline_node_content
                .outline_node_type()
                .is_function();
            let is_definition_assignment = snippet
                .outline_node_content
                .outline_node_type()
                .is_definition_assignment();
            if is_function || is_definition_assignment {
                let function_body = snippet.to_xml();
                Some((
                    format!(
                        r#"<rerank_entry>
<id>
0
</id>
{function_body}
</rerank_entry>"#
                    ),
                    vec![SnippetReRankInformation::new(
                        0,
                        snippet.range.clone(),
                        snippet.fs_file_path.to_owned(),
                    )],
                ))
            } else {
                let implementations = self.get_implementations().await;
                let snippets_ref = implementations.iter().collect::<Vec<_>>();
                // Now we need to format this properly and send it back over to the LLM
                let snippet_xml = snippets_ref
                    .iter()
                    .enumerate()
                    .map(|(idx, snippet)| {
                        let file_path = snippet.file_path();
                        let start_line = snippet.range().start_line();
                        let end_line = snippet.range().end_line();
                        let location = format!("{}:{}-{}", file_path, start_line, end_line);
                        let language = snippet.language();
                        let content = snippet.content();
                        // let content = self.tool_box.get_compressed_symbol_view(snippet.content(), snippet.file_path());
                        format!(
                            r#"<rerank_entry>
<id>
{idx}
</id>
<file_path>
{location}
</file_path>
<content>
```{language}
{content}
```
</content>
</rerank_entry>"#
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let snippet_information = snippets_ref
                    .into_iter()
                    .enumerate()
                    .map(|(idx, snippet)| {
                        SnippetReRankInformation::new(
                            idx,
                            snippet.range().clone(),
                            snippet.fs_file_path.to_owned(),
                        )
                    })
                    .collect::<Vec<_>>();
                Some((snippet_xml, snippet_information))
            }
        } else {
            None
        }
    }

    // To xml is a common way to say that the data object implements a way to be
    // written in a xml which is a standard way to represent it for a LLM
    // TODO(skcd): How do we get the symbols which need to be edited here
    // properly, can we ask the llm to put it out properly or we ask it for the section
    // index
    // in which case that might work with the caveat being that if the LLM gets confused
    // we will get a big threshold, another way would be that we ask the llm to also
    // reply in symbols and the indexes as well
    // we have to keep a mapping between the snippets and the indexes we are using
    // that's the hard part
    // we can reconstruct if nothing changes in between which is the initial case
    // anyways but might not be the case always
    // combining both would be better
    // we also need a mapping back here which will help us understand which snippet
    // to look at, the structure I can come up with is something like this:
    // idx -> (Range + FS_FILE_PATH + is_outline)
    // fin
    pub async fn to_llm_request(
        &self,
        message_properties: SymbolEventMessageProperties,
    ) -> Option<(String, Vec<SnippetReRankInformation>)> {
        let snippet_maybe = {
            // take owernship of the snippet over here
            self.snippet
                .lock()
                .await
                .as_ref()
                .map(|snippet| snippet.clone())
        };
        if let Some(snippet) = snippet_maybe {
            println!(
                "mecha_code_symbol_thinking::to_llm_request::symbol_as_ref({})",
                &self.symbol_name()
            );
            let is_function = snippet
                .outline_node_content
                .outline_node_type()
                .is_function();
            let is_definition_assignment = snippet
                .outline_node_content
                .outline_node_type()
                .is_definition_assignment();
            if is_function || is_definition_assignment {
                let function_body = snippet.to_xml();
                Some((
                    format!(
                        r#"<rerank_entry>
<id>
0
</id>
{function_body}
</rerank_entry>"#
                    ),
                    vec![SnippetReRankInformation::new(
                        0,
                        snippet.range.clone(),
                        snippet.fs_file_path.to_owned(),
                    )],
                ))
            } else {
                // and now we have the other symbols which might be a mix of the following
                // functions
                // class implementations
                // one of the problems we hvae have here is that we have to show
                // the llm all these sections and then show the llm on how to edit them
                // this is the most interesting part since we do know what the implementation
                // block looks like with the functions removed, we can use huristics
                // to fix it or expose it as part of the outline nodes
                let implementations = self.get_implementations().await;
                let snippets_ref = implementations.iter().collect::<Vec<_>>();
                println!("mecha_code_symbol_thinking::to_llm_request::class_implementations::symbol({}):implementations_len({})", &self.symbol_name(), snippets_ref.len());
                let mut outline_nodes = vec![];
                for implementation_snippet in snippets_ref.iter() {
                    // TODO(skcd): we are not getting the correct outline node over here :|
                    let outline_node = self
                        .tool_box
                        .get_outline_node_from_snippet(
                            implementation_snippet,
                            message_properties.clone(),
                        )
                        .await;
                    if let Ok(outline_node) = outline_node {
                        outline_nodes.push(outline_node);
                    }
                }

                let outline_nodes_vec = outline_nodes
                    .into_iter()
                    .map(|outline_node| outline_node.consume_all_outlines())
                    .flatten()
                    .collect::<Vec<_>>();
                // Snippets here for class hide the functions, so we want to get
                // the outline node again over here and pass that back to the LLM
                let class_implementations = outline_nodes_vec
                    .iter()
                    .filter(|outline_node| outline_node.is_class_type())
                    .collect::<Vec<_>>();
                let functions = outline_nodes_vec
                    .iter()
                    .filter(|outline_node| outline_node.is_function_type())
                    .collect::<Vec<_>>();
                println!("mecha_code_symbol_thinking::to_llm_request::class_implementations::symbol({}):class_len({})", &self.symbol_name(), class_implementations.len());
                println!(
                    "mecha_code_symbol_thinking::to_llm_request::functions::symbol({})::function_len({})",
                    &self.symbol_name(),
                    functions.len(),
                );
                let mut covered_function_idx: HashSet<usize> = Default::default();
                let class_covering_functions = class_implementations
                    .into_iter()
                    .map(|class_implementation| {
                        let class_range = class_implementation.range();
                        let class_file_path = class_implementation.fs_file_path();
                        let filtered_functions = functions
                            .iter()
                            .enumerate()
                            .filter_map(|(idx, function)| {
                                if class_range.contains(function.range())
                                    && function.fs_file_path() == class_file_path
                                {
                                    covered_function_idx.insert(idx);
                                    Some(function)
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>();
                        let class_non_overlap = class_implementation.get_non_overlapping_content(
                            filtered_functions
                                .iter()
                                .map(|filtered_function| filtered_function.range())
                                .collect::<Vec<_>>()
                                .as_slice(),
                        );
                        (class_implementation, filtered_functions, class_non_overlap)
                    })
                    .collect::<Vec<(
                        &OutlineNodeContent,
                        Vec<&&OutlineNodeContent>,
                        Option<(String, Range)>,
                    )>>();

                // now we will generate the code snippets over here
                // and give them a list
                // this list is a bit special cause it also has prefix in between
                // for some symbols
                let mut symbol_index = 0;
                // we are hedging on the fact that the agent will be able to pick
                // up the snippets properly, instead of going for the inner symbols
                // (kind of orthodox I know, the reason is that the starting part of
                // the symbol is also important and editable, so this approach should
                // in theory work)
                // ideally we will move it back to a range based edit later on
                let mut symbol_rerank_information = vec![];
                let symbol_list = class_covering_functions
                    .into_iter()
                    .map(|(class_snippet, functions, non_overlap_prefix)| {
                        let formatted_snippet = class_snippet.to_xml();
                        if class_snippet.is_class_definition() {
                            let definition = format!(
                                r#"<rerank_entry>
<id>
{symbol_index}
</id>
{formatted_snippet}
</rerank_entry>"#
                            );
                            symbol_rerank_information.push(SnippetReRankInformation::new(
                                symbol_index,
                                class_snippet.range().clone(),
                                class_snippet.fs_file_path().to_owned(),
                            ));
                            symbol_index = symbol_index + 1;
                            definition
                        } else {
                            let overlap = if let Some(non_overlap_prefix) = non_overlap_prefix {
                                let file_path = class_snippet.fs_file_path();
                                let non_overlap_prefix_content = non_overlap_prefix.0;
                                let non_overlap_prefix_range = non_overlap_prefix.1;
                                let start_line = non_overlap_prefix_range.start_line();
                                let end_line = non_overlap_prefix_range.end_line();
                                let language = class_snippet.language();
                                let overlapp_snippet = format!(
                                    r#"<rerank_entry>
<id>
{symbol_index}
</id>
<file_path>
{file_path}:{start_line}-{end_line}
</file_path>
<content>
```{language}
{non_overlap_prefix_content}
```
</content>
</rerank_entry>"#
                                )
                                .to_owned();
                                // guard against impl blocks in rust, since including
                                // just the impl statement can confuse the LLM
                                if !class_snippet.is_class_declaration()
                                    && class_snippet.language().to_lowercase() == "rust"
                                    && class_snippet.has_trait_implementation().is_none()
                                {
                                    None
                                } else {
                                    symbol_rerank_information.push(
                                        SnippetReRankInformation::new(
                                            symbol_index,
                                            non_overlap_prefix_range,
                                            class_snippet.fs_file_path().to_owned(),
                                        )
                                        .set_is_outline(),
                                    );
                                    symbol_index = symbol_index + 1;
                                    Some(overlapp_snippet)
                                }
                            } else {
                                None
                            };
                            let function_snippets = functions
                                .into_iter()
                                .map(|function| {
                                    let function_body = function.to_xml();
                                    let function_code_snippet = format!(
                                        r#"<rerank_entry>
<id>
{symbol_index}
</id>
{function_body}
</rerank_entry>"#
                                    );
                                    symbol_rerank_information.push(SnippetReRankInformation::new(
                                        symbol_index,
                                        function.range().clone(),
                                        function.fs_file_path().to_owned(),
                                    ));
                                    symbol_index = symbol_index + 1;
                                    function_code_snippet
                                })
                                .collect::<Vec<_>>()
                                .join("\n");

                            // now that we have the overlap we have to join it together
                            // with the functions
                            if let Some(overlap) = overlap {
                                format!(
                                    r#"{overlap}
{function_snippets}"#
                                )
                            } else {
                                function_snippets
                            }
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                // for functions which are inside trait boundaries we will do the following:
                // try to get the lines which are not covered by the functions from the outline
                // remove the } from the end of the string (always try and do class.end_line() - max(function.end_line()))
                // and then we put the functions, that way things turn out structured as we want
                // TODO(skcd): This will break in the future since we want to identify the property
                // identifiers, but for now this is completely fine
                // now for the functions which are not covered we will create separate prompts for them
                // cause those are not covered by any class implementation (which is suss...)
                // now we try to see which functions belong to a class
                let uncovered_functions = functions
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, snippet)| {
                        if !covered_function_idx.contains(&idx) {
                            Some(snippet)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();

                // we still have the uncovered functions which we want to sort
                // through
                let uncovered_functions = uncovered_functions
                    .into_iter()
                    .map(|uncovered_function| {
                        let formatted_content = uncovered_function.to_xml();
                        let llm_snippet = format!(
                            "<rerank_entry>
<id>
{symbol_index}
</id>
{formatted_content}
</rerank_entry>"
                        );
                        symbol_rerank_information.push(SnippetReRankInformation::new(
                            symbol_index,
                            uncovered_function.range().clone(),
                            uncovered_function.fs_file_path().to_owned(),
                        ));
                        symbol_index = symbol_index + 1;
                        llm_snippet
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Some((
                    format!(
                        "<rerank_list>
{symbol_list}
{uncovered_functions}
</rerank_list>"
                    ),
                    symbol_rerank_information,
                ))
            }
        } else {
            None
        }
    }
}
