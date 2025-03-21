use std::sync::Arc;

use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage},
};

use async_trait::async_trait;
use serde_xml_rs::from_str;

use crate::agentic::{
    symbol::identifier::LLMProperties,
    tool::{
        errors::ToolError,
        input::ToolInput,
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale},
    },
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReRankingCodeSnippetSymbolOutline {
    name: String,
    fs_file_path: String,
    content: String,
}

impl ReRankingCodeSnippetSymbolOutline {
    pub fn new(name: String, fs_file_path: String, content: String) -> Self {
        Self {
            name,
            fs_file_path,
            content,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn content(&self) -> &str {
        &self.content
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReRankingSnippetsForCodeEditingRequest {
    outline_nodes: Vec<ReRankingCodeSnippetSymbolOutline>,
    // We should make these outline as well, we do not need all the verbose content
    // over here
    code_above: Option<String>,
    code_below: Option<String>,
    code_to_edit_selection: String,
    fs_file_path: String,
    user_query: String,
    llm_properties: LLMProperties,
    root_request_id: String,
}

impl ReRankingSnippetsForCodeEditingRequest {
    pub fn new(
        outline_nodes: Vec<ReRankingCodeSnippetSymbolOutline>,
        code_above: Option<String>,
        code_below: Option<String>,
        code_to_edit_selection: String,
        fs_file_path: String,
        user_query: String,
        llm_properties: LLMProperties,
        root_request_id: String,
    ) -> Self {
        Self {
            outline_nodes,
            code_above,
            code_below,
            code_to_edit_selection,
            fs_file_path,
            user_query,
            llm_properties,
            root_request_id,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename = "code_symbol")]
pub struct ReRankingCodeSymbol {
    name: String,
    file_path: String,
}

impl ReRankingCodeSymbol {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn fs_file_path(&self) -> &str {
        &self.file_path
    }
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReRankingCodeSymbolList {
    #[serde(default)]
    code_symbol: Vec<ReRankingCodeSymbol>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename = "reply")]
pub struct ReRankingSnippetsForCodeEditingResponse {
    thinking: String,
    #[serde(rename = "code_symbol_outline_list")]
    code_symbol_outline_list: ReRankingCodeSymbolList,
}

impl ReRankingSnippetsForCodeEditingResponse {
    fn parse_response(response: &str) -> Result<Self, ToolError> {
        let parsed_response = from_str::<Self>(response);
        match parsed_response {
            Err(_e) => {
                println!("{:?}", _e);
                Err(ToolError::SerdeConversionFailed)
            }
            Ok(parsed_response) => Ok(parsed_response),
        }
    }

    pub fn code_symbol_outline_list(self) -> Vec<ReRankingCodeSymbol> {
        self.code_symbol_outline_list.code_symbol
    }
}

pub struct ReRankingSnippetsForCodeEditingContext {
    llm_client: Arc<LLMBroker>,
    fail_over_llm: LLMProperties,
}

impl ReRankingSnippetsForCodeEditingContext {
    pub fn new(llm_client: Arc<LLMBroker>, fail_over_llm: LLMProperties) -> Self {
        Self {
            llm_client,
            fail_over_llm,
        }
    }

    fn few_shot_examples(&self) -> Vec<LLMClientMessage> {
        vec![
            LLMClientMessage::user(
                r#"<user_query>
We want to implement a new method on symbol event which exposes the initial request question
</user_query>
<code_snippet_to_edit>
```rust
#[derive(Debug, Clone, serde::Serialize)]
pub enum SymbolEvent {{
    InitialRequest(InitialRequestData),
    AskQuestion(AskQuestionRequest),
    UserFeedback,
    Delete,
    Edit(SymbolToEditRequest),
    Outline,
    // Probe
    Probe(SymbolToProbeRequest),
}}
```
</code_snippet_to_edit>
<code_symbol_outline_list>
<code_symbol>
<name>
InitialRequestData
</name>
<content>
FILEPATH: /Users/skcd/scratch/sidecar/sidecar/src/agentic/symbol/events/initial_request.rs
#[derive(Debug, Clone, serde::Serialize)]
pub struct InitialRequestData {{
    original_question: String,
    plan_if_available: Option<String>,
    history: Vec<SymbolRequestHistoryItem>,
    /// We operate on the full symbol instead of the
    full_symbol_request: bool,
}}

impl InitialRequestData {{
    pub fn new(
        original_question: String,
        plan_if_available: Option<String>,
        history: Vec<SymbolRequestHistoryItem>,
        full_symbol_request: bool,
    ) -> Self
    
    pub fn full_symbol_request(&self) -> bool

    pub fn get_original_question(&self) -> &str

    pub fn get_plan(&self) -> Option<String>

    pub fn history(&self) -> &[SymbolRequestHistoryItem]
}}
</content>
</code_symbol>
<code_symbol>
<name>
AskQuestionRequest
</name>
<content>
FILEPATH: /Users/skcd/scratch/sidecar/sidecar/src/agentic/symbol/events/edit.rs
#[derive(Debug, Clone, serde::Serialize)]
pub struct AskQuestionRequest {{
    question: String,
}}

impl AskQuestionRequest {{
    pub fn new(question: String) -> Self

    pub fn get_question(&self) -> &str
}}
</content>
</code_symbol>
<code_symbol>
<name>
SymbolToEditRequest
</name>
<content>
FILEPATH: /Users/skcd/scratch/sidecar/sidecar/src/agentic/symbol/events/edit.rs
#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolToEditRequest {{
    symbols: Vec<SymbolToEdit>,
    symbol_identifier: SymbolIdentifier,
    history: Vec<SymbolRequestHistoryItem>,
}}

impl SymbolToEditRequest {{
    pub fn new(
        symbols: Vec<SymbolToEdit>,
        identifier: SymbolIdentifier,
        history: Vec<SymbolRequestHistoryItem>,
    ) -> Self

    pub fn symbols(self) -> Vec<SymbolToEdit>

    pub fn symbol_identifier(&self) -> &SymbolIdentifier

    pub fn history(&self) -> &[SymbolRequestHistoryItem]
}}
</content>
</code_symbol>
<code_symbol>
<name>
SymbolToProbeRequest
</name>
<content>
FILEPATH: /Users/skcd/scratch/sidecar/sidecar/src/agentic/symbol/events/probe.rs
#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolToProbeRequest {{
    symbol_identifier: SymbolIdentifier,
    probe_request: String,
    original_request: String,
    original_request_id: String,
    history: Vec<SymbolToProbeHistory>,
}}

impl SymbolToProbeRequest {{
    pub fn new(
        symbol_identifier: SymbolIdentifier,
        probe_request: String,
        original_request: String,
        original_request_id: String,
        history: Vec<SymbolToProbeHistory>,
    ) -> Self

    pub fn symbol_identifier(&self) -> &SymbolIdentifier

    pub fn original_request_id(&self) -> &str

    pub fn original_request(&self) -> &str

    pub fn probe_request(&self) -> &str

    pub fn history_slice(&self) -> &[SymbolToProbeHistory]

    pub fn history(&self) -> String
}}
</content>
</code_symbol>
</code_symbol_outline_list>"#
                    .to_owned(),
            ),
            LLMClientMessage::assistant(r#"<reply>
<thinking>
The request talks about implementing new methods for the initial request data, so we need to include the initial request data symbol in the context when trying to edit the code.
</thinking>
<code_symbol_outline_list>
<code_symbol>
<name>
InitialRequestData
</name>
<file_path>
/Users/skcd/scratch/sidecar/sidecar/src/agentic/symbol/events/initial_request.rs
</file_path>
</code_symbol>
</code_symbol_outline_list>
</reply>"#.to_owned()),
        ]
    }

    fn system_message(&self) -> String {
        format!(
            r"You are an expert software eningeer who never writes incorrect code and is tasked with selecting code symbols whose definitions you can use for editing.
The editor has stopped working for you, so we get no help with auto-complete when writing code, hence we want to make sure that we select all the code symbols which are necessary.
As a first step before making changes, you are tasked with collecting all the definitions of the various code symbols whose methods or parameters you will be using when editing the code in the selection.
- You will be given the original user query in <user_query>
- You will be provided the code snippet you will be editing in <code_snippet_to_edit> section.
- The various definitions of the class, method or function (just the high level outline of it) will be given to you as a list in <code_symbol_outline_list>. When writing code you will reuse the methods from here to make the edits, so be very careful when selecting the symbol outlines you are interested in.
- Pay attention to the <code_snippet_to_edit> section and select code symbols accordingly, do not select symbols which we will not be using for making edits.
- Each code_symbol_outline entry is in the following format:
```
<code_symbol>
<name>
{{name of the code symbol over here}}
</name>
<content>
{{the outline content for the code symbol over here}}
</content>
</code_symbol>
```
- You have to decide which code symbols you will be using when doing the edits and select those code symbols.
Your reply should be in the following format:
<reply>
<thinking>
</thinking>
<code_symbol_outline_list>
<code_symbol>
<name>
</name>
<file_path>
</file_path>
</code_symbol>
... more code_symbol sections over here as per your requirement
</code_symbol_outline_list>
<reply>"
        )
    }

    fn user_message(&self, user_context: &ReRankingSnippetsForCodeEditingRequest) -> String {
        let query = &user_context.user_query;
        let file_path = &user_context.fs_file_path;
        let code_interested = &user_context.code_to_edit_selection;
        let code_above = user_context
            .code_above
            .as_ref()
            .map(|code_above| {
                format!(
                    r#"<code_above>
{code_above}
</code_above>"#
                )
            })
            .unwrap_or("".to_owned());
        let code_below = user_context
            .code_below
            .as_ref()
            .map(|code_below| {
                format!(
                    r#"<code_below>
{code_below}
</code_below>"#
                )
            })
            .unwrap_or("".to_owned());
        let outline_nodes = user_context
            .outline_nodes
            .iter()
            .map(|outline_node| {
                let name = &outline_node.name;
                let file_path = &outline_node.fs_file_path;
                let content = &outline_node.content;
                format!(
                    r#"<code_symbol>
<name>
{name}
</name>
<file_path>
{file_path}
</file_path>
<content>
{content}
</content>
</code_symbol>"#
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            r#"<user_query>
{query}
</user_query>

<file_path>
{file_path}
</file_path>

{code_above}
{code_below}
<code_snippet_to_edit>
{code_interested}
</code_snippet_to_edit>

<code_symbol_outline_list>
{outline_nodes}
</code_symbol_outline_list>"#
        )
    }
}

#[async_trait]
impl Tool for ReRankingSnippetsForCodeEditingContext {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.reranking_code_snippets_for_editing_context()?;
        let root_request_id = context.root_request_id.to_owned();
        let llm_properties = context.llm_properties.clone();
        let system_message = LLMClientMessage::system(self.system_message());
        let user_message = LLMClientMessage::user(self.user_message(&context));
        let llm_request = LLMClientCompletionRequest::new(
            llm_properties.llm().clone(),
            vec![system_message]
                .into_iter()
                .chain(self.few_shot_examples())
                .chain(vec![user_message])
                .collect::<Vec<_>>(),
            0.2,
            None,
        );
        let mut retries = 0;
        loop {
            if retries >= 4 {
                return Err(ToolError::RetriesExhausted);
            }
            let (llm, api_key, provider) = if retries % 2 == 0 {
                (
                    llm_properties.llm().clone(),
                    llm_properties.api_key().clone(),
                    llm_properties.provider().clone(),
                )
            } else {
                (
                    self.fail_over_llm.llm().clone(),
                    self.fail_over_llm.api_key().clone(),
                    self.fail_over_llm.provider().clone(),
                )
            };
            let cloned_message = llm_request.clone().set_llm(llm);
            let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
            let response = self
                .llm_client
                .stream_completion(
                    api_key,
                    cloned_message,
                    provider,
                    vec![
                        (
                            "event_type".to_owned(),
                            "reranking_code_snippets_for_editing_context".to_owned(),
                        ),
                        ("root_id".to_owned(), root_request_id.to_owned()),
                    ]
                    .into_iter()
                    .collect(),
                    sender,
                )
                .await;
            match response {
                Ok(response) => {
                    if let Ok(parsed_response) =
                        ReRankingSnippetsForCodeEditingResponse::parse_response(
                            response.answer_up_until_now(),
                        )
                    {
                        return Ok(ToolOutput::re_ranked_code_snippets_for_editing_context(
                            parsed_response,
                        ));
                    } else {
                        retries = retries + 1;
                        continue;
                    }
                }
                Err(_e) => {
                    retries = retries + 1;
                    continue;
                }
            }
        }
    }

    fn tool_description(&self) -> String {
        "".to_owned()
    }

    fn tool_input_format(&self) -> String {
        "".to_owned()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::ReRankingSnippetsForCodeEditingResponse;

    #[test]
    fn test_parsing_with_some_items_in_list_works() {
        let response = r#"
<reply>
<thinking>
The user query requires adding a new endpoint `stop_code_editing` similar to the existing `probe_request_stop` endpoint. To achieve this, we need to understand the implementation of the `probe_request_stop` endpoint and replicate its structure for the new `stop_code_editing` endpoint. Therefore, we need the outline of the `probe_request_stop` function.
</thinking>
<code_symbol_outline_list>
<code_symbol>
<name>
probe_request_stop
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
</file_path>
</code_symbol>
<code_symbol>
<name>
probe_request_stop
</name>
<file_path>
/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs
</file_path>
</code_symbol>
</code_symbol_outline_list>
</reply>
        "#;
        let parsed_response = ReRankingSnippetsForCodeEditingResponse::parse_response(response);
        assert!(parsed_response.is_ok());
    }

    #[test]
    fn test_empty_list_also_works() {
        let response = r#"
<reply>
<thinking>
The user query indicates that we need to add a new endpoint for `stop_code_editing` similar to the existing `probe_request_stop` endpoint. This means we will be modifying the `agentic_router` function to include the new route. Since there are no specific code symbols provided in the outline list, we will focus on the existing structure of the `agentic_router` function and the related endpoints.
</thinking>
<code_symbol_outline_list>
</code_symbol_outline_list>
</reply>
        "#;
        let parsed_response = ReRankingSnippetsForCodeEditingResponse::parse_response(response);
        assert!(parsed_response.is_ok());
    }
}
