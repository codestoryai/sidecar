//! Contains the symbol type and how its structred and what kind of operations we
//! can do with it, its a lot of things but the end goal is that each symbol in the codebase
//! can be represented by some entity, and that's what we are storing over here
//! Inside each symbol we also have the various implementations of it, which we always
//! keep track of and whenever a question is asked we forward it to all the implementations
//! and select the ones which are necessary.

use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};

use derivative::Derivative;
use futures::{future::Shared, stream, FutureExt, StreamExt};
use llm_client::{
    clients::types::LLMType,
    provider::{GoogleAIStudioKey, LLMProvider, LLMProviderAPIKeys},
};
use logging::parea::{PareaClient, PareaLogEvent};
use tokio::sync::{
    mpsc::{UnboundedReceiver, UnboundedSender},
    Mutex,
};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::info;

use crate::{
    agentic::{
        symbol::{
            events::edit::SymbolToEditRequest,
            helpers::find_needle_position,
            identifier::Snippet,
            ui_event::{SymbolEventProbeRequest, SymbolEventSubStep, SymbolEventSubStepRequest},
        },
        tool::code_symbol::{
            important::{CodeSubSymbolProbingResult, CodeSymbolProbingSummarize},
            models::anthropic::AskQuestionSymbolHint,
        },
    },
    chunking::{
        text_document::{Position, Range},
        types::OutlineNode,
    },
};

use super::{
    errors::SymbolError,
    events::{
        edit::SymbolToEdit,
        initial_request::{InitialRequestData, SymbolEditedItem, SymbolRequestHistoryItem},
        message_event::{SymbolEventMessage, SymbolEventMessageProperties},
        probe::{SymbolToProbeHistory, SymbolToProbeRequest},
        types::{AskQuestionRequest, SymbolEvent},
    },
    identifier::{LLMProperties, MechaCodeSymbolThinking, SymbolIdentifier},
    tool_box::ToolBox,
    tool_properties::ToolProperties,
    ui_event::UIEventWithID,
};

const BUFFER_LIMIT: usize = 100;

#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolSubStepUpdate {
    sybmol: SymbolIdentifier,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolLocation {
    snippet: Snippet,
    symbol_identifier: SymbolIdentifier,
}

