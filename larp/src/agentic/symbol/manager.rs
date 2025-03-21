//! Contains the central manager for the symbols and maintains them in the system
//! as a connected graph in some ways in which these symbols are able to communicate
//! with each other

use std::sync::Arc;

use futures::{stream, StreamExt};
use llm_client::clients::types::LLMType;
use llm_client::provider::{GoogleAIStudioKey, LLMProvider};
use tokio::sync::mpsc::UnboundedSender;

use crate::agentic::swe_bench::search_cache::LongContextSearchCache;
use crate::agentic::symbol::events::input::SymbolEventRequestId;
use crate::agentic::symbol::events::probe::SymbolToProbeRequest;
use crate::agentic::symbol::helpers::SymbolFollowupBFS;
use crate::agentic::symbol::tool_properties::ToolProperties;
use crate::agentic::tool::code_symbol::important::CodeSymbolImportantWideSearch;
use crate::agentic::tool::input::ToolInput;
use crate::agentic::tool::r#type::Tool;
use crate::chunking::editor_parsing::EditorParsing;
use crate::chunking::languages::TSLanguageParsing;
use crate::user_context::types::UserContext;
use crate::{
    agentic::tool::{broker::ToolBroker, output::ToolOutput},
    inline_completion::symbols_tracker::SymbolTrackerInline,
};

use super::events::message_event::{SymbolEventMessage, SymbolEventMessageProperties};
use super::identifier::LLMProperties;
use super::tool_box::ToolBox;
use super::ui_event::UIEventWithID;
use super::{
    errors::SymbolError,
    locker::SymbolLocker,
    types::{SymbolEventRequest, SymbolEventResponse},
};

// This is the main communication manager between all the symbols
// this of this as the central hub through which all the events go forward
/// The SymbolManager is the central hub for managing and coordinating symbol-related operations.
/// It handles communication between symbols, manages their lifecycle, and orchestrates various tools and services.
pub struct SymbolManager {
    /// Channel sender for communication between symbols and the manager.
    /// This allows for asynchronous message passing within the system.
    sender: UnboundedSender<SymbolEventMessage>,

    /// Manages locking and unlocking of symbols to prevent concurrent access.
    /// This ensures thread-safety when multiple operations are performed on symbols simultaneously.
    symbol_locker: SymbolLocker,

    /// Broker for managing and invoking various tools.
    /// This provides a centralized way to access and use different tools required for symbol operations.
    tools: Arc<ToolBroker>,

    /// Parser for TypeScript language constructs.
    /// This is used to analyze and understand TypeScript code structures.
    ts_parsing: Arc<TSLanguageParsing>,

    /// Collection of tools and utilities for symbol operations.
    /// This provides a set of helper functions and utilities specific to symbol manipulation and analysis.
    tool_box: Arc<ToolBox>,

    /// Properties for the Language Model being used.
    /// This contains configuration and settings for the LLM used in various operations.
    _llm_properties: LLMProperties,

    /// Cache for storing long-context search results.
    /// This improves performance by storing and reusing results of expensive long-context searches.
    _long_context_cache: LongContextSearchCache,
}

impl SymbolManager {
    pub fn new(
        tools: Arc<ToolBroker>,
        symbol_broker: Arc<SymbolTrackerInline>,
        editor_parsing: Arc<EditorParsing>,
        llm_properties: LLMProperties,
    ) -> Self {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<SymbolEventMessage>();
        let tool_box = Arc::new(ToolBox::new(
            tools.clone(),
            symbol_broker.clone(),
            editor_parsing.clone(),
        ));
        let symbol_locker =
            SymbolLocker::new(sender.clone(), tool_box.clone(), llm_properties.clone());
        let cloned_symbol_locker = symbol_locker.clone();
        tokio::spawn(async move {
            // TODO(skcd): Make this run in full parallelism in the future, for
            // now this is fine
            while let Some(event) = receiver.recv().await {
                println!("symbol_manager::tokio::spawn::receiver_event");
                // let _ = cloned_ui_sender.send(UIEvent::from(event.0.clone()));
                let _ = cloned_symbol_locker.process_request(event).await;
            }
            println!("symbol_manager::tokio::spawn::end");
        });
        let ts_parsing = Arc::new(TSLanguageParsing::init());
        Self {
            sender,
            symbol_locker,
            ts_parsing,
            tools,
            tool_box,
            _llm_properties: llm_properties,
            _long_context_cache: LongContextSearchCache::new(),
        }
    }

