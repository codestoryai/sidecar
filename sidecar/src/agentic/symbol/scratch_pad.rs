//! Contains the scratch pad agent whose job is to work alongside the developer
//! and help them accomplish a task
//! This way the agent can look at all the events and the requests which are happening
//! and take a decision based on them on what should happen next

use std::{collections::HashSet, pin::Pin, sync::Arc};

use futures::{stream, Stream, StreamExt};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    agentic::symbol::ui_event::UIEventWithID,
    chunking::text_document::{Position, Range},
};

use super::{
    errors::SymbolError,
    events::{
        edit::{SymbolToEdit, SymbolToEditRequest},
        environment_event::EnvironmentEventType,
        human::{HumanAnchorRequest, HumanMessage},
        message_event::{SymbolEventMessage, SymbolEventMessageProperties},
        types::SymbolEvent,
    },
    identifier::SymbolIdentifier,
    tool_box::ToolBox,
    tool_properties::ToolProperties,
    types::{SymbolEventRequest, SymbolEventResponse},
};

// We should have a way to update our cache of all that has been done
// and what we are upto right now
// the ideal goal would be to rewrite the scratchpad in a good way so we are
// able to work on top of that
// a single LLM call should rewrite the sections which are present and take as input
// the lsp signal
// we also need to tell this agent what all things are possible, like: getting data from elsewhere
// looking at some other file and keeping that in its cache
// also what kind of information it should keep in:
// it can be state driven based on the user ask
// there will be files which the system has to keep in context, which can be dynamic as well
// we have to control it to not go over the 50kish limit ... cause it can grow by a lot
// but screw it, we keep it as it is
// lets keep it free-flow before we figure out the right way to go about doing this
// mega-scratchpad ftw

/// Different kind of events which can happen
/// We should move beyond symbol events tbh at this point :')

#[derive(Clone)]
pub struct ScratchPadAgent {
    storage_fs_path: String,
    message_properties: SymbolEventMessageProperties,
    tool_box: Arc<ToolBox>,
    symbol_event_sender: UnboundedSender<SymbolEventMessage>,
}

impl ScratchPadAgent {
    pub fn new(
        message_properties: SymbolEventMessageProperties,
        tool_box: Arc<ToolBox>,
        symbol_event_sender: UnboundedSender<SymbolEventMessage>,
    ) -> Self {
        Self {
            storage_fs_path: "/Users/skcd/test_repo/sidecar/scratchpad.md".to_owned(),
            message_properties,
            tool_box,
            symbol_event_sender,
        }
    }
}

impl ScratchPadAgent {
    /// We try to contain all the events which are coming in from the symbol
    /// which is being edited by the user, the real interface here will look like this
    pub async fn process_envrionment(
        self,
        mut stream: Pin<Box<dyn Stream<Item = EnvironmentEventType> + Send + Sync>>,
    ) {
        println!("scratch_pad_agent::start_processing_environment");
        while let Some(event) = stream.next().await {
            match event {
                EnvironmentEventType::LSP(_lsp_signal) => {
                    // process the lsp signal over here
                }
                EnvironmentEventType::Human(message) => {
                    println!("scratch_pad_agent::human_message::({:?})", &message);
                    let _ = self.handle_human_message(message).await;
                    // whenever the human sends a request over here, encode it and try
                    // to understand how to handle it, some might require search, some
                    // might be more automagic
                }
                EnvironmentEventType::Symbol(_symbol_event) => {
                    // we know a symbol is going to be edited, what should we do about it?
                }
                EnvironmentEventType::ShutDown => {
                    println!("scratch_pad_agent::shut_down");
                    break;
                }
            }
        }
    }

    async fn handle_human_message(&self, human_message: HumanMessage) -> Result<(), SymbolError> {
        match human_message {
            HumanMessage::Anchor(anchor_request) => self.human_message_anchor(anchor_request).await,
            HumanMessage::Followup(_followup_request) => Ok(()),
        }
    }