impl SymbolLocation {
    pub fn new(symbol_identifier: SymbolIdentifier, snippet: Snippet) -> Self {
        Self {
            snippet,
            symbol_identifier,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolEventRequest {
    symbol: SymbolIdentifier,
    event: SymbolEvent,
    tool_properties: ToolProperties,
}

impl SymbolEventRequest {
    pub fn event(&self) -> &SymbolEvent {
        &self.event
    }

    pub fn symbol(&self) -> &SymbolIdentifier {
        &self.symbol
    }

    pub fn remove_event(self) -> SymbolEvent {
        self.event
    }

    pub fn get_tool_properties(&self) -> &ToolProperties {
        &self.tool_properties
    }
}

impl SymbolEventRequest {
    pub fn new(
        symbol: SymbolIdentifier,
        event: SymbolEvent,
        tool_properties: ToolProperties,
    ) -> Self {
        Self {
            symbol,
            event,
            tool_properties,
        }
    }

    pub fn outline(symbol: SymbolIdentifier, tool_properties: ToolProperties) -> Self {
        Self {
            symbol,
            event: SymbolEvent::Outline,
            tool_properties,
        }
    }

    // no longer used - unless we're clear about a purpose.
    pub fn ask_question(
        symbol: SymbolIdentifier,
        question: String,
        tool_properties: ToolProperties,
    ) -> Self {
        Self {
            symbol,
            event: SymbolEvent::AskQuestion(AskQuestionRequest::new(question)),
            tool_properties,
        }
    }

    pub fn probe_request(
        symbol: SymbolIdentifier,
        request: SymbolToProbeRequest,
        tool_properties: ToolProperties,
    ) -> Self {
        Self {
            symbol,
            event: SymbolEvent::Probe(request),
            tool_properties,
        }
    }

    pub fn initial_request(
        symbol: SymbolIdentifier,
        original_user_query: String,
        request: String,
        // passing history to the symbols so we do not end up doing repeated work
        history: Vec<SymbolRequestHistoryItem>,
        tool_properties: ToolProperties,
        symbols_edited_list: Option<Vec<SymbolEditedItem>>,
        is_big_search: bool,
    ) -> Self {
        Self {
            symbol,
            event: SymbolEvent::InitialRequest(InitialRequestData::new(
                original_user_query,
                request, // we're passing request_id to plan...
                history,
                tool_properties.get_full_symbol_request(),
                symbols_edited_list,
                is_big_search,
            )),
            tool_properties,
        }
    }

    pub fn simple_edit_request(
        symbol_identifier: SymbolIdentifier,
        symbol_to_edit: SymbolToEdit,
        tool_properties: ToolProperties,
    ) -> Self {
        Self {
            symbol: symbol_identifier.to_owned(),
            event: SymbolEvent::Edit(SymbolToEditRequest::new(
                vec![symbol_to_edit],
                symbol_identifier.to_owned(),
                vec![],
            )),
            tool_properties,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SymbolEventResponse {
    TaskDone(String),
}

impl SymbolEventResponse {
    pub fn to_string(self) -> String {
        match self {
            Self::TaskDone(reply) => reply,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EditedCodeSymbol {
    original_code: String,
    edited_code: String,
}

impl EditedCodeSymbol {
    pub fn new(original_code: String, edited_code: String) -> Self {
        Self {
            original_code,
            edited_code,
        }
    }

    pub fn to_string(&self, sub_symbol_to_edit: &SymbolToEdit, instructions: &str) -> String {
        let symbol_name = sub_symbol_to_edit.symbol_name();
        let fs_file_path = sub_symbol_to_edit.fs_file_path();
        let original_code = &self.original_code;
        let edited_code = &self.edited_code;
        format!(
            r#"<sub_symbol_edited>
<sub_symbol_name>
{symbol_name}
</sub_symbol_name>
<fs_file_path>
{fs_file_path}
</fs_file_path>
<instruction>
{instructions}
</instruction>
<original_code>
{original_code}
<original_code>
<edited_code>
{edited_code}
</edited_code>
</sub_symbol_edited>"#
        )
    }
}

/// The symbol is going to spin in the background and keep working on things
/// is this how we want it to work???
/// ideally yes, cause its its own process which will work in the background
/// one of the keys things here is that we want this to be a arcable and clone friendly
/// since we are sending many such events to it at the same time
#[derive(Derivative)]
#[derivative(PartialEq, Eq, Debug, Clone)]
pub struct Symbol {
    symbol_identifier: SymbolIdentifier,
    #[derivative(PartialEq = "ignore")]
    #[derivative(Hash = "ignore")]
    #[derivative(Debug = "ignore")]
    hub_sender: UnboundedSender<SymbolEventMessage>,
    #[derivative(PartialEq = "ignore")]
    #[derivative(Hash = "ignore")]
    #[derivative(Debug = "ignore")]
    tools: Arc<ToolBox>,
    #[derivative(PartialEq = "ignore")]
    #[derivative(Hash = "ignore")]
    #[derivative(Debug = "ignore")]
    // TODO(skcd): this is a skill issue right here
    // we do not want to clone so aggresively here, we should be able to work
    // with just references somehow if we were not mutating our state so much
    mecha_code_symbol: Arc<MechaCodeSymbolThinking>,
    #[derivative(PartialEq = "ignore")]
    #[derivative(Hash = "ignore")]
    #[derivative(Debug = "ignore")]
    llm_properties: LLMProperties,
    #[derivative(PartialEq = "ignore")]
    #[derivative(Hash = "ignore")]
    #[derivative(Debug = "ignore")]
    tool_properties: ToolProperties,
    #[derivative(PartialEq = "ignore")]
    #[derivative(Hash = "ignore")]
    #[derivative(Debug = "ignore")]
    probe_questions_asked: Arc<Mutex<Vec<String>>>,
    #[derivative(PartialEq = "ignore")]
    #[derivative(Hash = "ignore")]
    #[derivative(Debug = "ignore")]
    parea_client: PareaClient,
    #[derivative(PartialEq = "ignore")]
    #[derivative(Hash = "ignore")]
    #[derivative(Debug = "ignore")]
    probe_questions_handler:
        Arc<Mutex<HashMap<String, Shared<tokio::sync::oneshot::Receiver<Option<String>>>>>>,
    #[derivative(PartialEq = "ignore")]
    #[derivative(Hash = "ignore")]
    #[derivative(Debug = "ignore")]
    probe_questions_answer: Arc<Mutex<HashMap<String, Option<String>>>>,
}

impl Symbol {
    pub async fn new(
        symbol_identifier: SymbolIdentifier,
        mecha_code_symbol: MechaCodeSymbolThinking,
        // this can be used to talk to other symbols and get them
        // to act on certain things
        hub_sender: UnboundedSender<SymbolEventMessage>,
        tools: Arc<ToolBox>,
        llm_properties: LLMProperties,
        tool_properties: ToolProperties,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Self, SymbolError> {
        let symbol = Self {
            mecha_code_symbol: Arc::new(mecha_code_symbol),
            symbol_identifier: symbol_identifier.clone(),
            hub_sender,
            tools: tools.clone(),
            llm_properties,
            tool_properties,
            probe_questions_asked: Arc::new(Mutex::new(vec![])),
            parea_client: PareaClient::new(),
            probe_questions_handler: Arc::new(Mutex::new(HashMap::new())),
            probe_questions_answer: Arc::new(Mutex::new(HashMap::new())),
        };
        // grab the implementations of the symbol
        // TODO(skcd): We also have to grab the diagnostics and auto-start any
        // process which we might want to

        // we are going to get stuck here unless we have an ui_sender over here
        symbol
            .mecha_code_symbol
            .grab_implementations(tools, symbol_identifier, message_properties)
            .await?;
        Ok(symbol)
    }

    pub fn mecha_code_symbol(&self) -> Arc<MechaCodeSymbolThinking> {
        self.mecha_code_symbol.clone()
    }

    fn fs_file_path(&self) -> &str {
        self.mecha_code_symbol.fs_file_path()
    }

    fn symbol_name(&self) -> &str {
        self.mecha_code_symbol.symbol_name()
    }

    // find the name of the sub-symbol
    pub async fn find_subsymbol_in_range(
        &self,
        range: &Range,
        fs_file_path: &str,
    ) -> Option<String> {
        self.mecha_code_symbol
            .find_symbol_in_range(range, fs_file_path)
            .await
    }

    async fn get_outline(
        &self,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        // to grab the outline first we need to understand the definition snippet
        // of the node and then create it appropriately
        // First thing we want to do here is to find the symbols which are present
        // in the file and get the one which corresponds to us, once we have that
        // we go to all the implementations and grab them as well and generate
        // the outline which is required
        self.tools
            .outline_nodes_for_symbol(self.fs_file_path(), self.symbol_name(), message_properties)
            .await
    }

    async fn probe_request_handler(
        &self,
        request: SymbolToProbeRequest,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        let original_request_id = request.original_request_id().to_owned();
        // First check the answer hashmap if we already have the answer, and take
        // the answer from there if it already exists
        {
            let answered_questions = self.probe_questions_answer.lock().await;
            if let Some(answer) = answered_questions.get(&original_request_id) {
                return match answer {
                    Some(answer) => Ok(answer.to_string()),
                    None => Err(SymbolError::CachedQueryFailed),
                };
            }
        }
        let receiver: Shared<tokio::sync::oneshot::Receiver<_>>;
        let sender: Option<tokio::sync::oneshot::Sender<_>>;
        {
            let mut ongoing_probe_requests = self.probe_questions_handler.lock().await;
            if let Some(receiver_present) = ongoing_probe_requests.get(&original_request_id) {
                println!(
                    "symbol::probe_request_handler::cache_hit::{}",
                    self.symbol_name()
                );
                receiver = receiver_present.clone();
                sender = None;
            } else {
                println!(
                    "symbol::probe_request_handler::cache_not_present::{}",
                    self.symbol_name(),
                );
                // then we want to call the the probe_request and put the receiver on our end
                let (sender_present, receiver_present) = tokio::sync::oneshot::channel();
                let shared_receiver = receiver_present.shared();
                let _ = ongoing_probe_requests
                    .insert(original_request_id.to_owned(), shared_receiver.clone());
                sender = Some(sender_present);
                receiver = shared_receiver;
            }
        }

        // If we have the sender with us, then we need to really perform the probe work
        // and then poll the receiver for the results
        if let Some(sender) = sender {
            println!(
                "symbol::probe_request_handler::probe_request::({})",
                self.symbol_name()
            );
            let result = self
                .probe_request(request, hub_sender, message_properties.clone())
                .await;
            // update our answer hashmap before sending it over
            {
                let mut answered_question = self.probe_questions_answer.lock().await;
                match &result {
                    Ok(result) => {
                        answered_question
                            .insert(original_request_id.to_owned(), Some(result.to_string()));
                    }
                    Err(e) => {
                        println!("symbol::probe_request_handler::probe_request::answer::error::({})::({:?})", self.symbol_name(), e);
                        answered_question.insert(original_request_id.to_owned(), None);
                    }
                }
            }
            match result {
                Ok(result) => {
                    let _ = sender.send(Some(result));
                }
                Err(e) => {
                    let _ = sender.send(None);
                    Err(e)?;
                }
            };
        }
        println!(
            "symbol::probe_request_handler::probe_request::receiver_wait::({})",
            self.symbol_name()
        );
        let result = receiver.await;
        println!(
            "symbol::probe_request_handler::probe_request::receiver_finished::({})",
            self.symbol_name()
        );
        match result {
            Ok(Some(result)) => Ok(result),
            _ => Err(SymbolError::CachedQueryFailed),
        }
    }

    /// Probing works in the following way:
    /// - We run 2 queries in parallel first:
    /// - - check if we have enough to answer the user query
    /// - - if we have to look deeper into certain sections of the code in the symbol
    /// - If we have to look deeper into certain sections of the code, find the code snippets
    /// where we have to cmd-click and grab them (these are the next probe symbols)
    /// - shoot a request to these next probe symbols with all the questions we have
    /// - wait in parallel and then merge the answer together to answer the user query
    async fn probe_request(
        &self,
        request: SymbolToProbeRequest,
        hub_sender: UnboundedSender<SymbolEventMessage>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        let original_request_id = request.original_request_id().to_owned();
        let original_request_id_ref = &original_request_id;
        // We want to keep track of all the probe requests which are going on for this symbol
        // and make sure that we are not repeating work which has already been done or is ongoing.
        let all_ongoing_probe_requests: Vec<String>;
        {
            let mut probe_requests = self.probe_questions_asked.lock().await;
            probe_requests.push(request.probe_request().to_owned());
            all_ongoing_probe_requests = probe_requests.clone();
        }
        let event_trace_id = uuid::Uuid::new_v4().to_string();
        // log all the probe requests which might be going on or are done for a given symbol
        let _ = self
            .parea_client
            .log_event(PareaLogEvent::new(
                format!("probe_request::{}", self.symbol_name()),
                event_trace_id.to_owned(),
                event_trace_id.to_owned(),
                all_ongoing_probe_requests
                    .into_iter()
                    .enumerate()
                    .map(|(idx, content)| (idx.to_string(), content))
                    .collect(),
            ))
            .await;
        // we can do a cache check here if we already have the answer or are working
        // on the similar request
        // First we refresh our state over here
        self.refresh_state(message_properties.clone()).await;

        let history = request.history();
        let history_slice = request.history_slice();
        let original_request = request.original_request();
        let history_ref = &history;
        let query = request.probe_request();
        let symbol_name = self.mecha_code_symbol.symbol_name();
        let tool_properties_ref = &self.tool_properties;

        let _snippets = self.mecha_code_symbol.get_implementations().await;
        info!(event_name = "refresh_state", symbol_name = symbol_name,);
        // - sub-symbol selection for probing
        // TODO(skcd): We are sending full blocks of implementations over here
        // instead we should be splitting the outline node into parts and then
        // asking which section we want to probe and show that to the LLM
        // for example: struct A { fn something(); fn something_else(); }
        // should break down into struct A {fn something(); }, struct A{ fn something_else(); }
        // instead of being struct A {fn something(); fn something_else(); }

        // TODO(skcd): We run both the queries in parallel:
        // - ask if the symbol can answer the question
        // - or we chose the sub-symbol which we want to focus on
        println!(
            "symbol::probe_request::parallel_requests_start::({})",
            &self.symbol_name()
        );
        let (probe_sub_symbols, probe_deeper_or_enough) = tokio::join!(
            self.mecha_code_symbol.probe_sub_sybmols(
                query,
                self.llm_properties.clone(),
                message_properties.clone(),
            ),
            self.mecha_code_symbol.probe_deeper_or_answer(
                query,
                self.llm_properties.clone(),
                message_properties.clone(),
            ),
        );
        if let Ok(probe_deeper_or_enough) = probe_deeper_or_enough {
            if let Some(answer_user_query) = probe_deeper_or_enough.answer_user_query() {
                // we found the answer very early, so lets just return over here
                let _ = message_properties
                    .ui_sender()
                    .send(UIEventWithID::probe_answer_event(
                        message_properties.request_id_str().to_owned(),
                        self.symbol_identifier.clone(),
                        answer_user_query.to_owned(),
                    ));
                println!(
                    "symbol::probe_request::answer_user_query::({})",
                    self.symbol_name()
                );
                return Ok(answer_user_query);
            }
        }

        // TODO(skcd): We are not getting the right question to ask to the sub-symbols
        // this is more of an observation about the sub-symbol related to our question
        let probe_sub_symbols = probe_sub_symbols?;
        println!(
            "symbol::probe_request::probe_sub_symbols::({})::len({})",
            self.symbol_name(),
            probe_sub_symbols.len()
        );
        let _ = message_properties
            .ui_sender()
            .send(UIEventWithID::sub_symbol_step(
                message_properties.root_request_id().to_owned(),
                SymbolEventSubStepRequest::new(
                    self.symbol_identifier.clone(),
                    SymbolEventSubStep::Probe(SymbolEventProbeRequest::SubSymbolSelection),
                ),
            ));
        println!("Sub symbol request: {:?}", &probe_sub_symbols);

        info!("sub symbol fetching done");

        // If we do end up doing this, we should just send the go-to-definition
        // request over here properly and ask the LLM to follow the symbol
        // the check at the start will stop the probe from going askew
        // and the ones which are required will always work out
        let snippet_to_symbols_to_follow = stream::iter(
            probe_sub_symbols
                .into_iter()
                .map(|data| (data, message_properties.clone())),
        )
        .map(|(probe_sub_symbol, message_properties)| async move {
            println!(
                "symbol::probe_request::probe_sub_symbols::({})({}@{})",
                self.symbol_name(),
                probe_sub_symbol.symbol_name(),
                probe_sub_symbol.fs_file_path(),
            );
            let outline_node = self
                .tools
                .find_sub_symbol_to_probe_with_name(
                    self.symbol_name(),
                    &probe_sub_symbol,
                    message_properties.clone(),
                )
                .await;
            if let Ok(outline_node) = outline_node {
                let snippet = Snippet::new(
                    probe_sub_symbol.symbol_name().to_owned(),
                    outline_node.range().clone(),
                    probe_sub_symbol.fs_file_path().to_owned(),
                    outline_node.content().to_owned(),
                    outline_node.clone(),
                );
                let response = self
                    .tools
                    // TODO(skcd): This seems okay for now, given at the context
                    // of sending the whole symbol before, instead of individual
                    // chunks, so if we send individual snippets here instead
                    // we could get rid of this LLM call over here
                    // [LLM:symbols_to_probe_questions]
                    .probe_deeper_in_symbol(
                        &snippet,
                        probe_sub_symbol.reason(),
                        history_ref,
                        query,
                        self.llm_properties.llm().clone(),
                        self.llm_properties.provider().clone(),
                        self.llm_properties.api_key().clone(),
                        message_properties.clone(),
                    )
                    .await;
                Some((snippet, response))
            } else {
                None
            }
        })
        .buffer_unordered(BUFFER_LIMIT)
        .collect::<Vec<_>>()
        .await;
        let _ = message_properties
            .ui_sender()
            .send(UIEventWithID::sub_symbol_step(
                message_properties.root_request_id().to_owned(),
                SymbolEventSubStepRequest::new(
                    self.symbol_identifier.clone(),
                    SymbolEventSubStep::Probe(SymbolEventProbeRequest::ProbeDeeperSymbol),
                ),
            ));

        println!(
            "Snippet to symbols to follow: {:?}",
            snippet_to_symbols_to_follow
        );

        let symbol_identifier_ref = &self.symbol_identifier;

        // Now for each of the go-to-definition we have to find the snippet and
        // the symbol it belongs to and send the request to the appropriate symbol
        // and let it answer the user query followed by the question which the
        // LLM itself will be asking
        let snippet_to_follow_with_definitions = stream::iter(
            snippet_to_symbols_to_follow
                .into_iter()
                .filter_map(|something| match something {
                    Some((snippet, Ok(response))) => {
                        Some((snippet, response, message_properties.clone()))
                    }
                    _ => None,
                }),
        )
        .map(|(snippet, response, message_properties)| async move {
            let referred_snippet = &snippet;
            // we want to parse the reponse here properly and get back the
            // snippets which are the most important by going to their definitions
            let symbols_to_follow_list = response.symbol_list();
            let definitions_to_follow = stream::iter(
                symbols_to_follow_list
                    .into_iter()
                    .map(|symbol| (symbol, message_properties.clone())),
            )
            .map(|(symbol_to_follow, message_properties)| async move {
                println!(
                    "Go to definition using symbol: {:?} {} {} {}",
                    referred_snippet.range(),
                    symbol_to_follow.file_path(),
                    symbol_to_follow.line_content(),
                    symbol_to_follow.name()
                );
                // the definitions over here might be just the symbols themselves
                // we have to make sure that there is no self-reference and the
                // LLM helps push the world model forward
                let definitions_for_snippet = self
                    .tools
                    .go_to_definition_using_symbol(
                        referred_snippet.range(),
                        symbol_to_follow.file_path(),
                        symbol_to_follow.line_content(),
                        symbol_to_follow.name(),
                        message_properties.clone(),
                    )
                    .await;
                if let Ok(ref definitions_for_snippet) = &definitions_for_snippet {
                    println!(
                        "symbol::probe_request::go_to_definition::ui_event::({})",
                        self.symbol_name()
                    );
                    let _ = message_properties
                        .ui_sender()
                        .send(UIEventWithID::sub_symbol_step(
                            message_properties.root_request_id().to_owned(),
                            SymbolEventSubStepRequest::go_to_definition_request(
                                symbol_identifier_ref.clone(),
                                symbol_to_follow.file_path().to_owned(),
                                definitions_for_snippet.1.clone(),
                                symbol_to_follow.thinking().to_owned(),
                            ),
                        ));
                }
                if let Ok(definitions_for_snippet) = definitions_for_snippet {
                    Some((symbol_to_follow, definitions_for_snippet))
                } else {
                    None
                }
            })
            .buffer_unordered(BUFFER_LIMIT)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .filter_map(|s| s)
            .collect::<Vec<_>>();
            // Now that we have this, we can ask the LLM to generate the next set of probe-queries
            // if required unless it already has the answer
            (snippet, definitions_to_follow)
        })
        // we go through the snippets one by one
        .buffered(100)
        .collect::<Vec<_>>()
        .await;

        println!(
            "Snippet to follow with definitions: {:?}",
            &snippet_to_follow_with_definitions
        );

        // We could be asking the same question to multiple definitions of the symbol
        // TODO(skcd): We should pass the history of the sub-symbol snippet of how we
        // are asking the question, since we are directly going for the go-to-definition
        let probe_results = stream::iter(
            snippet_to_follow_with_definitions
                .to_vec()
                .into_iter()
                .map(|data| (data, message_properties.clone())),
        )
        .map(
            |((snippet, ask_question_symbol_hint), message_properties)| async move {
                // A single snippet can have multiple go-to-definition-requests
                // Here we try to organise them in the form or outline node probe
                // requests
                let questions_with_definitions = ask_question_symbol_hint;
                // Now for the definition we want to follow, we have to do some deduplication
                // based on symbol identifier and the request we are getting.
                // since we might end up getting lot of requests to the same symbol, we need
                // to merge all the asks together.
                let mut question_to_outline_nodes = vec![];
                for (ask_question_hint, definition) in questions_with_definitions.into_iter() {
                    let definitions = definition.2;
                    let outline_nodes = stream::iter(
                        definitions
                            .into_iter()
                            .map(|definition| (definition, message_properties.clone())),
                    )
                    .map(|(definition, message_properties)| async move {
                        let range = definition.0.range();
                        let fs_file_path = definition.0.file_path();
                        println!(
                            "symbol::probe_request::get_outline_node_for_range::position({}, {:?})",
                            fs_file_path, range
                        );
                        let outline_nodes = self
                            .tools
                            .get_outline_node_for_range(range, fs_file_path, message_properties)
                            .await;
                        // log if the outline node is present for this go-to-definition
                        // location
                        match outline_nodes {
                            Ok(outline_node) => {
                                println!(
                                    "symbol::probe_request::go_to_definition::success({})",
                                    self.symbol_name()
                                );
                                Some(outline_node)
                            }
                            Err(_e) => {
                                println!(
                                    "symbol::probe_request::go_to_definition::failure({})",
                                    self.symbol_name()
                                );
                                None
                            }
                        }
                    })
                    .buffer_unordered(100)
                    .collect::<Vec<_>>()
                    .await
                    .into_iter()
                    .filter_map(|s| s)
                    .collect::<Vec<_>>();
                    if outline_nodes.is_empty() {
                        println!(
                        "symbol::probe_request::ask_question_hint::failed::no_outline_nodes::({})",
                        self.symbol_name()
                    );
                    } else {
                        question_to_outline_nodes.push((
                            snippet.clone(),
                            ask_question_hint,
                            outline_nodes,
                        ));
                    }
                }
                question_to_outline_nodes
            },
        )
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await;

        // outline nodes to probe questions
        let mut outline_node_to_probe_question: HashMap<
            OutlineNode,
            Vec<(Snippet, AskQuestionSymbolHint)>,
        > = HashMap::new();
        probe_results.into_iter().flatten().for_each(
            |(snippet, ask_question_hint, outline_nodes)| {
                for outline_node in outline_nodes {
                    if let Some(questions) = outline_node_to_probe_question.get_mut(&outline_node) {
                        questions.push((snippet.clone(), ask_question_hint.clone()));
                    } else {
                        outline_node_to_probe_question.insert(
                            outline_node,
                            vec![(snippet.clone(), ask_question_hint.clone())],
                        );
                    }
                }
            },
        );

        // Now we can create the probe next query for these outline nodes:
        let implementations = self.mecha_code_symbol.get_implementations().await;
        let symbol_to_probe_request =
            stream::iter(outline_node_to_probe_question.into_iter().filter_map(
                |(outline_node, questions)| {
                    // todo something else over here
                    // First we need to check if the outline node belongs to the current symbol
                    // in which case we need to do something special over here, to avoid having
                    // cyclic loops, for now we can even begin with logging them
                    let outline_node_range = outline_node.range();
                    let outline_node_fs_file_path = outline_node.fs_file_path();
                    if implementations.iter().any(|implementation| {
                        implementation
                            .range()
                            .contains_check_line(&outline_node_range)
                            && implementation.file_path() == outline_node_fs_file_path
                    }) {
                        println!(
                            "symbol::probe_request::same_symbol_probing::({})",
                            self.symbol_name()
                        );
                        None
                    } else {
                        Some((outline_node, questions, message_properties.clone()))
                    }
                },
            ))
            .map(
                |(outline_node, linked_snippet_with_questions, message_properties)| async move {
                    // over here we are going to send over a request to the outline nodes
                    // and ask them all the questions we have for them to answer
                    let symbol_identifier = SymbolIdentifier::with_file_path(
                        outline_node.name(),
                        outline_node.fs_file_path(),
                    );

                    // TODO(skcd): Create a new question request here using the data
                    // from the previous queries
                    //             let questions = linked_snippet_with_questions
                    //                 .iter()
                    //                 .map(|(_, question)| question)
                    //                 .collect::<Vec<_>>();
                    //             let mut reason = questions
                    //                 .into_iter()
                    //                 .map(|question| question.thinking().to_owned())
                    //                 .collect::<Vec<_>>()
                    //                 .join("\n");
                    //             reason = format!(
                    //                 r"#<original_question>
                    // {query}
                    // </original_question>
                    // <observation_about_symbol>
                    // {reason}
                    // </observation_about_symbol>#"
                    //             );
                    let mut history = history_slice.to_vec();
                    history.push(SymbolToProbeHistory::new(
                        self.symbol_name().to_owned(),
                        self.fs_file_path().to_owned(),
                        "".to_owned(),
                        query.to_owned(),
                    ));
                    let history_str = history
                        .to_vec()
                        .into_iter()
                        .map(|history| {
                            let symbol_name = history.symbol().to_owned();
                            let fs_file_path = history.fs_file_path().to_owned();
                            let question = history.question().to_owned();
                            format!(
                                r#"<symbol_name>
{symbol_name}
</symbol_name>
<fs_file_path>
{fs_file_path}
</fs_file_path>
<question_asked>
{question}
</question_asked>"#
                            )
                        })
                        .collect::<Vec<_>>();
                    let request = self
                        .tools
                        .probe_query_generation_for_symbol(
                            self.symbol_name(),
                            outline_node.name(),
                            outline_node.fs_file_path(),
                            original_request,
                            history_str,
                            linked_snippet_with_questions,
                            self.llm_properties.clone(),
                            message_properties.clone(),
                        )
                        .await;
                    match request {
                        Ok(request) => {
                            // The history item is not formatted here properly
                            // we need to make sure that we are passing the highlights of the snippets
                            // from where we are asking the question
                            Some((
                                outline_node,
                                SymbolToProbeRequest::new(
                                    symbol_identifier,
                                    request,
                                    original_request.to_owned(),
                                    original_request_id_ref.to_owned(),
                                    history,
                                ),
                            ))
                        }
                        Err(e) => {
                            println!(
                                "symbol::probe_request::probe_query_generation_for_symbol::({:?})",
                                e
                            );
                            None
                        }
                    }
                },
            )
            .buffer_unordered(100)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .filter_map(|s| s)
            .collect::<Vec<_>>();

        if symbol_to_probe_request.is_empty() {
            // TODO(skcd): if this is empty, then we should have our answer ready over here
            // we should probably ask the LLM to illcit an answer
            // marking this as a TODO
            // otherwise its a class and we might have to create a new symbol over here
            let all_contents = self
                .mecha_code_symbol
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
                .tools
                .probe_try_hard_answer(
                    all_contents,
                    LLMProperties::new(
                        LLMType::GeminiPro,
                        LLMProvider::GoogleAIStudio,
                        LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new("".to_owned())),
                    ),
                    original_request,
                    query,
                    message_properties.clone(),
                )
                .await
                .unwrap_or("LLM error, please contact the developers".to_owned());
            let _ = message_properties
                .ui_sender()
                .send(UIEventWithID::probe_answer_event(
                    message_properties.root_request_id().to_owned(),
                    self.symbol_identifier.clone(),
                    response.to_owned(),
                ));
            Ok(response.to_owned())
        } else {
            // send the requests over here to the symbol manager and then await
            // in parallel
            let hub_sender_ref = &hub_sender;
            // log all the symbols we are going to probe to parea
            let _ = self
                .parea_client
                .log_event(PareaLogEvent::new(
                    format!("probe_dependency_edges::{}", self.symbol_name()),
                    message_properties.request_id_str().to_owned(),
                    message_properties.request_id_str().to_owned().to_owned(),
                    vec![
                        ("calling_node".to_owned(), self.symbol_name().to_owned()),
                        (
                            "edges".to_owned(),
                            symbol_to_probe_request
                                .iter()
                                .map(|(outline_node, _)| outline_node.name())
                                .collect::<Vec<_>>()
                                .join(" ,"),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ))
                .await;
            let responses = stream::iter(
                symbol_to_probe_request
                    .into_iter()
                    .map(|symbol_to_probe| (symbol_to_probe, message_properties.clone())),
            )
            .map(
                |((outline_node, symbol_to_probe_request), message_properties)| async move {
                    let symbol_identifier = symbol_to_probe_request.symbol_identifier().clone();
                    let (sender, receiver) = tokio::sync::oneshot::channel();
                    let event = SymbolEventMessage::message_with_properties(
                        SymbolEventRequest::probe_request(
                            symbol_identifier,
                            symbol_to_probe_request,
                            tool_properties_ref.clone(),
                        ),
                        message_properties.set_request_id(uuid::Uuid::new_v4().to_string()),
                        sender,
                    );
                    let _ = hub_sender_ref.clone().send(event);
                    let response = receiver.await.map(|response| response.to_string());
                    match response {
                        Ok(response) => {
                            Some(CodeSubSymbolProbingResult::new(
                                outline_node.name().to_owned(),
                                outline_node.fs_file_path().to_owned(),
                                vec![response],
                                // Here we should have the outline and not the
                                // complete symbol over here
                                outline_node.content().content().to_owned(),
                            ))
                        }
                        Err(e) => {
                            println!(
                                "symbol::probe_request::sub_symbol::probe_result::error({:?})",
                                e
                            );
                            None
                        }
                    }
                },
            )
            .buffer_unordered(100)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .filter_map(|s| s)
            .collect::<Vec<_>>();

            // summarize the results over here properly
            let request = CodeSymbolProbingSummarize::new(
                query.to_owned(),
                history.to_owned(),
                self.mecha_code_symbol.symbol_name().to_owned(),
                self.get_outline(message_properties.clone()).await?,
                self.mecha_code_symbol.fs_file_path().to_owned(),
                responses,
                LLMType::GeminiPro,
                LLMProvider::GoogleAIStudio,
                LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new("".to_owned())),
                message_properties.root_request_id().to_owned(),
            );
            let result = self.tools.probing_results_summarize(request).await;
            let _ = message_properties
                .ui_sender()
                .send(UIEventWithID::probe_answer_event(
                    message_properties.root_request_id().to_owned(),
                    self.symbol_identifier.clone(),
                    result
                        .as_ref()
                        .map(|s| s.to_owned())
                        .unwrap_or("Error with probing answer".to_owned()),
                ));
            println!(
                "Probing finished for {} with result: {:?}",
                &self.mecha_code_symbol.symbol_name(),
                &result
            );
            result
        }
    }

    /// Refreshing the state here implies the following:
    /// - we figure out all the implementations again and also
    /// our core snippet again
    /// - this way even if there have been changes we are almost always
    /// correct
    async fn refresh_state(&self, message_properties: SymbolEventMessageProperties) {
        self.mecha_code_symbol
            .refresh_state(message_properties)
            .await;
    }

    /// Sends additional requests to symbols which need changes or gathering more
    /// information to understand how to solve a problem
    ///
    /// TODO(codestory): This is disabled for now
    async fn _follow_along_requests(
        &self,
        original_request: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        if self.mecha_code_symbol.is_snippet_present().await {
            let symbol_content = self
                .mecha_code_symbol
                .get_symbol_content()
                .await
                .expect("snippet presence implies this would never fail");
            let symbols_to_follow = self
                .tools
                .follow_along_initial_query(
                    symbol_content,
                    original_request,
                    self.llm_properties.llm().clone(),
                    self.llm_properties.provider().clone(),
                    self.llm_properties.api_key().clone(),
                    message_properties.clone(),
                )
                .await?;

            let mut pending_requests_to_hub: Vec<SymbolEventRequest> = vec![];

            // Now that we have the symbols to follow over here, we invoke a go-to-definition
            // for each of these symbols, if there are multiple matches we take the last one for
            // now
            for symbol_to_follow in symbols_to_follow.code_symbols_to_follow().into_iter() {
                let fs_file_path = symbol_to_follow.file_path().to_owned();
                let file_open_result = self
                    .tools
                    .file_open(fs_file_path.to_owned(), message_properties.clone())
                    .await?;
                let _ = self
                    .tools
                    .force_add_document(
                        &fs_file_path,
                        file_open_result.contents_ref(),
                        file_open_result.language(),
                    )
                    .await?;
                let symbol_to_follow_identifier = symbol_to_follow.symbol().to_owned();
                // Now we try to find the line in the file which matches with the line
                // content we are looking for and look for the symbol which contains it
                // one trick over here is that we are working within the scope of the current
                // symbol so we can filter out the content very easily and find the line we are
                // interested in by restricting the search space to be inside the outline node
                // content of the current symbol we are in
                let outline_nodes_maybe = self.tools.get_outline_nodes_grouped(&fs_file_path).await;
                if outline_nodes_maybe.is_none() {
                    continue;
                }
                let outline_nodes = outline_nodes_maybe.expect("is_none to hold above");
                // This should work for all identifiers and everything else
                // TODO(skcd): This logic does not work when we are looking at functions
                // inside the node
                let matching_outline_node = outline_nodes
                    .into_iter()
                    .find(|outline_node| outline_node.name() == self.symbol_name());
                if matching_outline_node.is_none() {
                    println!(
                        "symbol::follow_along_request::matching_outline_node::is_none({})::({})",
                        self.symbol_name(),
                        &symbol_to_follow_identifier,
                    );
                    continue;
                }
                let matching_outline_node = matching_outline_node.expect("is_none above to hold");
                let start_line = matching_outline_node.range().start_line();
                let line_containing_content = matching_outline_node
                    .content()
                    .content()
                    .lines()
                    .enumerate()
                    .map(|(idx, line)| (idx + start_line, line.to_string()))
                    .collect::<Vec<_>>()
                    .into_iter()
                    .find(|(_, line)| line.contains(&symbol_to_follow.line_content()));
                if line_containing_content.is_none() {
                    println!(
                        "symbol::follow_along_request::line_containing_content::is_none({})::({})",
                        self.symbol_name(),
                        &symbol_to_follow_identifier,
                    );
                    continue;
                }
                let line_containing_content =
                    line_containing_content.expect("is_none to hold above");

                // Now we need to find the position of the needle in the line
                let column_position =
                    find_needle_position(&line_containing_content.1, symbol_to_follow.symbol());
                if column_position.is_none() {
                    println!(
                        "symbol::follow_along_request::find_needle_position::is_none({})::({})",
                        self.symbol_name(),
                        &symbol_to_follow_identifier,
                    );
                    continue;
                }
                let column_position = column_position.expect("is_none to hold above");

                // Now that we have the line and column, we execute a go-to-definition on this and grab the symbol
                // which it points to
                let mut definitions = self
                    .tools
                    .go_to_definition(
                        symbol_to_follow.file_path(),
                        Position::new(line_containing_content.0, column_position, 0),
                        message_properties.clone(),
                    )
                    .await?
                    .definitions();

                // Now that we have the definition(s) for the symbol we can choose to send a request
                // to the symbol asking for a change request, we pick the first one over here
                // and then ask it the question along with the user query
                if definitions.is_empty() {
                    println!(
                        "symbol::follow_along_request::definition::is_empty({})::({})",
                        self.symbol_name(),
                        &symbol_to_follow_identifier,
                    );
                    continue;
                }
                let definition = definitions.remove(0);
                // check if definition belongs to one of the implementations we have
                // for the current symbol, that way its not really necessary to follow
                // along over here since we will handle it later on
                let implementations = self.mecha_code_symbol.get_implementations().await;
                if implementations.into_iter().any(|implementation| {
                    let implementation_file_path = implementation.file_path();
                    if definition.file_path() == implementation_file_path
                        && implementation
                            .range()
                            .contains_check_line(definition.range())
                    {
                        true
                    } else {
                        false
                    }
                }) {
                    println!(
                        "symbol::follow_along_request::self_follow_along_found({})::({})",
                        self.symbol_name(),
                        &symbol_to_follow_identifier,
                    );
                    continue;
                }
                let outline_node = self
                    .tools
                    .get_outline_node_for_range(
                        definition.range(),
                        definition.file_path(),
                        message_properties.clone(),
                    )
                    .await?;

                let current_symbol_name = self.symbol_name();
                let thinking = symbol_to_follow.reason_for_selection();
                let request = format!(
                    r#"The user asked the following question:
{original_request}

We were initially at {current_symbol_name} and believe that to satisfy the user query we need to handle the following:

{thinking}

Satisfy the requirement either by making edits or gathering the required information so the user query can be handled at {current_symbol_name}"#
                );

                // Now that we have the outline node, we can create a request which we want to send
                // over to this symbol to handle
                // probing ???
                let symbol_identifier = SymbolIdentifier::with_file_path(
                    outline_node.name(),
                    outline_node.fs_file_path(),
                );

                // TODO(skcd): we want to avoid following ourselves, we should guard against
                // that over here
                if symbol_identifier != self.symbol_identifier {
                    pending_requests_to_hub.push(SymbolEventRequest::probe_request(
                        symbol_identifier.clone(),
                        SymbolToProbeRequest::new(
                            symbol_identifier,
                            request.to_owned(),
                            request,
                            message_properties.root_request_id().to_owned(),
                            vec![],
                        ),
                        self.tool_properties.clone(),
                    ));
                }
            }

            let pending_futures = pending_requests_to_hub
                .into_iter()
                .map(|pending_request| {
                    let (sender, receiver) = tokio::sync::oneshot::channel();
                    let event = SymbolEventMessage::message_with_properties(
                        pending_request,
                        message_properties.clone(),
                        sender,
                    );
                    let _ = self.hub_sender.send(event);
                    receiver
                })
                .collect::<Vec<_>>();

            let responses = stream::iter(pending_futures)
                .map(|pending_receiver| async move {
                    let response = pending_receiver.await;
                    if let Ok(response) = response {
                        Some(response.to_string())
                    } else {
                        None
                    }
                })
                .buffer_unordered(100)
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .filter_map(|s| s)
                .collect::<Vec<_>>();
            println!("{:?}", &responses);
        }

        Ok(())
    }

    /// Take the initial request and generates the edit request which we should
    /// act on if required
    ///
    /// In the future we will be figuring out how to answer a question etc
    async fn generate_initial_request(
        &self,
        request_data: InitialRequestData,
        message_properties: SymbolEventMessageProperties,
        // TODO(codestory): This is a bit wrong, we will figure this out in due time
    ) -> Result<Option<SymbolEventRequest>, SymbolError> {
        println!(
            "symbol::generate_initial_request::symbol_name({})",
            self.symbol_name()
        );
        if self.mecha_code_symbol.is_snippet_present().await {
            let request = if request_data.full_symbol_request() {
                self.mecha_code_symbol
                    .full_symbol_initial_request(
                        &request_data,
                        &self.tool_properties,
                        message_properties.clone(),
                    )
                    .await?
            } else {
                Some(
                    self.mecha_code_symbol
                        .initial_request(
                            self.tools.clone(),
                            &request_data,
                            self.llm_properties.clone(),
                            &self.tool_properties,
                            self.hub_sender.clone(),
                            message_properties.clone(),
                        )
                        .await?,
                )
            };
            Ok(request)
        } else {
            let file_content = self
                .tools
                .file_open(self.fs_file_path().to_owned(), message_properties.clone())
                .await?;
            let file_content_range = Range::new(
                Position::new(0, 0, 0),
                Position::new(file_content.file_content_len(), 0, 0),
            );

            // if the last line is not empty, then we want to create an empty line
            // and then start inserting the code over there
            let sub_symbol_to_edit = SymbolToEdit::new(
                self.symbol_name().to_owned(),
                file_content_range,
                self.fs_file_path().to_owned(),
                vec![request_data.get_plan()],
                false,
                true,
                false,
                request_data.get_original_question().to_owned(),
                request_data
                    .symbols_edited_list()
                    .map(|symbol_edited_list| symbol_edited_list.to_vec()),
                false,
                None,
                true, // should we disable followups and correctness check
                None,
                vec![],
                None,
            );
            let mut history = request_data.history().to_vec();
            history.push(SymbolRequestHistoryItem::new(
                self.symbol_name().to_owned(),
                self.fs_file_path().to_owned(),
                request_data.get_original_question().to_owned(),
                None,
            ));
            let symbol_event_request = SymbolEventRequest::new(
                self.symbol_identifier.clone(),
                SymbolEvent::Edit(SymbolToEditRequest::new(
                    vec![sub_symbol_to_edit],
                    self.symbol_identifier.clone(),
                    history,
                )),
                self.tool_properties.clone(),
            );
            Ok(Some(symbol_event_request))
        }
    }

    // The protocol here is that the questions are just plain text, its on the symbol
    // to decide if it needs to collect more information or make changes, we need to carefully
    // figure that out over here
    // what tools do we provide to the symbol for this?
    async fn _answer_question(&self, _question: &str) -> Result<SymbolEventRequest, SymbolError> {
        // The idea here we want to do is:
        // - We first ask which symbols we want to go towards and also do a global search
        // - We then do a question for any followup changes which we need to do on these other symbols (ask them a query and wait for the result)
        // - Use the returned data to create the final edit or answer question here as required
        // - Finally if our shape has changed we need to schedule followups
        todo!("we need to make sure this works")
    }

    /// Grabs the context which is required for editing, this is necessary to make
    /// the code editing work properly and use the right functions
    ///
    /// We look at the all symbols which are part of the import scope and filter
    /// them down and only include the outlines for it in the context, this way
    /// we reduce the amount of tokens and use a weaker model to generate the data
    pub async fn grab_context_for_editing_faster(
        &self,
        sub_symbol: &SymbolToEdit,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Vec<String>, SymbolError> {
        self.tools
            .rerank_outline_nodes_for_code_editing(sub_symbol, message_properties)
            .await
    }

    // TODO(skcd): Handle the cases where the outline is within a symbol and spread
    // across different lines (as is the case in typescript and python)
    // for now we are focussing on rust
    pub async fn grab_context_for_editing(
        &self,
        subsymbol: &SymbolToEdit,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Vec<String>, SymbolError> {
        // force add the file again because the content might have changed
        let file_contents = self
            .tools
            .file_open(
                subsymbol.fs_file_path().to_owned(),
                message_properties.clone(),
            )
            .await?;
        let _ = self
            .tools
            .force_add_document(
                subsymbol.fs_file_path(),
                file_contents.contents_ref(),
                file_contents.language(),
            )
            .await;
        let file_content = self
            .tools
            .get_file_content(&subsymbol.fs_file_path())
            .await?;
        let symbol_to_edit = self
            .tools
            .find_sub_symbol_to_edit_with_name(
                self.symbol_name(),
                subsymbol,
                message_properties.clone(),
            )
            .await?;
        println!(
            "symbol::grab_context_for_editing::symbol_to_edit\n{:?}",
            &symbol_to_edit
        );
        let selection_range = symbol_to_edit.range();
        let _language = self
            .tools
            .detect_language(&subsymbol.fs_file_path())
            .unwrap_or("".to_owned());
        // we have 2 tools which can be used here and they are both kind of interesting:
        // - one of them is the grab definitions which are relevant
        // - one of them is the global context search
        // - first we try to check if the sub-symbol exists in the file
        let interested_defintiions = self
            .tools
            .gather_important_symbols_with_definition(
                symbol_to_edit.fs_file_path(),
                &file_content,
                selection_range,
                self.llm_properties.llm().clone(),
                self.llm_properties.provider().clone(),
                self.llm_properties.api_key().clone(),
                &subsymbol.instructions().join("\n"),
                self.hub_sender.clone(),
                message_properties.clone(),
                &self.tool_properties,
            )
            .await?;

        // cool now we have all the symbols which are necessary for making the edit
        // and more importantly we have all the context which is required
        // we can send the edit request
        // this is the planning stage at this point, now we can begin the editing
        let outlines = interested_defintiions
            .iter()
            .filter_map(|interesed_definitions| {
                if let Some(interesed_definitions) = interesed_definitions {
                    Some(interesed_definitions.1.to_owned())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        Ok(outlines)
    }

    /// Editing the full symbol using search and replace blocks
    ///
    /// This leads to better edits and performance in general at the cost of speed
    ///
    /// For larger blocks of code, this is preferred
    async fn edit_code_full(
        &self,
        // sub-symbol here referes to the full symbol not an individual symbol
        // inside the symbol itself
        sub_symbol: &SymbolToEdit,
        context: Vec<String>,
        tool_properties: ToolProperties,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<EditedCodeSymbol, SymbolError> {
        let file_content = self
            .tools
            .file_open(
                sub_symbol.fs_file_path().to_owned(),
                message_properties.clone(),
            )
            .await?;
        let content = if sub_symbol.is_new() {
            "".to_owned()
        } else {
            let symbol_to_edit = self
                .tools
                .find_sub_symbol_to_edit_with_name(
                    self.symbol_name(),
                    sub_symbol,
                    message_properties.clone(),
                )
                .await?;
            symbol_to_edit.content().to_owned()
        };
        let symbol_to_edit_range = if sub_symbol.is_new() {
            Range::new(
                Position::new(0, 0, 0),
                Position::new(file_content.file_content_len(), 0, 0),
            )
        } else {
            let symbol_to_edit = self
                .tools
                .find_sub_symbol_to_edit_with_name(
                    self.symbol_name(),
                    sub_symbol,
                    message_properties.clone(),
                )
                .await?;
            symbol_to_edit.range().clone()
        };

        println!(
            "symbol::edit_code_full::symbol_range::({:?})",
            &symbol_to_edit_range
        );

        // check if we should continue editing here
        println!(
            "symbol::edit_code_full::should_edit::start({})",
            self.symbol_name()
        );

        let edited_code = self
            .tools
            .code_editing_with_search_and_replace(
                sub_symbol,
                sub_symbol.fs_file_path(),
                file_content.contents_ref(),
                &symbol_to_edit_range,
                context.join("\n"),
                sub_symbol.instructions().join("\n"),
                &self.symbol_identifier,
                sub_symbol.symbol_edited_list(),
                sub_symbol.user_provided_context(),
                message_properties.clone(),
            )
            .await?;

        if tool_properties.should_apply_edits_directly() {
            let _ = self
                .tools
                .apply_edits_to_editor(
                    sub_symbol.fs_file_path(),
                    &symbol_to_edit_range,
                    &edited_code,
                    // apply these directly skipping any kind of undo-stacks
                    // its raw editor apply
                    true,
                    message_properties.clone(),
                )
                .await;
        }

        Ok(EditedCodeSymbol::new(content, edited_code))
    }

    async fn edit_code(
        &self,
        sub_symbol: &SymbolToEdit,
        context: Vec<String>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<EditedCodeSymbol, SymbolError> {
        // TODO(skcd): This part of the lookup is not correct because we might
        // want to edit the private members of the class and its an outline
        // instead if we pass the outline over here we get back the whole symbol
        // instead
        println!(
            "symbol::edit_code::symbol_name({})::sub_symbol_name({})",
            self.symbol_name(),
            sub_symbol.symbol_name()
        );
        let symbol_to_edit = self
            .tools
            .find_sub_symbol_to_edit_with_name(
                self.symbol_name(),
                sub_symbol,
                message_properties.clone(),
            )
            .await?;
        let content = symbol_to_edit.content().to_owned();
        let (_llm_properties, swe_bench_initial_edit) =
            if let Some(llm_properties) = self.tool_properties.get_swe_bench_code_editing_llm() {
                // if the symbol is extremely long which we want to edit, fallback
                // to an llm with a bigger context window, in this case we use gpt4-32k
                if symbol_to_edit.range().line_size() > 500 {
                    if let Some(llm_properties_long_context) =
                        self.tool_properties.get_long_context_editing_llm()
                    {
                        (llm_properties_long_context, true)
                    } else {
                        (llm_properties, true)
                    }
                } else {
                    (llm_properties, true)
                }
            } else {
                (self.llm_properties.clone(), false)
            };
        let file_content = self
            .tools
            .file_open(
                sub_symbol.fs_file_path().to_owned(),
                message_properties.clone(),
            )
            .await?;

        let response = self
            .tools
            .code_edit(
                sub_symbol.fs_file_path(),
                file_content.contents_ref(),
                symbol_to_edit.range(),
                context.join("\n"),
                sub_symbol.instructions().join("\n"),
                swe_bench_initial_edit,
                Some(symbol_to_edit.name().to_owned()),
                None,
                sub_symbol.symbol_edited_list(),
                &self.symbol_identifier,
                sub_symbol.user_provided_context(),
                message_properties.clone(),
            )
            .await?;
        Ok(EditedCodeSymbol::new(content, response))
    }

    async fn gather_definitions_for_editing(
        &self,
        sub_symbol_to_edit: &SymbolToEdit,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Vec<String>, SymbolError> {
        if !sub_symbol_to_edit.should_gather_definitions_for_editing() {
            return Ok(vec![]);
        }
        if sub_symbol_to_edit.is_new() {
            self.grab_context_for_editing_faster(sub_symbol_to_edit, message_properties.clone())
                .await
        } else {
            if sub_symbol_to_edit.is_full_edit() {
                // TODO(skcd): Limit this so we are fast enough over here, do something
                // anything about this on the fast path
                self.grab_context_for_editing_faster(sub_symbol_to_edit, message_properties.clone())
                    .await
            } else {
                self.grab_context_for_editing(sub_symbol_to_edit, message_properties.clone())
                    .await
            }
        }
    }

    // we are going to edit the symbols over here
    // some challenges:
    // - we want this to be fully parallel first of all
    // - we also have to do the follow-up fixes on this when we are done editing
    // - we have to look at the lsp information as well
    // - we also want it to be fully parallel
    async fn edit_implementations(
        &self,
        edit_request: SymbolToEditRequest,
        message_properties: SymbolEventMessageProperties,
        tool_properties: ToolProperties,
    ) -> Result<SymbolEventResponse, SymbolError> {
        // NOTE: we do not add an entry to the history here because the initial
        // request already adds the entry before sending over the edit
        // here we might want to edit ourselves or generate new code depending
        // on the scope of the changes being made
        let sub_symbols_to_edit = edit_request.symbols();
        println!(
            "symbol::edit_implementations::sub_symbols::({}).len({})",
            self.symbol_name(),
            sub_symbols_to_edit.len()
        );
        let mut changes_made = vec![];
        // edit requires the following:
        // - gathering context for the symbols which the definitions or outlines are required
        // - do a COT to figure out how to go about making the changes
        // - making the edits
        // - following the changed symbol to check on the references and wherever its being used
        for sub_symbol_to_edit in sub_symbols_to_edit.into_iter() {
            let instructions = sub_symbol_to_edit.instructions().to_vec().join("\n");
            println!(
                "symbol::edit_implementation::sub_symbol_to_edit::({})::is_new({:?})",
                sub_symbol_to_edit.symbol_name(),
                sub_symbol_to_edit.is_new(),
            );

            println!(
                "symbol::edit_implementation::gather_definitions_for_editing::({})::should_grab({})",
                self.symbol_name(),
                sub_symbol_to_edit.should_gather_definitions_for_editing(),
            );

            // todo(zi): how long does this take?
            let context_for_editing = self
                .gather_definitions_for_editing(&sub_symbol_to_edit, message_properties.clone())
                .await?;
            println!("symbol::edit_implementation::gather_definitions_for_editing::({})::context_len({})", self.symbol_name(), context_for_editing.len());

            // if this is a new sub-symbol we have to create we have to diverge the
            // implementations a bit or figure out how to edit with a new line added
            // to the end of the symbol
            let edited_code_response =
                if sub_symbol_to_edit.is_new() {
                    let _ = message_properties.ui_sender().send(
                        UIEventWithID::range_selection_for_edit(
                            message_properties.request_id_str().to_owned(),
                            self.symbol_identifier.clone(),
                            sub_symbol_to_edit.range().to_owned(),
                            sub_symbol_to_edit.fs_file_path().to_owned(),
                        ),
                    );
                    println!(
                        "symbol::edit_implementation::add_subsymbol::({})::({})",
                        self.symbol_name(),
                        sub_symbol_to_edit.symbol_name()
                    );
                    // here
                    self.edit_code_full(
                        &sub_symbol_to_edit,
                        context_for_editing.clone(),
                        tool_properties.clone(),
                        message_properties.clone(),
                    )
                    .await?
                } else {
                    // always return the original code which was present here in case of rollbacks
                    let _ = message_properties.ui_sender().send(
                        UIEventWithID::range_selection_for_edit(
                            message_properties.request_id_str().to_owned(),
                            self.symbol_identifier.clone(),
                            sub_symbol_to_edit.range().clone(),
                            sub_symbol_to_edit.fs_file_path().to_owned(),
                        ),
                    );

                    // Toggle between the full edit mode and the accurate edits
                    if sub_symbol_to_edit.is_full_edit() {
                        self.edit_code_full(
                            &sub_symbol_to_edit,
                            context_for_editing.to_vec(),
                            tool_properties.clone(),
                            message_properties.clone(),
                        )
                        .await?
                    } else {
                        // here
                        self.edit_code(
                            &sub_symbol_to_edit,
                            context_for_editing.to_vec(),
                            message_properties.clone(),
                        )
                        .await?
                    }
                };
            let original_code = &edited_code_response.original_code;
            let edited_code = &edited_code_response.edited_code;

            // trim and compare instead of original, cause white spaces leak in at start and end
            if original_code.trim() == edited_code.trim() {
                println!(
                    "symbol::edit_implementation::no_changes::({})::({})",
                    self.symbol_name(),
                    sub_symbol_to_edit.symbol_name()
                );
                continue;
            }

            // send over edited code request
            let _ = message_properties
                .ui_sender()
                .send(UIEventWithID::edited_code(
                    message_properties.request_id_str().to_owned(),
                    self.symbol_identifier.clone(),
                    // we need to reshape our range here for the edited
                    // code so we just make sure that the end line is properly
                    // moved
                    sub_symbol_to_edit
                        .range()
                        .clone()
                        .reshape_for_selection(&edited_code),
                    sub_symbol_to_edit.fs_file_path().to_owned(),
                    edited_code.to_owned(),
                ));
            println!(
                "symbol::edit_implementation::check_code_correctness::({})",
                self.symbol_name()
            );

            // if we have to make sure that followups and correctness checks need
            // to keep happening
            if !sub_symbol_to_edit.should_disable_followups_and_correctness() {
                let cloned_tools = self.tools.clone();
                let parent_symbol_name = self.symbol_name().to_owned();
                let cloned_sub_symbol_to_edit = sub_symbol_to_edit.clone();
                let cloned_symbol_identifier = self.symbol_identifier.clone();
                let cloned_llm_properties = self.llm_properties.clone();
                let cloned_hub_sender = self.hub_sender.clone();
                let cloned_tool_properties = self.tool_properties.clone();
                let cloned_message_properties = message_properties.clone();
                let cloned_cancellation_token = message_properties.cancellation_token();

                // TODO(skcd): This is gonna bite us in the ass later on? maybe
                let _ = tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    let _response = run_with_cancellation(
                        cloned_cancellation_token,
                        cloned_tools.check_code_correctness(
                            &parent_symbol_name,
                            &cloned_sub_symbol_to_edit,
                            cloned_symbol_identifier,
                            cloned_llm_properties.llm().clone(),
                            cloned_llm_properties.provider().clone(),
                            cloned_llm_properties.api_key().clone(),
                            &cloned_tool_properties,
                            vec![],
                            cloned_hub_sender,
                            cloned_message_properties,
                        ),
                    )
                    .await
                    .map(|res| {
                        if let Err(e) = res {
                            eprintln!("Error checking code correctness: {}", e);
                        }
                    });
                });

                // 🪦 follow ups was here - lest we forget 🪦
            } else {
                println!("symbol::edit_implementation::symbol_name({})::followups_and_correctness_check_disabled({})", self.symbol_name(), sub_symbol_to_edit.should_disable_followups_and_correctness());
            }
            changes_made.push(edited_code_response.to_string(&sub_symbol_to_edit, &instructions));
        }
        println!(
            "symbol::edit_implementation::finish::({})",
            self.symbol_name()
        );
        let symbol_name = self.symbol_name();
        let changes = changes_made.join("\n");
        let formatted_changes_made = format!(
            r#"<changes_made_to_symbol>
<name>
{symbol_name}
</name>
<changes>
{changes}
</changes>"#
        );
        Ok(SymbolEventResponse::TaskDone(formatted_changes_made))
    }

    // this is the function which is polling the requests from the hub
    // we also want another loop which allows the agent to wait
    // for the requests which it was waiting for after sending it to the hub
    pub async fn run(
        self,
        receiver: UnboundedReceiver<SymbolEventMessage>,
    ) -> Result<(), SymbolError> {
        println!("Symbol::run({}) at types.rs", self.symbol_name());
        let receiver_stream = UnboundedReceiverStream::new(receiver);
        receiver_stream
            .map(|symbol_event| (symbol_event, self.clone()))
            .map(|(symbol_event, symbol)| async move {
                let ui_sender = symbol_event.ui_sender();
                let cancellation_token = symbol_event.cancellation_token();
                let symbol_event_request = symbol_event.symbol_event_request().clone();
                let request_id_data = symbol_event.request_id_data().clone();
                let message_properties = symbol_event.get_properties().clone();
                let tool_properties = symbol_event_request.get_tool_properties();
                let response_sender = symbol_event.remove_response_sender();
                println!("Symbol::receiver_stream::event::({})", symbol.symbol_name(),);
                let _ = ui_sender.send(UIEventWithID::from_symbol_event(
                    request_id_data.request_id().to_owned(),
                    symbol_event_request.clone(),
                ));
                match symbol_event_request.event().clone() {
                    SymbolEvent::InitialRequest(initial_request) => {
                        println!("Symbol::inital_request: {}", symbol.symbol_name());
                        symbol.refresh_state(message_properties.clone()).await;
                        let initial_request = run_with_cancellation(
                            cancellation_token,
                            symbol.generate_initial_request(
                                initial_request,
                                message_properties.clone(),
                            ),
                        )
                        .await;
                        println!(
                            "Symbol::initial_request::generated({}).is_some({})",
                            symbol.symbol_name(),
                            initial_request.is_some(),
                        );
                        match initial_request {
                            Some(Ok(Some(initial_request))) => {
                                let (sender, receiver) = tokio::sync::oneshot::channel();
                                let event = SymbolEventMessage::message_with_properties(
                                    initial_request.clone(),
                                    message_properties
                                        // since this is the initial request, we will end up generating
                                        // a new request id for this
                                        .set_request_id(uuid::Uuid::new_v4().to_string()),
                                    sender,
                                );
                                let _ = symbol.hub_sender.send(event);
                                let response = receiver.await;
                                if response.is_err() {
                                    println!(
                                        "symbol.hub_sender::({})::initial_request({:?})",
                                        symbol.symbol_name(),
                                        &initial_request,
                                    );
                                }
                                // println!(
                                //     "Response from symbol.hub_sender::({}): {:?}",
                                //     symbol.symbol_name(),
                                //     &response,
                                // );
                                // ideally we want to give this resopnse back to the symbol
                                // so it can keep track of everything that its doing, we will get to that
                                let _ = response_sender.send(SymbolEventResponse::TaskDone(
                                    "initial request done".to_owned(),
                                ));
                                Ok(())
                            }
                            Some(Ok(None)) => {
                                println!("symbol::run::initial_request::empy_response");
                                let _ = response_sender.send(SymbolEventResponse::TaskDone(
                                    "initial request done".to_owned(),
                                ));
                                Ok(())
                            }
                            Some(Err(e)) => {
                                println!("symbol::run::initial_request::({:?})", &e);
                                let _ = response_sender.send(SymbolEventResponse::TaskDone(
                                    "initial request done".to_owned(),
                                ));
                                Err(e)
                            }
                            None => {
                                println!("symbol::run::initial_request::cancelled");
                                let _ = response_sender.send(SymbolEventResponse::TaskDone(
                                    "initial request cancelled".to_owned(),
                                ));
                                Ok(())
                            }
                        }
                    }
                    SymbolEvent::Edit(edit_request) => {
                        // we refresh our state always
                        println!(
                            "symbol::types::symbol_event::edit::refresh_state({})",
                            symbol.symbol_name()
                        );
                        symbol.refresh_state(message_properties.clone()).await;
                        // one of the primary goals here is that we can make edits
                        // everywhere at the same time unless its on the same file
                        // but for now, we are gonna pleb our way and make edits
                        // one by one
                        println!(
                            "symbol::types::symbol_event::edit::edit_implementations({})",
                            symbol.symbol_name()
                        );
                        let response = symbol
                            .edit_implementations(
                                edit_request,
                                message_properties.clone(),
                                tool_properties.clone(),
                            )
                            .await;
                        if let Ok(response) = response {
                            let _ = response_sender.send(response);
                        } else {
                            let _ = response_sender
                                .send(SymbolEventResponse::TaskDone("edit failed".to_owned()));
                        }
                        Ok(())
                    }
                    SymbolEvent::AskQuestion(_ask_question_request) => {
                        // we refresh our state always
                        symbol.refresh_state(message_properties).await;
                        // we will the following in sequence:
                        // - ask for information from surrounding nodes
                        // - refresh the state
                        // - ask for changes which need to be made to the surrounding nodes
                        // - refresh our state
                        // - edit ourselves if required or formulate the answer
                        // - followup
                        // - task 1: sending probes to the world about gathering information
                        todo!("ask question is not implemented yet");
                    }
                    SymbolEvent::Delete => {
                        todo!("delete is not implemented yet");
                    }
                    SymbolEvent::UserFeedback => {
                        todo!("user feedback is not implemented yet");
                    }
                    SymbolEvent::Outline => {
                        // we have been asked to provide an outline of the symbol we are part of
                        // this is a bit easy to do so lets try and finish this
                        let outline = symbol.get_outline(message_properties.clone()).await;
                        let _ = match outline {
                            Ok(outline) => {
                                response_sender.send(SymbolEventResponse::TaskDone(outline))
                            }
                            Err(_) => response_sender.send(SymbolEventResponse::TaskDone(
                                "failed to get outline".to_owned(),
                            )),
                        };
                        Ok(())
                    }
                    SymbolEvent::Probe(probe_request) => {
                        // we make the probe request an explicit request
                        // we are still going to do the same things just
                        // that this one is for gathering answeres
                        let reply = symbol
                            .probe_request_handler(
                                probe_request,
                                symbol.hub_sender.clone(),
                                message_properties.clone(),
                            )
                            .await;
                        let _ = match reply {
                            Ok(reply) => {
                                let _ = message_properties.ui_sender().send(
                                    UIEventWithID::sub_symbol_step(
                                        message_properties.request_id_str().to_owned(),
                                        SymbolEventSubStepRequest::probe_answer(
                                            symbol.symbol_identifier.clone(),
                                            reply.to_owned(),
                                        ),
                                    ),
                                );
                                let _ = response_sender.send(SymbolEventResponse::TaskDone(reply));
                            }
                            Err(e) => {
                                println!("Error when probing: {:?}", e);
                                let _ = message_properties
                                    .ui_sender()
                                    .send(UIEventWithID::sub_symbol_step(
                                    message_properties.request_id_str().to_owned(),
                                    SymbolEventSubStepRequest::probe_answer(
                                        symbol.symbol_identifier.clone(),
                                        "failed to answer the user query because of external error"
                                            .to_owned(),
                                    ),
                                ));
                                let _ = response_sender.send(SymbolEventResponse::TaskDone(
                                    "failed to look depeer to answer user query".to_owned(),
                                ));
                            }
                        };
                        Ok(())
                    }
                }
            })
            .buffer_unordered(1000)
            .collect::<Vec<_>>()
            .await;
        Ok(())
    }
}

pub async fn run_with_cancellation<F, T>(
    cancel_token: tokio_util::sync::CancellationToken,
    future: F,
) -> Option<T>
where
    F: Future<Output = T>,
{
    tokio::select! {
        res = future => Some(res),               // Future completed successfully
        _ = cancel_token.cancelled() => None,    // Cancellation token was triggered
    }
}
