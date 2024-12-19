//! Contains the handler for agnetic requests and how they work

use super::model_selection::LLMClientConfig;
use super::plan::check_session_storage_path;
use super::types::json as json_result;
use axum::response::{sse, IntoResponse, Sse};
use axum::{extract::Query as axumQuery, Extension, Json};
use futures::{stream, StreamExt};
use llm_client::clients::types::{LLMClientCompletionRequest, LLMClientMessage, LLMType};
use llm_client::provider::{
    CodeStoryLLMTypes, CodestoryAccessToken, LLMProvider, LLMProviderAPIKeys,
};
use serde_json::json;
use std::collections::HashMap;
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use super::types::Result;
use crate::agentic::symbol::anchored::AnchoredSymbol;
use crate::agentic::symbol::events::environment_event::{EnvironmentEvent, EnvironmentEventType};
use crate::agentic::symbol::events::input::SymbolEventRequestId;
use crate::agentic::symbol::events::lsp::LSPDiagnosticError;
use crate::agentic::symbol::events::message_event::SymbolEventMessageProperties;
use crate::agentic::symbol::helpers::SymbolFollowupBFS;
use crate::agentic::symbol::identifier::LLMProperties;
use crate::agentic::symbol::scratch_pad::ScratchPadAgent;
use crate::agentic::symbol::tool_properties::ToolProperties;
use crate::agentic::symbol::toolbox::helpers::SymbolChangeSet;
use crate::agentic::symbol::ui_event::{RelevantReference, UIEventWithID};
use crate::agentic::tool::lsp::open_file::OpenFileResponse;
use crate::agentic::tool::plan::service::PlanService;
use crate::agentic::tool::session::session::AideAgentMode;
use crate::chunking::text_document::Range;
use crate::repo::types::RepoRef;
use crate::webserver::plan::{
    check_plan_storage_path, check_scratch_pad_path, plan_storage_directory,
};
use crate::{application::application::Application, user_context::types::UserContext};

use super::types::ApiResponse;

/// Tracks and manages probe requests in a concurrent environment.
/// This struct is responsible for keeping track of ongoing probe requests
pub struct ProbeRequestTracker {
    /// A thread-safe map of running requests, keyed by request ID.
    ///
    /// - Key: String representing the unique request ID.
    /// - Value: JoinHandle for the asynchronous task handling the request.
    pub running_requests:
        Arc<Mutex<HashMap<String, (tokio_util::sync::CancellationToken, Option<JoinHandle<()>>)>>>,
}