    async fn human_message_anchor(
        &self,
        anchor_request: HumanAnchorRequest,
    ) -> Result<(), SymbolError> {
        let start_instant = std::time::Instant::now();
        println!("scratch_pad_agent::human_message_anchor::start");
        let anchored_symbols = anchor_request.anchored_symbols();
        let symbols_to_edit_request = self
            .tool_box
            .symbol_to_edit_request(
                anchored_symbols,
                anchor_request.user_query(),
                anchor_request.anchor_request_context(),
                self.message_properties.clone(),
            )
            .await?;

        let cloned_self = self.clone();
        let cloned_anchored_request = anchor_request.clone();
        let _ = tokio::spawn(async move {
            cloned_self
                .handle_user_anchor_request(cloned_anchored_request)
                .await;
        });

        let edits_done = stream::iter(symbols_to_edit_request.into_iter().map(|data| {
            (
                data,
                self.message_properties.clone(),
                self.symbol_event_sender.clone(),
            )
        }))
        .map(
            |(symbol_to_edit_request, message_properties, symbol_event_sender)| async move {
                let (sender, receiver) = tokio::sync::oneshot::channel();
                let symbol_event_request = SymbolEventRequest::new(
                    symbol_to_edit_request.symbol_identifier().clone(),
                    SymbolEvent::Edit(symbol_to_edit_request), // defines event type
                    ToolProperties::new(),
                );
                let event = SymbolEventMessage::message_with_properties(
                    symbol_event_request,
                    message_properties,
                    sender,
                );
                let _ = symbol_event_sender.send(event);
                receiver.await
            },
        )
        // run 100 edit requests in parallel to prevent race conditions
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s.ok())
        .collect::<Vec<_>>();

        let cloned_self = self.clone();
        let cloned_user_query = anchor_request.user_query().to_owned();
        let _ = tokio::spawn(async move {
            let _ = cloned_self
                .react_to_edits(edits_done, cloned_user_query)
                .await;
        });
        println!(
            "scratch_pad_agent::human_message_anchor::end::time_taken({}ms)",
            start_instant.elapsed().as_millis()
        );
        // send end of iteration event over here to the frontend
        let _ = self
            .message_properties
            .ui_sender()
            .send(UIEventWithID::code_iteration_finished(
                self.message_properties.request_id_str().to_owned(),
            ));
        Ok(())
    }

    async fn handle_user_anchor_request(&self, anchor_request: HumanAnchorRequest) {
        println!("scratch_pad::handle_user_anchor_request");
        // figure out what to do over here
        let file_paths = anchor_request
            .anchored_symbols()
            .into_iter()
            .filter_map(|anchor_symbol| anchor_symbol.fs_file_path())
            .collect::<Vec<_>>();
        let mut already_seen_files: HashSet<String> = Default::default();
        let mut user_context_files = vec![];
        for fs_file_path in file_paths.into_iter() {
            if already_seen_files.contains(&fs_file_path) {
                continue;
            }
            already_seen_files.insert(fs_file_path.to_owned());
            let file_contents = self
                .tool_box
                .file_open(fs_file_path, self.message_properties.clone())
                .await;
            if let Ok(file_contents) = file_contents {
                user_context_files.push({
                    let file_path = file_contents.fs_file_path();
                    let language = file_contents.language();
                    let content = file_contents.contents_ref();
                    format!(
                        r#"<file>
<fs_file_path>
{file_path}
</fs_file_path>
<content>
```{language}
{content}
```
</content>
</file>"#
                    )
                });
            }
        }
        println!("scratch_pad_agent::tool_box::agent_human_request");
        let _ = self
            .tool_box
            .scratch_pad_agent_human_request(
                self.storage_fs_path.to_owned(),
                anchor_request.user_query().to_owned(),
                user_context_files,
                anchor_request
                    .anchored_symbols()
                    .into_iter()
                    .map(|anchor_symbol| {
                        let content = anchor_symbol.content();
                        let fs_file_path = anchor_symbol.fs_file_path().unwrap_or_default();
                        let line_range_header = format!(
                            "{}-{}:{}",
                            fs_file_path,
                            anchor_symbol.possible_range().start_line(),
                            anchor_symbol.possible_range().end_line()
                        );
                        format!(
                            r#"Location: {line_range_header}
```
{content}
```"#
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                self.message_properties.clone(),
            )
            .await;
    }

    /// We want to react to the various edits which have happened and the request they were linked to
    /// and come up with next steps and try to understand what we can do to help the developer
    async fn react_to_edits(&self, edits: Vec<SymbolEventResponse>, user_query: String) {
        println!("scratch_pad::react_to_edits");
        // figure out what to do over here
    }
}