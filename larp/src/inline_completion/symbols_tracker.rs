//! We are going to track the symbols here which can be because of any of the following
//! reasons:
//! - file was open in the editor
//! - file is being imported
//! - this file is part of the implementation being done in the current file (think implementations
//! of the same type)
//! - We also want to get the code snippets which have been recently edited
//! Note: this will build towards the next edit prediciton which we want to do eventually
//! Steps being taken:
//! - First we start with just the open tabs and also edit tracking here

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use tokio::sync::Mutex;

use crate::{
    chunking::{
        editor_parsing::EditorParsing,
        text_document::{Position, Range},
        types::OutlineNode,
    },
    inline_completion::helpers::should_track_file,
};

use super::{
    document::content::{
        DocumentEditLines, IdentifierNodeInformation, SnippetInformationWithScore,
    },
    types::TypeIdentifier,
};

const MAX_HISTORY_SIZE: usize = 50;
const MAX_HISTORY_SIZE_FOR_CODE_SNIPPETS: usize = 20;

struct GetDocumentLinesRequest {
    file_path: String,
    context_to_compare: String,
    skip_line: Option<usize>,
}

impl GetDocumentLinesRequest {
    pub fn new(file_path: String, context_to_compare: String, skip_line: Option<usize>) -> Self {
        Self {
            file_path,
            context_to_compare,
            skip_line,
        }
    }
}

struct AddDocumentRequest {
    document_path: String,
    language: String,
    content: String,
    /// Force update can be used to over-write the contents of the what we are
    /// tracking in the document and make it seem like its a new document
    force_update: bool,
}

impl AddDocumentRequest {
    pub fn new(document_path: String, language: String, content: String) -> Self {
        Self {
            document_path,
            language,
            content,
            force_update: false,
        }
    }

    pub fn set_foce_update(mut self) -> Self {
        self.force_update = true;
        self
    }
}

struct FileContentChangeRequest {
    document_path: String,
    file_content: String,
    language: String,
    edits: Vec<(Range, String)>,
}

impl FileContentChangeRequest {
    pub fn new(
        document_path: String,
        file_content: String,
        language: String,
        edits: Vec<(Range, String)>,
    ) -> Self {
        Self {
            document_path,
            file_content,
            language,
            edits,
        }
    }
}

struct GetFileEditedLinesRequest {
    file_path: String,
}

impl GetFileEditedLinesRequest {
    pub fn new(file_path: String) -> Self {
        Self { file_path }
    }
}

struct GetIdentifierNodesRequest {
    file_path: String,
    cursor_position: Position,
}

struct GetDefinitionOutlineRequest {
    file_path: String,
    type_definitions: Vec<TypeIdentifier>,
    editor_parsing: Arc<EditorParsing>,
}

struct GetDocumentOutlineRequest {
    file_path: String,
}

struct SymbolsInRangeRequest {
    fs_file_path: String,
    range: Range,
}

enum SharedStateRequest {
    GetFileContent(String),
    GetDocumentLines(GetDocumentLinesRequest),
    AddDocument(AddDocumentRequest),
    FileContentChange(FileContentChangeRequest),
    GetDocumentHistory,
    GetFileEditedLines(GetFileEditedLinesRequest),
    GetIdentifierNodes(GetIdentifierNodesRequest),
    GetDefinitionOutline(GetDefinitionOutlineRequest),
    GetDocumentOutline(GetDocumentOutlineRequest),
    GetSymbolHistory,
    GetSymbolsInRange(SymbolsInRangeRequest),
}

enum SharedStateResponse {
    DocumentHistoryResponse(Vec<String>),
    Ok,
    FileContentResponse(Option<String>),
    GetDocumentLinesResponse(Option<Vec<SnippetInformationWithScore>>),
    FileEditedLinesResponse(Vec<usize>),
    GetIdentifierNodesResponse(IdentifierNodeInformation),
    GetDefinitionOutlineResponse(Vec<String>),
    GetSymbolHistoryResponse(Vec<SymbolInformation>),
    GetDocumentOutlineResponse(Option<Vec<OutlineNode>>),
    SymbolsInRangeResponse(Vec<OutlineNode>),
}