impl ProbeRequestTracker {
    pub fn new() -> Self {
        Self {
            running_requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn cancel_request(&self, request_id: &str) {
        let mut running_requests = self.running_requests.lock().await;
        if let Some((cancellation_token, response)) = running_requests.get_mut(request_id) {
            // we abort the running requests
            cancellation_token.cancel();
            if let Some(response) = response {
                response.abort();
            }
        }
    }
}

/// Contains all the data which we will need to trigger the edits
/// Represents metadata for anchored editing operations.
#[derive(Clone)]
struct AnchoredEditingMetadata {
    /// Properties of the message event associated with this editing session.
    message_properties: SymbolEventMessageProperties,
    /// The symbols that are currently focused on in the selection.
    /// These are the primary targets for the editing operation.
    anchored_symbols: Vec<AnchoredSymbol>,
    /// Stores the original content of the files mentioned before editing started.
    /// This allows for comparison and potential rollback if needed.
    /// Key: File path, Value: Original file content
    previous_file_content: HashMap<String, String>,
    /// Stores references to the anchor selection nodes.
    /// These references can be used for navigation or additional context during editing.
    references: Vec<RelevantReference>,
    /// Optional string representing the user's context for this editing session.
    /// This can provide additional information or constraints for the editing process.
    user_context_string: Option<String>,
    /// environment events
    environment_event_sender: UnboundedSender<EnvironmentEvent>,
    /// the scratchpad agent which tracks the state of the request
    scratch_pad_agent: ScratchPadAgent,
    /// current cancellation token for the ongoing query
    cancellation_token: tokio_util::sync::CancellationToken,
}

impl AnchoredEditingMetadata {
    pub fn references(&self) -> &[RelevantReference] {
        &self.references
    }

    pub fn anchored_symbols(&self) -> &[AnchoredSymbol] {
        &self.anchored_symbols
    }
}

pub struct AnchoredEditingTracker {
    // right now our cache is made up of file path to the file content and this is the cache
    // which we pass to the agents when we startup
    // we update the cache only when we have a hit on a new request
    cache_right_now: Arc<Mutex<Vec<OpenFileResponse>>>,
    running_requests_properties: Arc<Mutex<HashMap<String, AnchoredEditingMetadata>>>,
    running_requests: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
}

impl AnchoredEditingTracker {
    pub fn new() -> Self {
        Self {
            cache_right_now: Arc::new(Mutex::new(vec![])),
            running_requests_properties: Arc::new(Mutex::new(HashMap::new())),
            running_requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn get_properties(&self, request_id: &str) -> Option<AnchoredEditingMetadata> {
        let running_requests = self.running_requests_properties.lock().await;
        running_requests.get(request_id).map(|data| data.clone())
    }

    /// this replaces the existing references field
    async fn _add_reference(&self, request_id: &str, relevant_refs: &[RelevantReference]) {
        let mut running_request_properties = self.running_requests_properties.lock().await;

        if let Some(metadata) = running_request_properties.get_mut(request_id) {
            metadata.references = relevant_refs.to_vec();
        } else {
            println!("No metadata found for request_id: {}", request_id);
        }
    }

    // consider better error handling
    pub async fn add_join_handle(
        &self,
        request_id: &str,
        join_handle: JoinHandle<()>,
    ) -> Result<(), String> {
        let mut running_requests = self.running_requests.lock().await;
        if running_requests.contains_key(request_id) {
            running_requests.insert(request_id.to_owned(), join_handle);
            Ok(())
        } else {
            Err(format!(
                "No existing request found for request_id: {}",
                request_id
            ))
        }
    }

    pub async fn override_running_request(&self, request_id: &str, join_handle: JoinHandle<()>) {
        {
            let mut running_requests = self.running_requests.lock().await;
            running_requests.insert(request_id.to_owned(), join_handle);
        }
    }

    pub async fn scratch_pad_agent(
        &self,
        request_id: &str,
    ) -> Option<(ScratchPadAgent, UnboundedSender<EnvironmentEvent>)> {
        let scratch_pad_agent;
        {
            scratch_pad_agent = self
                .running_requests_properties
                .lock()
                .await
                .get(request_id)
                .map(|properties| {
                    (
                        properties.scratch_pad_agent.clone(),
                        properties.environment_event_sender.clone(),
                    )
                });
        }
        scratch_pad_agent
    }

    pub async fn cached_content(&self) -> String {
        let cached_content = self
            .cache_right_now
            .lock()
            .await
            .iter()
            .map(|open_file_response| {
                let fs_file_path = open_file_response.fs_file_path();
                let language_id = open_file_response.language();
                let content = open_file_response.contents_ref();
                format!(
                    r#"# FILEPATH: {fs_file_path}
```{language_id}
{content}
```"#
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        cached_content
    }

    pub async fn cancel_request(&self, request_id: &str) {
        {
            if let Some(properties) = self
                .running_requests_properties
                .lock()
                .await
                .get(request_id)
            {
                println!("anchored_editing_tracker::cancelling_request");
                // cancel the ongoing request over here
                properties.cancellation_token.cancel();
            }
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProbeRequestActiveWindow {
    file_path: String,
    file_content: String,
    language: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProbeStopRequest {
    request_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProbeStopResponse {
    done: bool,
}

pub async fn probe_request_stop(
    Extension(app): Extension<Application>,
    Json(ProbeStopRequest { request_id }): Json<ProbeStopRequest>,
) -> Result<impl IntoResponse> {
    println!("webserver::probe_request_stop");
    let probe_request_tracker = app.probe_request_tracker.clone();
    let _ = probe_request_tracker.cancel_request(&request_id).await;
    let anchored_editing_tracker = app.anchored_request_tracker.clone();
    let _ = anchored_editing_tracker.cancel_request(&request_id).await;
    Ok(Json(ProbeStopResponse { done: true }))
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SWEBenchRequest {
    git_dname: String,
    problem_statement: String,
    editor_url: String,
    test_endpoint: String,
    // This is the file path with the repo map present in it
    repo_map_file: Option<String>,
    gcloud_access_token: String,
    swe_bench_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SweBenchCompletionResponse {
    done: bool,
}

impl ApiResponse for SweBenchCompletionResponse {}

pub async fn swe_bench(
    axumQuery(SWEBenchRequest {
        git_dname: _git_dname,
        problem_statement: _problem_statement,
        editor_url: _editor_url,
        test_endpoint: _test_endpoint,
        repo_map_file: _repo_map_file,
        gcloud_access_token: _glcoud_access_token,
        swe_bench_id: _swe_bench_id,
    }): axumQuery<SWEBenchRequest>,
    Extension(_app): Extension<Application>,
) -> Result<impl IntoResponse> {
    // let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    // let tool_broker = Arc::new(ToolBroker::new(
    //     app.llm_broker.clone(),
    //     Arc::new(CodeEditBroker::new()),
    //     app.symbol_tracker.clone(),
    //     app.language_parsing.clone(),
    //     // for swe-bench tests we do not care about tracking edits
    //     ToolBrokerConfiguration::new(None, true),
    //     LLMProperties::new(
    //         LLMType::GeminiPro,
    //         LLMProvider::GoogleAIStudio,
    //         LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new(
    //             "".to_owned(),
    //         )),
    //     ),
    // ));
    // let user_context = UserContext::new(vec![], vec![], None, vec![git_dname]);
    // let model = LLMType::ClaudeSonnet;
    // let provider_type = LLMProvider::Anthropic;
    // let anthropic_api_keys = LLMProviderAPIKeys::Anthropic(AnthropicAPIKey::new("".to_owned()));
    // let symbol_manager = SymbolManager::new(
    //     tool_broker,
    //     app.symbol_tracker.clone(),
    //     app.editor_parsing.clone(),
    //     LLMProperties::new(
    //         model.clone(),
    //         provider_type.clone(),
    //         anthropic_api_keys.clone(),
    //     ),
    // );

    // let message_properties = SymbolEventMessageProperties::new(
    //     SymbolEventRequestId::new(swe_bench_id.to_owned(), swe_bench_id.to_owned()),
    //     sender.clone(),
    //     editor_url.to_owned(),
    // );

    println!("we are getting a hit at this endpoint");

    // Now we send the original request over here and then await on the sender like
    // before
    // tokio::spawn(async move {
    //     let _ = symbol_manager
    //         .initial_request(
    //             SymbolInputEvent::new(
    //                 user_context,
    //                 model,
    //                 provider_type,
    //                 anthropic_api_keys,
    //                 problem_statement,
    //                 "web_server_input".to_owned(),
    //                 "web_server_input".to_owned(),
    //                 Some(test_endpoint),
    //                 repo_map_file,
    //                 None,
    //                 None,
    //                 None,
    //                 None,
    //                 None,
    //                 false,
    //                 None,
    //                 None,
    //                 false,
    //                 sender,
    //             )
    //             .set_swe_bench_id(swe_bench_id),
    //             message_properties,
    //         )
    //         .await;
    // });
    // let event_stream = Sse::new(
    //     tokio_stream::wrappers::UnboundedReceiverStream::new(receiver).map(|event| {
    //         sse::Event::default()
    //             .json_data(event)
    //             .map_err(anyhow::Error::new)
    //     }),
    // );

    // // return the stream as a SSE event stream over here
    // Ok(event_stream.keep_alive(
    //     sse::KeepAlive::new()
    //         .interval(Duration::from_secs(3))
    //         .event(
    //             sse::Event::default()
    //                 .json_data(json!({
    //                     "keep_alive": "alive"
    //                 }))
    //                 .expect("json to not fail in keep alive"),
    //         ),
    // ))
    Ok(json_result(SweBenchCompletionResponse { done: true }))
}

/// Represents a request to warm up the code sculpting system.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CodeSculptingWarmup {
    file_paths: Vec<String>,
    grab_import_nodes: bool,
    editor_url: String,
    access_token: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CodeSculptingHeal {
    request_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CodeSculptingHealResponse {
    done: bool,
}

impl ApiResponse for CodeSculptingHealResponse {}

pub async fn code_sculpting_heal(
    Extension(app): Extension<Application>,
    Json(CodeSculptingHeal { request_id }): Json<CodeSculptingHeal>,
) -> Result<impl IntoResponse> {
    println!(
        "webserver::code_sculpting_heal::request_id({})",
        &request_id
    );
    let anchor_properties;
    {
        let anchor_tracker = app.anchored_request_tracker.clone();
        anchor_properties = anchor_tracker.get_properties(&request_id).await;
    }
    println!(
        "code_sculpting::heal::request_id({})::properties_present({})",
        request_id,
        anchor_properties.is_some()
    );
    if anchor_properties.is_none() {
        Ok(json_result(CodeSculptingHealResponse { done: false }))
    } else {
        let anchor_properties = anchor_properties.expect("is_none to hold");

        let anchored_symbols = anchor_properties.anchored_symbols();

        let relevant_references = anchor_properties.references();
        println!(
            "agentic::webserver::code_sculpting_heal::relevant_references.len({})",
            relevant_references.len()
        );

        let file_paths = anchored_symbols
            .iter()
            .filter_map(|r| r.fs_file_path())
            .collect::<Vec<_>>();

        let older_file_content_map = anchor_properties.previous_file_content;
        let message_properties = anchor_properties.message_properties.clone();

        // Now grab the symbols which have changed
        let cloned_tools = app.tool_box.clone();
        let symbol_change_set: HashMap<String, SymbolChangeSet> =
            stream::iter(file_paths.into_iter().map(|file_path| {
                let older_file_content = older_file_content_map
                    .get(&file_path)
                    .map(|content| content.to_owned());
                (
                    file_path,
                    cloned_tools.clone(),
                    older_file_content,
                    message_properties.clone(),
                )
            }))
            .map(
                |(fs_file_path, tools, older_file_content, message_properties)| async move {
                    if let Some(older_content) = older_file_content {
                        let file_content = tools
                            .file_open(fs_file_path.to_owned(), message_properties)
                            .await
                            .ok();
                        if let Some(new_content) = file_content {
                            tools
                                .get_symbol_change_set(
                                    &fs_file_path,
                                    &older_content,
                                    new_content.contents_ref(),
                                )
                                .await
                                .ok()
                                .map(|symbol_change_set| (fs_file_path, symbol_change_set))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                },
            )
            .buffer_unordered(10)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .filter_map(|s| s)
            .collect::<HashMap<_, _>>();

        let changed_symbols = anchor_properties
            .anchored_symbols
            .into_iter()
            .filter_map(|anchored_symbol| {
                let symbol_identifier = anchored_symbol.identifier().to_owned();
                let fs_file_path = symbol_identifier.fs_file_path();
                if fs_file_path.is_none() {
                    return None;
                }
                let fs_file_path = fs_file_path.clone().expect("is_none to hold");
                let changed_symbols_in_file = symbol_change_set.get(&fs_file_path);
                if let Some(changed_symbols_in_file) = changed_symbols_in_file {
                    let symbol_changes = changed_symbols_in_file
                        .changes()
                        .into_iter()
                        .filter(|changed_symbol| {
                            changed_symbol.symbol_identifier().symbol_name()
                                == symbol_identifier.symbol_name()
                        })
                        .map(|changed_symbol| changed_symbol.clone())
                        .collect::<Vec<_>>();
                    Some(symbol_changes)
                } else {
                    None
                }
            })
            .flatten()
            .collect::<Vec<_>>();

        println!(
            "webserver::agentic::changed_symbols: \n{:?}",
            &changed_symbols
        );

        // changed symbols also has symbol_identifier
        let followup_bfs_request = changed_symbols
            .into_iter()
            .map(|changes| {
                let symbol_identifier = changes.symbol_identifier().clone();
                let symbol_identifier_ref = &symbol_identifier;
                changes
                    .remove_changes()
                    .into_iter()
                    .map(|symbol_to_edit| {
                        SymbolFollowupBFS::new(
                            symbol_to_edit.0,
                            symbol_identifier_ref.clone(),
                            symbol_to_edit.1,
                            symbol_to_edit.2,
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .flatten()
            .collect::<Vec<_>>();
        // make sure that the edit request we are creating is on the whole outline
        // node and not on the individual function

        let hub_sender = app.symbol_manager.hub_sender();
        let cloned_tools = app.tool_box.clone();
        let _join_handle = tokio::spawn(async move {
            let _ = cloned_tools
                .check_for_followups_bfs(
                    followup_bfs_request,
                    hub_sender,
                    message_properties.clone(),
                    &ToolProperties::new(),
                )
                .await;

            // send event after we are done with the followups
            let ui_sender = message_properties.ui_sender();
            let _ = ui_sender.send(UIEventWithID::finish_edit_request(
                message_properties.request_id_str().to_owned(),
            ));
        });
        Ok(json_result(CodeSculptingHealResponse { done: true }))
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CodeSculptingRequest {
    request_id: String,
    instruction: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CodeSculptingResponse {
    done: bool,
}

impl ApiResponse for CodeSculptingResponse {}

pub async fn code_sculpting(
    Extension(app): Extension<Application>,
    Json(CodeSculptingRequest {
        request_id,
        instruction,
    }): Json<CodeSculptingRequest>,
) -> Result<impl IntoResponse> {
    let anchor_properties;
    {
        let anchor_tracker = app.anchored_request_tracker.clone();
        anchor_properties = anchor_tracker.get_properties(&request_id).await;
    }
    println!(
        "code_sculpting::instruction({})::properties_present({})",
        instruction,
        anchor_properties.is_some()
    );
    if anchor_properties.is_none() {
        Ok(json_result(CodeSculptingResponse { done: false }))
    } else {
        let anchor_properties = anchor_properties.expect("is_none to hold");
        let join_handle = tokio::spawn(async move {
            let anchored_symbols = anchor_properties.anchored_symbols;
            let user_provided_context = anchor_properties.user_context_string;
            let environment_sender = anchor_properties.environment_event_sender;
            let message_properties = anchor_properties.message_properties.clone();
            let _ = environment_sender.send(EnvironmentEvent::event(
                EnvironmentEventType::human_anchor_request(
                    instruction,
                    anchored_symbols,
                    user_provided_context,
                ),
                message_properties,
            ));
        });
        {
            let anchor_tracker = app.anchored_request_tracker.clone();
            let _ = anchor_tracker
                .override_running_request(&request_id, join_handle)
                .await;
        }
        Ok(json_result(CodeSculptingResponse { done: true }))
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgenticDiagnosticData {
    message: String,
    range: Range,
    range_content: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgenticDiagnostics {
    fs_file_path: String,
    diagnostics: Vec<AgenticDiagnosticData>,
    source: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgenticDiagnosticsResponse {
    done: bool,
}

impl ApiResponse for AgenticDiagnosticsResponse {}

pub async fn push_diagnostics(
    Extension(_app): Extension<Application>,
    Json(AgenticDiagnostics {
        fs_file_path,
        diagnostics,
        source: _source,
    }): Json<AgenticDiagnostics>,
) -> Result<impl IntoResponse> {
    // implement this api endpoint properly and send events over to the right
    // scratch-pad agent
    let _ = diagnostics
        .into_iter()
        .map(|webserver_diagnostic| {
            LSPDiagnosticError::new(
                webserver_diagnostic.range,
                webserver_diagnostic.range_content,
                fs_file_path.to_owned(),
                webserver_diagnostic.message,
                None,
                None,
            )
        })
        .collect::<Vec<_>>();

    // now look at all the active scratch-pad agents and send them this event
    // let _ = app
    //     .anchored_request_tracker
    //     .send_diagnostics_event(lsp_diagnostics)
    //     .await;
    Ok(json_result(AgenticDiagnosticsResponse { done: true }))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgenticEditFeedbackExchange {
    exchange_id: String,
    session_id: String,
    step_index: Option<usize>,
    editor_url: String,
    accepted: bool,
    access_token: String,
    model_configuration: LLMClientConfig,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgenticHandleSessionUndo {
    session_id: String,
    exchange_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgenticHandleSessionUndoResponse {
    done: bool,
}

impl ApiResponse for AgenticHandleSessionUndoResponse {}

pub async fn handle_session_undo(
    Extension(app): Extension<Application>,
    Json(AgenticHandleSessionUndo {
        session_id,
        exchange_id,
    }): Json<AgenticHandleSessionUndo>,
) -> Result<impl IntoResponse> {
    println!("webserver::agent_session::handle_session_undo::hit");
    println!(
        "webserver::agent_session::handle_session_undo::session_id({})",
        &session_id
    );

    let session_storage_path =
        check_session_storage_path(app.config.clone(), session_id.to_string()).await;

    let session_service = app.session_service.clone();
    let _ = session_service
        .handle_session_undo(&exchange_id, session_storage_path)
        .await;
    Ok(Json(AgenticHandleSessionUndoResponse { done: true }))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgenticEditFeedbackExchangeResponse {
    success: bool,
}

impl ApiResponse for AgenticEditFeedbackExchangeResponse {}

pub async fn user_feedback_on_exchange(
    Extension(app): Extension<Application>,
    Json(AgenticEditFeedbackExchange {
        exchange_id,
        session_id,
        step_index,
        editor_url,
        accepted,
        access_token,
        model_configuration,
    }): Json<AgenticEditFeedbackExchange>,
) -> Result<impl IntoResponse> {
    let llm_provider = model_configuration
        .llm_properties_for_slow_model()
        .unwrap_or(LLMProperties::new(
            LLMType::ClaudeSonnet,
            LLMProvider::CodeStory(CodeStoryLLMTypes::new()),
            LLMProviderAPIKeys::CodeStory(CodestoryAccessToken::new(access_token.to_owned())),
        ));
    // give this as feedback to the agent to make sure that it can react to it (ideally)
    // for now we are gonig to close the exchange if it was not closed already
    println!("webserver::agent_session::feedback_on_exchange::hit");
    println!(
        "webserver::agent_session::feedback_on_exchange::session_id({})",
        &session_id
    );
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let message_properties = SymbolEventMessageProperties::new(
        SymbolEventRequestId::new(exchange_id.to_owned(), session_id.to_string()),
        sender.clone(),
        editor_url,
        cancellation_token.clone(),
        llm_provider,
    );

    let session_storage_path =
        check_session_storage_path(app.config.clone(), session_id.to_string()).await;

    let session_service = app.session_service.clone();
    let _ = tokio::spawn(async move {
        let _ = session_service
            .feedback_for_exchange(
                &exchange_id,
                step_index,
                accepted,
                session_storage_path,
                app.tool_box.clone(),
                message_properties,
            )
            .await;
    });

    // TODO(skcd): Over here depending on the exchange reply mode we want to send over the
    // response using ui_sender with the correct exchange_id and the thread_id
    // do we go for a global ui_sender which is being sent to a sink which sends over the data
    // to the editor via http or streaming or whatever (keep an active conneciton always?)
    // how do we notify when the streaming is really completed

    let ui_event_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);
    let cloned_session_id = session_id.to_string();
    let init_stream = futures::stream::once(async move {
        Ok(sse::Event::default()
            .json_data(json!({
                "session_id": cloned_session_id,
                "started": true,
            }))
            // This should never happen, so we force an unwrap.
            .expect("failed to serialize initialization object"))
    });

    // We know the stream is unwind safe as it doesn't use synchronization primitives like locks.
    let answer_stream = ui_event_stream.map(|ui_event: UIEventWithID| {
        sse::Event::default()
            .json_data(ui_event)
            .map_err(anyhow::Error::new)
    });

    // TODO(skcd): Re-introduce this again when we have a better way to manage
    // server side events on the client side

    // this will never get sent cause the sender is never dropped in a way, it will be
    // dropped once we have completed the tokio::spawn above
    let done_stream = futures::stream::once(async move {
        Ok(sse::Event::default()
            .json_data(json!(
                {"done": "[CODESTORY_DONE]".to_owned(),
                "session_id": session_id.to_string(),
            }))
            .expect("failed to send done object"))
    });

    let stream = init_stream.chain(answer_stream).chain(done_stream);

    Ok(Sse::new(Box::pin(stream)))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgenticCancelRunningExchange {
    exchange_id: String,
    session_id: String,
    editor_url: String,
    access_token: String,
    model_configuration: LLMClientConfig,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgenticCancelRunningExchangeResponse {
    success: bool,
}

impl ApiResponse for AgenticCancelRunningExchangeResponse {}

/// TODO(skcd): Figure out how to cancel a running request properly over here
pub async fn cancel_running_exchange(
    Extension(app): Extension<Application>,
    Json(AgenticCancelRunningExchange {
        exchange_id,
        session_id,
        editor_url,
        access_token,
        model_configuration,
    }): Json<AgenticCancelRunningExchange>,
) -> Result<impl IntoResponse> {
    let llm_provider = model_configuration
        .llm_properties_for_slow_model()
        .unwrap_or(LLMProperties::new(
            LLMType::ClaudeSonnet,
            LLMProvider::CodeStory(CodeStoryLLMTypes::new()),
            LLMProviderAPIKeys::CodeStory(CodestoryAccessToken::new(access_token.to_owned())),
        ));

    println!(
        "cancel_running_exchange::session_id({})::exchange_id({})",
        session_id, exchange_id
    );
    let session_service = app.session_service.clone();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let message_properties = SymbolEventMessageProperties::new(
        SymbolEventRequestId::new(exchange_id.to_owned(), session_id.to_string()),
        sender.clone(),
        editor_url,
        cancellation_token.clone(),
        llm_provider,
    );
    if let Some(cancellation_token) = session_service
        .get_cancellation_token(&session_id, &exchange_id)
        .await
    {
        println!(
            "cancel_running_exchange::session_id({})::exchange_id({})::cancelled",
            session_id,
            exchange_id.to_owned()
        );
        cancellation_token.cancel();
        // we should also notify the editor that we have cancelled the request
        // bring this back later
        println!("webserver::agent_session::cancel_running_exchange::hit");
        println!(
            "webserver::agent_session::cancel_running_exchange::session_id({})",
            &session_id
        );

        // give ourselves some time to cleanup before we start working on the cancellation
        // zi: doubling this to halve the number of people discovering this condition
        let _ = tokio::time::sleep(Duration::from_millis(600)).await;
        println!(
            "webserver::agent_session::loading_from_storage::({})",
            &exchange_id
        );
        let session_storage_path =
            check_session_storage_path(app.config.clone(), session_id.to_string()).await;

        // we can either set the signal over here as cancelled (in which case the exchange
        // finished without destroying the world) or we we have to let the user
        // know that there are some edits associated with the current run and the user
        // should see the approve and reject flow
        session_service
            .set_exchange_as_cancelled(
                session_storage_path,
                exchange_id.to_owned(),
                message_properties,
            )
            .await
            .unwrap_or_default();

        let _ = sender.send(UIEventWithID::request_cancelled(
            session_id.to_owned(),
            exchange_id,
        ));
    }

    // send over the events on the stream
    let ui_event_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);
    let cloned_session_id = session_id.to_string();
    let init_stream = futures::stream::once(async move {
        Ok(sse::Event::default()
            .json_data(json!({
                "session_id": cloned_session_id,
                "started": true,
            }))
            // This should never happen, so we force an unwrap.
            .expect("failed to serialize initialization object"))
    });

    // We know the stream is unwind safe as it doesn't use synchronization primitives like locks.
    let answer_stream = ui_event_stream.map(|ui_event: UIEventWithID| {
        sse::Event::default()
            .json_data(ui_event)
            .map_err(anyhow::Error::new)
    });

    // TODO(skcd): Re-introduce this again when we have a better way to manage
    // server side events on the client side

    // this will never get sent cause the sender is never dropped in a way, it will be
    // dropped once we have completed the tokio::spawn above
    let done_stream = futures::stream::once(async move {
        Ok(sse::Event::default()
            .json_data(json!(
                {"done": "[CODESTORY_DONE]".to_owned(),
                "session_id": session_id.to_string(),
            }))
            .expect("failed to send done object"))
    });

    let stream = init_stream.chain(answer_stream).chain(done_stream);

    Ok(Sse::new(Box::pin(stream)))
}

/// We keep track of the thread-id over here
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentSessionChatRequest {
    session_id: String,
    exchange_id: String,
    editor_url: String,
    query: String,
    user_context: UserContext,
    // The mode in which we want to reply to the exchanges
    // agent_mode: AideAgentMode,
    repo_ref: RepoRef,
    root_directory: String,
    project_labels: Vec<String>,
    #[serde(default)]
    codebase_search: bool,
    access_token: String,
    model_configuration: LLMClientConfig,
    all_files: Vec<String>,
    open_files: Vec<String>,
    shell: String,
}

/// Handles the agent session and either creates it or appends to it
///
/// Whenever we try to do an anchored or agentic editing we also go through this flow
pub async fn agent_session_chat(
    Extension(app): Extension<Application>,
    Json(AgentSessionChatRequest {
        session_id,
        exchange_id,
        editor_url,
        query,
        user_context,
        // agent_mode,
        repo_ref,
        project_labels,
        root_directory: _root_directory,
        codebase_search: _codebase_search,
        access_token,
        model_configuration,
        all_files: _all_files,
        open_files: _open_files,
        shell: _shell,
    }): Json<AgentSessionChatRequest>,
) -> Result<impl IntoResponse> {
    let llm_provider = model_configuration
        .llm_properties_for_slow_model()
        .unwrap_or(LLMProperties::new(
            LLMType::ClaudeSonnet,
            LLMProvider::CodeStory(CodeStoryLLMTypes::new()),
            LLMProviderAPIKeys::CodeStory(CodestoryAccessToken::new(access_token.to_owned())),
        ));
    println!("llm_provider::{:?}", &llm_provider);
    // bring this back later
    let agent_mode = AideAgentMode::Chat;
    println!("webserver::agent_session::chat::hit");
    println!(
        "webserver::agent_session::chat::session_id({})",
        &session_id
    );
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let message_properties = SymbolEventMessageProperties::new(
        SymbolEventRequestId::new(exchange_id.to_owned(), session_id.to_string()),
        sender.clone(),
        editor_url,
        cancellation_token.clone(),
        llm_provider,
    );

    let session_storage_path =
        check_session_storage_path(app.config.clone(), session_id.to_string()).await;

    let session_service = app.session_service.clone();
    let cloned_session_id = session_id.to_string();
    let _ = tokio::spawn(async move {
        let _ = session_service
            .human_message(
                cloned_session_id,
                session_storage_path,
                exchange_id,
                query,
                user_context,
                project_labels,
                repo_ref,
                agent_mode,
                message_properties,
            )
            .await;
    });

    // TODO(skcd): Over here depending on the exchange reply mode we want to send over the
    // response using ui_sender with the correct exchange_id and the thread_id
    // do we go for a global ui_sender which is being sent to a sink which sends over the data
    // to the editor via http or streaming or whatever (keep an active conneciton always?)
    // how do we notify when the streaming is really completed

    let ui_event_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);
    let cloned_session_id = session_id.to_string();
    let init_stream = futures::stream::once(async move {
        Ok(sse::Event::default()
            .json_data(json!({
                "session_id": cloned_session_id,
                "started": true,
            }))
            // This should never happen, so we force an unwrap.
            .expect("failed to serialize initialization object"))
    });

    // We know the stream is unwind safe as it doesn't use synchronization primitives like locks.
    let answer_stream = ui_event_stream.map(|ui_event: UIEventWithID| {
        sse::Event::default()
            .json_data(ui_event)
            .map_err(anyhow::Error::new)
    });

    // TODO(skcd): Re-introduce this again when we have a better way to manage
    // server side events on the client side

    // this will never get sent cause the sender is never dropped in a way, it will be
    // dropped once we have completed the tokio::spawn above
    let done_stream = futures::stream::once(async move {
        Ok(sse::Event::default()
            .json_data(json!(
                {"done": "[CODESTORY_DONE]".to_owned(),
                "session_id": session_id.to_string(),
            }))
            .expect("failed to send done object"))
    });

    let stream = init_stream.chain(answer_stream).chain(done_stream);

    Ok(Sse::new(Box::pin(stream)))
}

pub async fn agent_session_edit_anchored(
    Extension(app): Extension<Application>,
    Json(AgentSessionChatRequest {
        session_id,
        exchange_id,
        editor_url,
        query,
        user_context,
        // agent_mode,
        repo_ref,
        project_labels,
        root_directory: _root_directory,
        codebase_search: _codebase_search,
        access_token,
        model_configuration,
        open_files: _open_files,
        all_files: _all_files,
        shell: _shell,
    }): Json<AgentSessionChatRequest>,
) -> Result<impl IntoResponse> {
    let llm_provider = model_configuration
        .llm_properties_for_slow_model()
        .unwrap_or(LLMProperties::new(
            LLMType::ClaudeSonnet,
            LLMProvider::CodeStory(CodeStoryLLMTypes::new()),
            LLMProviderAPIKeys::CodeStory(CodestoryAccessToken::new(access_token.to_owned())),
        ));
    // bring this back later
    let _agent_mode = AideAgentMode::Edit;
    println!("webserver::agent_session::anchored_edit::hit");
    println!(
        "webserver::agent_session::anchored_edit::session_id({})",
        &session_id
    );
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let message_properties = SymbolEventMessageProperties::new(
        SymbolEventRequestId::new(exchange_id.to_owned(), session_id.to_string()),
        sender.clone(),
        editor_url,
        cancellation_token.clone(),
        llm_provider,
    );

    let session_storage_path =
        check_session_storage_path(app.config.clone(), session_id.to_string()).await;

    let scratch_pad_path = check_scratch_pad_path(app.config.clone(), session_id.to_string()).await;
    let scratch_pad_agent = ScratchPadAgent::new(
        scratch_pad_path,
        app.tool_box.clone(),
        app.symbol_manager.hub_sender(),
        None,
    )
    .await;

    let cloned_session_id = session_id.to_string();
    let session_service = app.session_service.clone();
    let _ = tokio::spawn(async move {
        let _ = session_service
            .code_edit_anchored(
                cloned_session_id,
                session_storage_path,
                scratch_pad_agent,
                exchange_id,
                query,
                user_context,
                project_labels,
                repo_ref,
                message_properties,
            )
            .await;
    });

    // TODO(skcd): Over here depending on the exchange reply mode we want to send over the
    // response using ui_sender with the correct exchange_id and the thread_id
    // do we go for a global ui_sender which is being sent to a sink which sends over the data
    // to the editor via http or streaming or whatever (keep an active conneciton always?)
    // how do we notify when the streaming is really completed

    let ui_event_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);
    let cloned_session_id = session_id.to_string();
    let init_stream = futures::stream::once(async move {
        Ok(sse::Event::default()
            .json_data(json!({
                "session_id": cloned_session_id,
                "started": true,
            }))
            // This should never happen, so we force an unwrap.
            .expect("failed to serialize initialization object"))
    });

    // We know the stream is unwind safe as it doesn't use synchronization primitives like locks.
    let answer_stream = ui_event_stream.map(|ui_event: UIEventWithID| {
        sse::Event::default()
            .json_data(ui_event)
            .map_err(anyhow::Error::new)
    });

    // TODO(skcd): Re-introduce this again when we have a better way to manage
    // server side events on the client side

    // this will never get sent cause the sender is never dropped in a way, it will be
    // dropped once we have completed the tokio::spawn above
    let done_stream = futures::stream::once(async move {
        Ok(sse::Event::default()
            .json_data(json!(
                {"done": "[CODESTORY_DONE]".to_owned(),
                "session_id": session_id.to_string(),
            }))
            .expect("failed to send done object"))
    });

    let stream = init_stream.chain(answer_stream).chain(done_stream);

    Ok(Sse::new(Box::pin(stream)))
}

/// This takes care of the agentic editing and we use the scratchpad agent over here
/// for editing
pub async fn agent_session_edit_agentic(
    Extension(app): Extension<Application>,
    Json(AgentSessionChatRequest {
        session_id,
        exchange_id,
        editor_url,
        query,
        user_context,
        // agent_mode,
        repo_ref,
        project_labels,
        root_directory,
        codebase_search,
        access_token,
        model_configuration,
        all_files: _all_files,
        open_files: _open_files,
        shell: _shell,
    }): Json<AgentSessionChatRequest>,
) -> Result<impl IntoResponse> {
    let llm_provider = model_configuration
        .llm_properties_for_slow_model()
        .unwrap_or(LLMProperties::new(
            LLMType::ClaudeSonnet,
            LLMProvider::CodeStory(CodeStoryLLMTypes::new()),
            LLMProviderAPIKeys::CodeStory(CodestoryAccessToken::new(access_token.to_owned())),
        ));
    // bring this back later
    let _agent_mode = AideAgentMode::Edit;
    println!("webserver::agent_session::agentic_edit::hit");
    println!(
        "webserver::agent_session::agentic_edit::session_id({})",
        &session_id
    );
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let message_properties = SymbolEventMessageProperties::new(
        SymbolEventRequestId::new(exchange_id.to_owned(), session_id.to_string()),
        sender.clone(),
        editor_url,
        cancellation_token.clone(),
        llm_provider,
    );

    let session_storage_path =
        check_session_storage_path(app.config.clone(), session_id.to_string()).await;

    let scratch_pad_path = check_scratch_pad_path(app.config.clone(), session_id.to_string()).await;
    let scratch_pad_agent = ScratchPadAgent::new(
        scratch_pad_path,
        app.tool_box.clone(),
        app.symbol_manager.hub_sender(),
        None,
    )
    .await;

    let cloned_session_id = session_id.to_string();
    let session_service = app.session_service.clone();
    let _ = tokio::spawn(async move {
        let _ = session_service
            .code_edit_agentic(
                cloned_session_id,
                session_storage_path,
                scratch_pad_agent,
                exchange_id,
                query,
                user_context,
                project_labels,
                repo_ref,
                root_directory,
                codebase_search,
                message_properties,
            )
            .await;
        println!("tokio::spawn::code_edit_agentic::finished");
    });

    // TODO(skcd): Over here depending on the exchange reply mode we want to send over the
    // response using ui_sender with the correct exchange_id and the thread_id
    // do we go for a global ui_sender which is being sent to a sink which sends over the data
    // to the editor via http or streaming or whatever (keep an active conneciton always?)
    // how do we notify when the streaming is really completed

    let ui_event_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);
    let cloned_session_id = session_id.to_string();
    let init_stream = futures::stream::once(async move {
        Ok(sse::Event::default()
            .json_data(json!({
                "session_id": cloned_session_id,
                "started": true,
            }))
            // This should never happen, so we force an unwrap.
            .expect("failed to serialize initialization object"))
    });

    // We know the stream is unwind safe as it doesn't use synchronization primitives like locks.
    let answer_stream = ui_event_stream.map(|ui_event: UIEventWithID| {
        sse::Event::default()
            .json_data(ui_event)
            .map_err(anyhow::Error::new)
    });

    // TODO(skcd): Re-introduce this again when we have a better way to manage
    // server side events on the client side

    // this will never get sent cause the sender is never dropped in a way, it will be
    // dropped once we have completed the tokio::spawn above
    let done_stream = futures::stream::once(async move {
        Ok(sse::Event::default()
            .json_data(json!(
                {"done": "[CODESTORY_DONE]".to_owned(),
                "session_id": session_id.to_string(),
            }))
            .expect("failed to send done object"))
    });

    let stream = init_stream.chain(answer_stream).chain(done_stream);

    Ok(Sse::new(Box::pin(stream)))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgenticVerifyModelConfig {
    model_configuration: LLMClientConfig,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgenticVerifyModelConfigResponse {
    valid: bool,
    error: Option<String>,
}

impl ApiResponse for AgenticVerifyModelConfigResponse {}

pub async fn verify_model_config(
    Extension(app): Extension<Application>,
    Json(AgenticVerifyModelConfig {
        model_configuration,
    }): Json<AgenticVerifyModelConfig>,
) -> Result<impl IntoResponse> {
    let llm_provider = model_configuration.llm_properties_for_slow_model();

    match llm_provider {
        Some(llm_provider) => {
            // send a dummy request over here to the llm providers checking the validity
            let llm_broker = app.llm_broker.clone();
            let api_key = llm_provider.api_key().clone();
            let provider = llm_provider.provider().clone();
            let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
            let response = llm_broker.stream_completion(api_key, LLMClientCompletionRequest::new(
                llm_provider.llm().clone(),
                vec![LLMClientMessage::user("only say hi back and nothing else, this is to check the validity of the api key".to_owned())],
                0.0,
                None,
            ), provider, vec![("event_type".to_owned(), "validation".to_owned())].into_iter().collect(), sender).await;

            match response {
                Ok(response) => {
                    if response.is_empty() {
                        Ok(Json(AgenticVerifyModelConfigResponse {
                            valid: false,
                            error:
                                Some("No response from provider, please check your api-keys and settings"
                                    .to_owned()),
                        }))
                    } else {
                        Ok(Json(AgenticVerifyModelConfigResponse {
                            valid: true,
                            error: None,
                        }))
                    }
                }
                Err(e) => Ok(Json(AgenticVerifyModelConfigResponse {
                    valid: false,
                    error: Some(e.to_string()),
                })),
            }
        }
        None => {
            return Ok(Json(AgenticVerifyModelConfigResponse {
                valid: false,
                error: None,
            }))
        }
    }
}

pub async fn agent_tool_use(
    Extension(app): Extension<Application>,
    Json(AgentSessionChatRequest {
        session_id,
        exchange_id,
        editor_url,
        query,
        user_context,
        // agent_mode,
        repo_ref,
        project_labels,
        root_directory,
        codebase_search: _codebase_search,
        access_token,
        model_configuration,
        all_files,
        open_files,
        shell,
    }): Json<AgentSessionChatRequest>,
) -> Result<impl IntoResponse> {
    let llm_provider = model_configuration
        .llm_properties_for_slow_model()
        .unwrap_or(LLMProperties::new(
            LLMType::ClaudeSonnet,
            LLMProvider::CodeStory(CodeStoryLLMTypes::new()),
            LLMProviderAPIKeys::CodeStory(CodestoryAccessToken::new(access_token.to_owned())),
        ));
    println!("llm_provider::{:?}", &llm_provider);
    println!("webserver::agent_session::tool_use::hit");
    println!(
        "webserver::agent_session::tool_use::session_id({})",
        &session_id
    );
    println!("user_context::({:?})", &user_context);
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    // check if model and provider combo supports tool use and midwit agent use
    let is_midwit_tool_agent = llm_provider.supports_midwit_and_tool_use();
    let message_properties = SymbolEventMessageProperties::new(
        SymbolEventRequestId::new(exchange_id.to_owned(), session_id.to_string()),
        sender.clone(),
        editor_url,
        cancellation_token.clone(),
        llm_provider,
    );

    let tool_box = app.tool_box.clone();
    let llm_broker = app.llm_broker.clone();

    let session_storage_path =
        check_session_storage_path(app.config.clone(), session_id.to_string()).await;

    let cloned_session_id = session_id.to_string();
    let session_service = app.session_service.clone();
    let _ = tokio::spawn(async move {
        let _ = session_service
            .tool_use_agentic(
                cloned_session_id,
                session_storage_path,
                query,
                exchange_id,
                all_files,
                open_files,
                shell,
                project_labels,
                repo_ref,
                root_directory,
                tool_box,
                llm_broker,
                user_context,
                is_midwit_tool_agent,
                message_properties,
            )
            .await;
        println!("tokio::spawn::tool_use::iteration::finished");
    });

    let ui_event_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);
    let cloned_session_id = session_id.to_string();
    let init_stream = futures::stream::once(async move {
        Ok(sse::Event::default()
            .json_data(json!({
                "session_id": cloned_session_id,
                "started": true,
            }))
            // This should never happen, so we force an unwrap.
            .expect("failed to serialize initialization object"))
    });

    // We know the stream is unwind safe as it doesn't use synchronization primitives like locks.
    let answer_stream = ui_event_stream.map(|ui_event: UIEventWithID| {
        sse::Event::default()
            .json_data(ui_event)
            .map_err(anyhow::Error::new)
    });

    // TODO(skcd): Re-introduce this again when we have a better way to manage
    // server side events on the client side

    // this will never get sent cause the sender is never dropped in a way, it will be
    // dropped once we have completed the tokio::spawn above
    let done_stream = futures::stream::once(async move {
        Ok(sse::Event::default()
            .json_data(json!(
                {"done": "[CODESTORY_DONE]".to_owned(),
                "session_id": session_id.to_string(),
            }))
            .expect("failed to send done object"))
    });

    let stream = init_stream.chain(answer_stream).chain(done_stream);

    Ok(Sse::new(Box::pin(stream)))
}

pub async fn agent_session_plan_iterate(
    Extension(app): Extension<Application>,
    Json(AgentSessionChatRequest {
        session_id,
        exchange_id,
        editor_url,
        query,
        user_context,
        // agent_mode,
        repo_ref,
        project_labels,
        root_directory,
        codebase_search,
        access_token,
        model_configuration,
        all_files: _all_files,
        open_files: _open_files,
        shell: _shell,
    }): Json<AgentSessionChatRequest>,
) -> Result<impl IntoResponse> {
    let llm_provider = model_configuration
        .llm_properties_for_slow_model()
        .unwrap_or(LLMProperties::new(
            LLMType::ClaudeSonnet,
            LLMProvider::CodeStory(CodeStoryLLMTypes::new()),
            LLMProviderAPIKeys::CodeStory(CodestoryAccessToken::new(access_token.to_owned())),
        ));
    // bring this back later
    let _agent_mode = AideAgentMode::Edit;
    println!("webserver::agent_session::plan::iteration::hit");
    println!(
        "webserver::agent_session::plan::session_id({})",
        &session_id
    );
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let message_properties = SymbolEventMessageProperties::new(
        SymbolEventRequestId::new(exchange_id.to_owned(), session_id.to_string()),
        sender.clone(),
        editor_url,
        cancellation_token.clone(),
        llm_provider,
    );

    let session_storage_path =
        check_session_storage_path(app.config.clone(), session_id.to_string()).await;

    let plan_storage_directory = plan_storage_directory(app.config.clone()).await;

    let plan_service = PlanService::new(
        app.tool_box.clone(),
        app.symbol_manager.clone(),
        plan_storage_directory,
    );

    // plan-id is made up of session_id and the exchange-id joined together
    let plan_id = plan_service.generate_unique_plan_id(&session_id, &exchange_id);
    let plan_storage_path = check_plan_storage_path(app.config.clone(), plan_id.to_owned()).await;

    let cloned_session_id = session_id.to_string();
    let session_service = app.session_service.clone();
    let _ = tokio::spawn(async move {
        let _ = session_service
            .plan_iteration(
                cloned_session_id,
                session_storage_path,
                plan_storage_path,
                plan_id,
                plan_service,
                exchange_id,
                query,
                user_context,
                project_labels,
                repo_ref,
                root_directory,
                codebase_search,
                message_properties,
            )
            .await;
        println!("tokio::spawn::plan::iteration::finished");
    });

    let ui_event_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);
    let cloned_session_id = session_id.to_string();
    let init_stream = futures::stream::once(async move {
        Ok(sse::Event::default()
            .json_data(json!({
                "session_id": cloned_session_id,
                "started": true,
            }))
            // This should never happen, so we force an unwrap.
            .expect("failed to serialize initialization object"))
    });

    // We know the stream is unwind safe as it doesn't use synchronization primitives like locks.
    let answer_stream = ui_event_stream.map(|ui_event: UIEventWithID| {
        sse::Event::default()
            .json_data(ui_event)
            .map_err(anyhow::Error::new)
    });

    // TODO(skcd): Re-introduce this again when we have a better way to manage
    // server side events on the client side

    // this will never get sent cause the sender is never dropped in a way, it will be
    // dropped once we have completed the tokio::spawn above
    let done_stream = futures::stream::once(async move {
        Ok(sse::Event::default()
            .json_data(json!(
                {"done": "[CODESTORY_DONE]".to_owned(),
                "session_id": session_id.to_string(),
            }))
            .expect("failed to send done object"))
    });

    let stream = init_stream.chain(answer_stream).chain(done_stream);

    Ok(Sse::new(Box::pin(stream)))
}

/// Generates the plan over here
pub async fn agent_session_plan(
    Extension(app): Extension<Application>,
    Json(AgentSessionChatRequest {
        session_id,
        exchange_id,
        editor_url,
        query,
        user_context,
        // agent_mode,
        repo_ref,
        project_labels,
        root_directory,
        codebase_search,
        access_token,
        model_configuration,
        all_files: _all_files,
        open_files: _open_files,
        shell: _shell,
    }): Json<AgentSessionChatRequest>,
) -> Result<impl IntoResponse> {
    let llm_provider = model_configuration
        .llm_properties_for_slow_model()
        .unwrap_or(LLMProperties::new(
            LLMType::ClaudeSonnet,
            LLMProvider::CodeStory(CodeStoryLLMTypes::new()),
            LLMProviderAPIKeys::CodeStory(CodestoryAccessToken::new(access_token.to_owned())),
        ));
    // bring this back later
    let _agent_mode = AideAgentMode::Edit;
    println!("webserver::agent_session::plan::hit");
    println!(
        "webserver::agent_session::plan::session_id({})",
        &session_id
    );
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let message_properties = SymbolEventMessageProperties::new(
        SymbolEventRequestId::new(exchange_id.to_owned(), session_id.to_string()),
        sender.clone(),
        editor_url,
        cancellation_token.clone(),
        llm_provider,
    );

    let session_storage_path =
        check_session_storage_path(app.config.clone(), session_id.to_string()).await;

    let plan_storage_directory = plan_storage_directory(app.config.clone()).await;

    let plan_service = PlanService::new(
        app.tool_box.clone(),
        app.symbol_manager.clone(),
        plan_storage_directory,
    );

    // plan-id is made up of session_id and the exchange-id joined together
    let plan_id = plan_service.generate_unique_plan_id(&session_id, &exchange_id);
    let plan_storage_path = check_plan_storage_path(app.config.clone(), plan_id.to_owned()).await;

    let cloned_session_id = session_id.to_string();
    let session_service = app.session_service.clone();
    let _ = tokio::spawn(async move {
        let _ = session_service
            .plan_generation(
                cloned_session_id,
                session_storage_path,
                plan_storage_path,
                plan_id,
                plan_service,
                exchange_id,
                query,
                user_context,
                project_labels,
                repo_ref,
                root_directory,
                codebase_search,
                message_properties,
            )
            .await;
        println!("tokio::spawn::plan::finished");
    });

    // TODO(skcd): Over here depending on the exchange reply mode we want to send over the
    // response using ui_sender with the correct exchange_id and the thread_id
    // do we go for a global ui_sender which is being sent to a sink which sends over the data
    // to the editor via http or streaming or whatever (keep an active conneciton always?)
    // how do we notify when the streaming is really completed

    let ui_event_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);
    let cloned_session_id = session_id.to_string();
    let init_stream = futures::stream::once(async move {
        Ok(sse::Event::default()
            .json_data(json!({
                "session_id": cloned_session_id,
                "started": true,
            }))
            // This should never happen, so we force an unwrap.
            .expect("failed to serialize initialization object"))
    });

    // We know the stream is unwind safe as it doesn't use synchronization primitives like locks.
    let answer_stream = ui_event_stream.map(|ui_event: UIEventWithID| {
        sse::Event::default()
            .json_data(ui_event)
            .map_err(anyhow::Error::new)
    });

    // TODO(skcd): Re-introduce this again when we have a better way to manage
    // server side events on the client side

    // this will never get sent cause the sender is never dropped in a way, it will be
    // dropped once we have completed the tokio::spawn above
    let done_stream = futures::stream::once(async move {
        Ok(sse::Event::default()
            .json_data(json!(
                {"done": "[CODESTORY_DONE]".to_owned(),
                "session_id": session_id.to_string(),
            }))
            .expect("failed to send done object"))
    });

    let stream = init_stream.chain(answer_stream).chain(done_stream);

    Ok(Sse::new(Box::pin(stream)))
}