    pub fn hub_sender(&self) -> UnboundedSender<SymbolEventMessage> {
        self.sender.clone()
    }

    // TODO(codestory): This is hardcoded function, we of course want to follow
    // something similar but make it more generic later on
    pub async fn impls_test(
        &self,
        root_dir: &str,
        fs_file_path: &str,
        // this contains all the request id related jazz over here
        message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        let symbol_change_set = self
            .tool_box
            .grab_changed_symbols_in_file_git(&root_dir, &fs_file_path)
            .await?;
        println!("symbol_change_set:{:?}", &symbol_change_set);

        let symbols_to_followup_request = symbol_change_set
            .remove_changes()
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

        // This needs to be an iterative loop over here instead of a single call
        // we have to perform a BFS at this layer so the symbols can organise and
        // get the job done
        let _ = self
            .tool_box
            .check_for_followups_bfs(
                symbols_to_followup_request,
                self.symbol_locker.hub_sender.clone(),
                message_properties,
                &ToolProperties::new().set_apply_edits_directly(),
            )
            .await;

        Ok(())
    }

    pub fn tool_box(&self) -> &ToolBox {
        &self.tool_box
    }

    pub async fn probe_request_from_user_context(
        &self,
        query: String,
        user_context: UserContext,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        println!("symbol_manager::probe_request_from_user_context::start");
        let request_id = uuid::Uuid::new_v4().to_string();
        let request_id = SymbolEventRequestId::new(request_id.to_owned(), request_id.to_owned());
        let request_id_ref = &request_id;
        let variables = self
            .tool_box
            .grab_symbols_from_user_context(
                query.to_owned(),
                user_context.clone(),
                message_properties.clone(),
            )
            .await;
        let outline = self
            .tool_box
            .outline_for_user_context(&user_context, message_properties.clone())
            .await;
        let code_wide_search =
            ToolInput::RequestImportantSymbolsCodeWide(CodeSymbolImportantWideSearch::new(
                user_context.clone(),
                query.to_owned(),
                // Hardcoding here, but we can remove this later
                LLMType::GeminiPro,
                LLMProvider::GoogleAIStudio,
                llm_client::provider::LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new(
                    "".to_owned(),
                )),
                request_id_ref.root_request_id().to_owned(),
                outline,
                "".to_owned(),
                "".to_owned(),
                message_properties.to_owned(),
            ));
        let output = {
            match variables {
                Ok(variables) => ToolOutput::ImportantSymbols(variables),
                _ => self
                    .tools
                    .invoke(code_wide_search)
                    .await
                    .map_err(|e| SymbolError::ToolError(e))?,
            }
        };
        println!(
            "symbol_manager::probe_request_from_user_context::output({:?})",
            &output
        );
        if let ToolOutput::ImportantSymbols(important_symbols)
        | ToolOutput::RepoMapSearch(important_symbols) = output
        {
            // We have the important symbols here which we can then use to invoke the individual process request
            let important_symbols = important_symbols.fix_symbol_names(self.ts_parsing.clone());

            let mut symbols = self
                .tool_box
                .important_symbols(&important_symbols, message_properties.clone())
                .await
                .map_err(|e| e.into())?;
            // TODO(skcd): Another check over here is that we can search for the exact variable
            // and then ask for the edit
            println!(
                "symbol_manager::probe_request_from_user_context::[{}]",
                symbols
                    .iter()
                    .map(|(symbol, _)| symbol.symbol_name().to_owned())
                    .collect::<Vec<_>>()
                    .join(",")
            );
            // TODO(skcd): the symbol here might belong to a class or it might be a global function
            // we want to grab the largest node containing the symbol here instead of using
            // the symbol directly since our algorithm would not work otherwise
            // we would also need to de-duplicate the symbols which we have to process right over here
            // otherwise it might lead to errors
            if symbols.is_empty() {
                println!("symbol_manager::grab_symbols_using_search");
                symbols = self
                    .tool_box
                    .grab_symbol_using_search(important_symbols, message_properties.clone())
                    .await
                    .map_err(|e| e.into())?;
            }

            // Create all the symbol agents
            let symbol_identifiers = stream::iter(
                symbols
                    .into_iter()
                    .map(|symbol| (symbol, message_properties.clone())),
            )
            .map(|((symbol_request, _), message_properties)| async move {
                let symbol_identifier = self
                    .symbol_locker
                    .create_symbol_agent(symbol_request, ToolProperties::new(), message_properties)
                    .await;
                symbol_identifier
            })
            .buffer_unordered(100)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .filter_map(|s| s.ok())
            .collect::<Vec<_>>();

            println!(
                "symbol_manager::probe_request_from_user_context::len({})",
                symbol_identifiers.len(),
            );

            let query_ref = &query;

            // Now for all the symbol identifiers which we are getting we have to
            // send a request to all of them with the probe query
            let responses = stream::iter(symbol_identifiers.into_iter().map(|symbol_identifier| {
                (
                    symbol_identifier.clone(),
                    SymbolEventRequest::probe_request(
                        symbol_identifier.clone(),
                        SymbolToProbeRequest::new(
                            symbol_identifier,
                            query_ref.to_owned(),
                            query_ref.to_owned(),
                            request_id_ref.root_request_id().to_owned(),
                            vec![],
                        ),
                        ToolProperties::new(),
                    ),
                    message_properties.clone(),
                )
            }))
            .map(
                |(symbol_identifier, symbol_event_request, message_properties)| async move {
                    let (sender, receiver) = tokio::sync::oneshot::channel();
                    dbg!(
                        "sending initial request to symbol: {:?}",
                        &symbol_identifier
                    );
                    let request_event = SymbolEventMessage::message_with_properties(
                        symbol_event_request,
                        message_properties.clone(),
                        sender,
                    );
                    self.symbol_locker.process_request(request_event).await;
                    let response = receiver.await;
                    dbg!(
                        "For symbol identifier: {:?} the response is {:?}",
                        &symbol_identifier,
                        &response
                    );
                    (response, symbol_identifier)
                },
            )
            .buffer_unordered(100)
            .collect::<Vec<_>>()
            .await;

            // send the response forward after combining all the answers using the LLM
            let final_responses = responses
                .into_iter()
                .filter_map(|(response, symbol_identifier)| match response {
                    Ok(response) => Some((response, symbol_identifier)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let final_answer = final_responses
                .into_iter()
                .map(|(response, symbol_identifier)| {
                    let symbol_name = symbol_identifier.symbol_name();
                    let symbol_file_path = symbol_identifier
                        .fs_file_path()
                        .map(|fs_file_path| format!("at {}", fs_file_path))
                        .unwrap_or("".to_owned());
                    let response = response.to_string();
                    let symbol_readable = format!("{symbol_name} {symbol_file_path}");
                    format!(
                        r#"{symbol_readable}
{response}"#
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            let _ = message_properties
                .ui_sender()
                .send(UIEventWithID::probing_finished_event(
                    request_id_ref.root_request_id().to_owned(),
                    final_answer,
                ));
            println!("things are completed over here after probing");
        }
        println!("things are more complete over here after probing");
        Ok(())
    }

    // This is just for testing out the flow for single input events
    pub async fn probe_request(
        &self,
        symbol_event_message: SymbolEventMessage,
        receiver: tokio::sync::oneshot::Receiver<SymbolEventResponse>,
    ) -> Result<(), SymbolError> {
        let _ = self
            .symbol_locker
            .process_request(symbol_event_message)
            .await;
        let response = receiver.await;
        println!("{:?}", response.expect("to work"));
        Ok(())
    }
}