/// We are keeping track of the symbol node where the user is editing, this can
/// imply that the user is deleting recent code or even just navigating, we are just
/// marking it and keep track of those things
/// The higher level idea is that it will be helpful to hae this list in some form
/// or fashion.
#[derive(Debug, Clone)]
pub struct SymbolInformation {
    symbol_node: OutlineNode,
    // we want to keep an increasing order of timestamp and evict things frmo the queue
    // as they become unnecessary, not sure whats the right thing to do here
    timestamp: i64,
    // the lines which have been edited in this symbols
    edited_lines: Vec<usize>,
}

impl SymbolInformation {
    pub fn new(symbol_node: OutlineNode, timestamp: i64, edited_lines: Vec<usize>) -> Self {
        Self {
            symbol_node,
            timestamp,
            edited_lines,
        }
    }

    pub fn get_edited_lines(&self) -> Vec<usize> {
        self.edited_lines.to_vec()
    }

    pub fn set_edited_lines(&mut self, edited_lines: Vec<usize>) {
        self.edited_lines = edited_lines;
    }

    pub fn symbol_node(&self) -> &OutlineNode {
        &self.symbol_node
    }

    pub fn timestamp(&self) -> i64 {
        self.timestamp
    }
}

pub struct SharedState {
    document_lines: Mutex<HashMap<String, DocumentEditLines>>,
    document_history: Mutex<Vec<String>>,
    editor_parsing: Arc<EditorParsing>,
    // really here this should not be a vector but it needs to be a graph where
    // the user is jumping around, somehow we wll figure out what to do about that?
    // let's keep it linear for now
    symbol_history: Arc<Mutex<Vec<SymbolInformation>>>,
}

impl SharedState {
    async fn process_request(&self, request: SharedStateRequest) -> SharedStateResponse {
        match request {
            SharedStateRequest::AddDocument(add_document_request) => {
                let _ = self
                    .add_document(
                        add_document_request.document_path,
                        add_document_request.content,
                        add_document_request.language,
                        add_document_request.force_update,
                    )
                    .await;
                SharedStateResponse::Ok
            }
            SharedStateRequest::FileContentChange(file_content_change_request) => {
                let _ = self
                    .file_content_change(
                        file_content_change_request.document_path,
                        file_content_change_request.file_content,
                        file_content_change_request.language,
                        file_content_change_request.edits,
                    )
                    .await;
                SharedStateResponse::Ok
            }
            SharedStateRequest::GetDocumentLines(get_document_lines_request) => {
                let response = self
                    .get_document_lines(
                        &get_document_lines_request.file_path,
                        &get_document_lines_request.context_to_compare,
                        get_document_lines_request.skip_line,
                    )
                    .await;
                SharedStateResponse::GetDocumentLinesResponse(response)
            }
            SharedStateRequest::GetFileContent(get_file_content_request) => {
                let response = self.get_file_content(&get_file_content_request).await;
                SharedStateResponse::FileContentResponse(response)
            }
            SharedStateRequest::GetDocumentHistory => {
                let response = self.get_document_history().await;
                SharedStateResponse::DocumentHistoryResponse(response)
            }
            SharedStateRequest::GetFileEditedLines(file_request) => {
                let response = self.get_edited_lines(&file_request.file_path).await;
                SharedStateResponse::FileEditedLinesResponse(response)
            }
            SharedStateRequest::GetIdentifierNodes(request) => {
                let file_path = request.file_path;
                let position = request.cursor_position;
                let response = self.get_identifier_nodes(&file_path, position).await;
                SharedStateResponse::GetIdentifierNodesResponse(response)
            }
            SharedStateRequest::GetDefinitionOutline(request) => {
                let response = self.get_definition_outline(request).await;
                SharedStateResponse::GetDefinitionOutlineResponse(response)
            }
            SharedStateRequest::GetSymbolHistory => {
                let symbols = self.symbol_history.lock().await;
                SharedStateResponse::GetSymbolHistoryResponse(symbols.to_vec())
            }
            SharedStateRequest::GetDocumentOutline(document_outline) => {
                let file_path = document_outline.file_path;
                let response = self.get_outline_nodes(&file_path).await;
                SharedStateResponse::GetDocumentOutlineResponse(response)
            }
            SharedStateRequest::GetSymbolsInRange(symbols_in_range_request) => {
                let response = self.get_symbols_in_range(symbols_in_range_request).await;
                SharedStateResponse::SymbolsInRangeResponse(response)
            }
        }
    }

    async fn get_symbols_in_range(&self, request: SymbolsInRangeRequest) -> Vec<OutlineNode> {
        let outline_nodes: Vec<OutlineNode>;
        {
            let document_lines = self.document_lines.lock().await;
            if let Some(document_line) = document_lines.get(&request.fs_file_path) {
                outline_nodes = document_line.get_symbols_in_ranges(vec![request.range].as_slice());
            } else {
                // TODO(skcd): This is wrong, because we should be still returning
                // None instead of blank outline nodes
                outline_nodes = vec![];
            }
        }
        outline_nodes
    }

    async fn get_definition_outline(&self, request: GetDefinitionOutlineRequest) -> Vec<String> {
        let file_path = request.file_path;
        let language_config = request.editor_parsing.for_file_path(&file_path);
        if let None = language_config {
            return vec![];
        }
        let language_config = language_config.expect("if let None to hold");
        let comment_prefix = language_config.comment_prefix.to_owned();
        // TODO(skcd): Filter out the files which belong to the native
        // dependencies of the language (the LLM already knows about them)
        let definition_file_paths = request
            .type_definitions
            .iter()
            .map(|type_definition| {
                let definitions = type_definition.type_definitions();
                definitions
                    .iter()
                    .map(|definition| definition.file_path().to_owned())
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
            })
            .flatten()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<String>>();
        // putting in a block so we drop the lock quickly
        let file_to_outline: HashMap<String, Vec<OutlineNode>>;
        {
            let document_lines = self.document_lines.lock().await;
            // Now here we are going to check for each of the file
            file_to_outline = definition_file_paths
                .into_iter()
                .filter_map(|definition_file_path| {
                    let document_lines = document_lines.get(&definition_file_path);
                    if let Some(document_lines) = document_lines {
                        let outline_nodes = document_lines.outline_nodes();
                        Some((definition_file_path, outline_nodes))
                    } else {
                        None
                    }
                })
                .collect::<HashMap<_, _>>();
        }

        // Now we can grab the outline as required, we need to check this by
        // the range provided and then grabbing the context from the outline
        let definitions_string = request
            .type_definitions
            .into_iter()
            .filter_map(|type_definition| {
                // Here we have to not include files which are part of the common
                // lib which the LLM will know about
                let definitions_interested = type_definition
                    .type_definitions()
                    .iter()
                    .filter(|definition| language_config.is_file_relevant(definition.file_path()))
                    .filter(|definition| file_to_outline.contains_key(definition.file_path()))
                    .collect::<Vec<_>>();

                let identifier = type_definition.node().identifier();
                let definitions = definitions_interested
                    .iter()
                    .filter_map(|definition_interested| {
                        if let Some(outline_nodes) =
                            file_to_outline.get(definition_interested.file_path())
                        {
                            definition_interested
                                .get_outline(outline_nodes.as_slice(), language_config)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                if definitions.is_empty() {
                    None
                } else {
                    let definitions_str = definitions.join("\n");
                    Some(format!(
                        r#"{comment_prefix} Type for {identifier}
{definitions_str}"#
                    ))
                }
            })
            .collect::<Vec<_>>();

        definitions_string
    }

    async fn get_file_content(&self, file_path: &str) -> Option<String> {
        let document_lines = self.document_lines.lock().await;
        document_lines
            .get(file_path)
            .map(|document_lines| document_lines.get_content())
    }

    async fn track_file(&self, document_path: String) {
        // First we check if the document is already present in the history
        {
            let mut document_history = self.document_history.lock().await;
            if !document_history.contains(&document_path) {
                document_history.push(document_path.to_owned());
                if document_history.len() > MAX_HISTORY_SIZE {
                    document_history.remove(0);
                }
            } else {
                let index = document_history
                    .iter()
                    .position(|x| x == &document_path)
                    .unwrap();
                document_history.remove(index);
                document_history.push(document_path.to_owned());
            }
        }
    }

    async fn get_edited_lines(&self, file_path: &str) -> Vec<usize> {
        {
            let mut document_lines = self.document_lines.lock().await;
            if let Some(ref mut document_lines_entry) = document_lines.get_mut(file_path) {
                return document_lines_entry.get_edited_lines();
            }
        }
        Vec::new()
    }

    async fn get_outline_nodes(&self, file_path: &str) -> Option<Vec<OutlineNode>> {
        let document_lines = self.document_lines.lock().await;
        if let Some(ref document_lines_entry) = document_lines.get(file_path) {
            Some(document_lines_entry.outline_nodes())
        } else {
            None
        }
    }

    async fn get_identifier_nodes(
        &self,
        file_path: &str,
        position: Position,
    ) -> IdentifierNodeInformation {
        {
            let mut document_lines = self.document_lines.lock().await;
            if let Some(ref mut document_lines_entry) = document_lines.get_mut(file_path) {
                return document_lines_entry.get_identifier_nodes(position);
            }
        }
        Default::default()
    }

    async fn get_document_lines(
        &self,
        file_path: &str,
        context_to_compare: &str,
        skip_line: Option<usize>,
    ) -> Option<Vec<SnippetInformationWithScore>> {
        {
            let mut document_lines = self.document_lines.lock().await;
            if let Some(ref mut document_lines_entry) = document_lines.get_mut(file_path) {
                return Some(
                    document_lines_entry.grab_similar_context(context_to_compare, skip_line),
                );
            }
        }
        None
    }

    async fn add_document(
        &self,
        document_path: String,
        content: String,
        language: String,
        force_update: bool,
    ) {
        if !should_track_file(&document_path) {
            return;
        }
        // First we check if the document is already present in the history
        self.track_file(document_path.to_owned()).await;
        if force_update {
            {
                let mut document_lines = self.document_lines.lock().await;
                let document_lines_entry = DocumentEditLines::new(
                    document_path.to_owned(),
                    content,
                    language,
                    self.editor_parsing.clone(),
                );
                document_lines.insert(document_path.clone(), document_lines_entry);
                assert!(document_lines.contains_key(&document_path));
            }
        } else {
            // Next we will create an entry in the document lines if it does not exist
            {
                let mut document_lines = self.document_lines.lock().await;
                if !document_lines.contains_key(&document_path) {
                    let document_lines_entry = DocumentEditLines::new(
                        document_path.to_owned(),
                        content,
                        language,
                        self.editor_parsing.clone(),
                    );
                    document_lines.insert(document_path.clone(), document_lines_entry);
                }
                assert!(document_lines.contains_key(&document_path));
            }
        }
    }

    async fn update_symbol_history(&self, changed_symbols: Vec<SymbolInformation>) {
        // we will update the changed symbols queue
        // we also want to get the range of the lines which have been updated in the symbol, probably not their content
        // right now cause thats not useful enough right??
        {
            let mut symbol_history = self.symbol_history.lock().await;
            // TODO(skcd): While appending here we want to make sure that we are able
            // to keep track of the list of symbols properly and not keep adding
            // repeated symbols to the list
            for symbol_information in changed_symbols.into_iter() {
                match symbol_history.last_mut() {
                    Some(last_symbol) => {
                        //TODO(skcd): we need to check here if these 2 are possibly the same symbol nodes
                        // as the user might be typing them out in which case its pretty bad
                        // just a prefix check here might work?
                        if last_symbol.symbol_node().name()
                            == symbol_information.symbol_node().name()
                        {
                            last_symbol.set_edited_lines(symbol_information.get_edited_lines());
                            // we need to update the edited lines here on the last or replace it
                            // with what we have
                            continue;
                        } else {
                            // if the last symbol which we saw was different
                            // from the one which is being edited then we should insert it
                            symbol_history.push(symbol_information);
                        }
                    }
                    None => {
                        symbol_history.push(symbol_information);
                    }
                }
            }
        }
    }

    // TOOD(skcd): We are going to return the symbols which are changed after
    // we apply the edits, this will help us maintain the history map we are interested
    // in.
    async fn file_content_change(
        &self,
        document_path: String,
        file_content: String,
        language: String,
        edits: Vec<(Range, String)>,
    ) {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        // always track the file which is being edited
        self.track_file(document_path.to_owned()).await;
        if edits.is_empty() {
            return;
        }

        let mut changed_symbol_nodes = vec![];
        // we call the lock on document lines in a scope
        {
            // Now we first need to get the lock over the document lines
            // and then iterate over all the edits and apply them
            let mut document_lines = self.document_lines.lock().await;

            // If we do not have the document (which can happen if the sidecar restarts, just add it
            // and do not do anything about the edits yet)
            if !document_lines.contains_key(&document_path) {
                let document_lines_entry = DocumentEditLines::new(
                    document_path.to_owned(),
                    file_content,
                    language,
                    self.editor_parsing.clone(),
                );
                document_lines.insert(document_path.clone(), document_lines_entry);
            } else {
                let document_lines_entry = document_lines.get_mut(&document_path);
                // This match should not be required but for some reason we are hitting
                // the none case even in this branch after our checks
                match document_lines_entry {
                    Some(document_lines_entry) => {
                        for (range, new_text) in edits {
                            // we give all of these the same timestamp
                            let nodes_edit_current_edit =
                                document_lines_entry.content_change(range, new_text, current_time);
                            changed_symbol_nodes.append(
                                &mut nodes_edit_current_edit
                                    .into_iter()
                                    .map(|edited_nodes| {
                                        let node_range = edited_nodes.range();
                                        // dbg!("file_content_change.node", &node_range);
                                        let edited_lines = document_lines_entry
                                            .get_edited_lines_in_range(node_range);
                                        // dbg!("file_content_change.edited_lines", &edited_lines);
                                        SymbolInformation::new(
                                            edited_nodes,
                                            current_time,
                                            edited_lines,
                                        )
                                    })
                                    .collect::<Vec<_>>(),
                            );
                            // grab the range of the edits which are happening using the document lines
                        }
                    }
                    None => {
                        let document_lines_entry = DocumentEditLines::new(
                            document_path.to_owned(),
                            file_content,
                            language,
                            self.editor_parsing.clone(),
                        );
                        document_lines.insert(document_path.clone(), document_lines_entry);
                    }
                }
            }
        }
        self.update_symbol_history(changed_symbol_nodes).await;
    }

    async fn get_document_history(&self) -> Vec<String> {
        // only get the MAX_HISTORY_SIZE_FOR_CODE_SNIPPETS size from the history
        let document_history = self.document_history.lock().await;
        document_history
            .iter()
            // we need it in the reverse order
            .rev()
            .take(MAX_HISTORY_SIZE_FOR_CODE_SNIPPETS)
            .map(|x| x.clone())
            .collect()
    }
}

/// This is the symbol tracker which will be used for inline completion
/// We keep track of the document histories and the content of these documents
pub struct SymbolTrackerInline {
    // We are storing the fs path of the documents, these are stored in the reverse
    // order
    sender: tokio::sync::mpsc::UnboundedSender<(
        SharedStateRequest,
        tokio::sync::oneshot::Sender<SharedStateResponse>,
    )>,
}

impl SymbolTrackerInline {
    pub fn new(editor_parsing: Arc<EditorParsing>) -> SymbolTrackerInline {
        let shared_state = Arc::new(SharedState {
            document_lines: Mutex::new(HashMap::new()),
            document_history: Mutex::new(Vec::new()),
            editor_parsing,
            symbol_history: Arc::new(Mutex::new(Vec::new())),
        });
        let shared_state_cloned = shared_state.clone();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<(
            SharedStateRequest,
            tokio::sync::oneshot::Sender<SharedStateResponse>,
        )>();

        // start a background thread with the receiver
        tokio::spawn(async move {
            let shared_state = shared_state_cloned.clone();
            while let Some(value) = receiver.recv().await {
                let request = value.0;
                let sender = value.1;
                let response = shared_state.process_request(request).await;
                let _ = sender.send(response);
            }
        });

        // we also want to reindex and re-order the snippets continuously over here
        // the question is what kind of files are necessary here to make it work
        SymbolTrackerInline { sender }
    }

    pub async fn get_file_content(&self, file_path: &str) -> Option<String> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let request = SharedStateRequest::GetFileContent(file_path.to_owned());
        let _ = self.sender.send((request, sender));
        let reply = receiver.await;
        if let Ok(SharedStateResponse::FileContentResponse(response)) = reply {
            response
        } else {
            None
        }
    }

    pub async fn get_file_edited_lines(&self, file_path: &str) -> Vec<usize> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let request = SharedStateRequest::GetFileEditedLines(GetFileEditedLinesRequest::new(
            file_path.to_owned(),
        ));
        let _ = self.sender.send((request, sender));
        let reply = receiver.await;
        if let Ok(SharedStateResponse::FileEditedLinesResponse(response)) = reply {
            response
        } else {
            Vec::new()
        }
    }

    pub async fn get_document_lines(
        &self,
        file_path: &str,
        context_to_compare: &str,
        skip_line: Option<usize>,
    ) -> Option<Vec<SnippetInformationWithScore>> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let request = SharedStateRequest::GetDocumentLines(GetDocumentLinesRequest::new(
            file_path.to_owned(),
            context_to_compare.to_owned(),
            skip_line,
        ));
        let _ = self.sender.send((request, sender));
        let reply = receiver.await;
        if let Ok(SharedStateResponse::GetDocumentLinesResponse(response)) = reply {
            response
        } else {
            None
        }
    }

    /// This adds the document as a new entry even if it already existed
    pub async fn force_add_document(
        &self,
        document_path: String,
        content: String,
        langauge: String,
    ) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let request = SharedStateRequest::AddDocument(
            AddDocumentRequest::new(document_path, langauge, content).set_foce_update(),
        );
        let _ = self.sender.send((request, sender));
        let _ = receiver.await;
    }

    pub async fn add_document(&self, document_path: String, content: String, language: String) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let request = SharedStateRequest::AddDocument(AddDocumentRequest::new(
            document_path,
            language,
            content,
        ));
        let _ = self.sender.send((request, sender));
        let _ = receiver.await;
    }

    pub async fn file_content_change(
        &self,
        document_path: String,
        file_content: String,
        language: String,
        edits: Vec<(Range, String)>,
    ) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let request = SharedStateRequest::FileContentChange(FileContentChangeRequest::new(
            document_path,
            file_content,
            language,
            edits,
        ));
        let _ = self.sender.send((request, sender));
        let _ = receiver.await;
    }

    pub async fn get_document_history(&self) -> Vec<String> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let request = SharedStateRequest::GetDocumentHistory;
        let _ = self.sender.send((request, sender));
        let reply = receiver.await;
        if let Ok(SharedStateResponse::DocumentHistoryResponse(response)) = reply {
            response
        } else {
            vec![]
        }
    }

    pub async fn get_identifier_nodes(
        &self,
        file_path: &str,
        position: Position,
    ) -> IdentifierNodeInformation {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let request = SharedStateRequest::GetIdentifierNodes(GetIdentifierNodesRequest {
            file_path: file_path.to_owned(),
            cursor_position: position,
        });
        let _ = self.sender.send((request, sender));
        let reply = receiver.await;
        if let Ok(SharedStateResponse::GetIdentifierNodesResponse(response)) = reply {
            response
        } else {
            Default::default()
        }
    }

    pub async fn get_definition_configs(
        &self,
        file_path: &str,
        type_definitions: Vec<TypeIdentifier>,
        editor_parsing: Arc<EditorParsing>,
    ) -> Vec<String> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let request = SharedStateRequest::GetDefinitionOutline(GetDefinitionOutlineRequest {
            file_path: file_path.to_owned(),
            type_definitions,
            editor_parsing,
        });
        let _ = self.sender.send((request, sender));
        let response = receiver.await;
        if let Ok(SharedStateResponse::GetDefinitionOutlineResponse(response)) = response {
            response
        } else {
            Default::default()
        }
    }

    pub async fn get_symbol_history(&self) -> Vec<SymbolInformation> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let request = SharedStateRequest::GetSymbolHistory;
        let _ = self.sender.send((request, sender));
        let response = receiver.await;
        if let Ok(SharedStateResponse::GetSymbolHistoryResponse(response)) = response {
            response
        } else {
            Default::default()
        }
    }

    pub async fn get_symbols_outline(&self, file_path: &str) -> Option<Vec<OutlineNode>> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let request = SharedStateRequest::GetDocumentOutline(GetDocumentOutlineRequest {
            file_path: file_path.to_owned(),
        });
        let _ = self.sender.send((request, sender));
        let response = receiver.await;
        if let Ok(SharedStateResponse::GetDocumentOutlineResponse(response)) = response {
            response
        } else {
            None
        }
    }

    pub async fn get_symbols_in_range(
        &self,
        fs_file_path: &str,
        range: &Range,
    ) -> Option<Vec<OutlineNode>> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let request = SharedStateRequest::GetSymbolsInRange(SymbolsInRangeRequest {
            fs_file_path: fs_file_path.to_owned(),
            range: range.clone(),
        });
        let _ = self.sender.send((request, sender));
        let response = receiver.await;
        if let Ok(SharedStateResponse::SymbolsInRangeResponse(response)) = response {
            Some(response)
        } else {
            None
        }
    }
}
