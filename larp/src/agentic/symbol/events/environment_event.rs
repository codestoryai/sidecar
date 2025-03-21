//! Contains the environment event which might be sent externally
//! to either inform of something happening or for a request-id

use crate::agentic::symbol::{
    anchored::AnchoredSymbol,
    types::{SymbolEventRequest, SymbolEventResponse},
};

use super::{
    agent::AgentMessage,
    human::{HumanAnchorRequest, HumanMessage},
    lsp::LSPSignal,
    message_event::SymbolEventMessageProperties,
};

pub struct EditorStateChangeRequest {
    edits_made: Vec<SymbolEventResponse>,
    user_query: String,
}

impl EditorStateChangeRequest {
    pub fn new(edits_made: Vec<SymbolEventResponse>, user_query: String) -> Self {
        Self {
            edits_made,
            user_query,
        }
    }
    pub fn user_query(&self) -> &str {
        &self.user_query
    }

    pub fn consume_edits_made(self) -> Vec<SymbolEventResponse> {
        self.edits_made
    }
}

pub enum EnvironmentEventType {
    Symbol(SymbolEventRequest),
    EditorStateChange(EditorStateChangeRequest),
    LSP(LSPSignal),
    Human(HumanMessage),
    Agent(AgentMessage),
    ShutDown,
}

impl EnvironmentEventType {
    pub fn is_shutdown(&self) -> bool {
        matches!(self, Self::ShutDown)
    }

    pub fn human_anchor_request(
        query: String,
        anchored_symbols: Vec<AnchoredSymbol>,
        context: Option<String>,
    ) -> Self {
        EnvironmentEventType::Human(HumanMessage::Anchor(HumanAnchorRequest::new(
            query,
            anchored_symbols,
            context,
        )))
    }
}

pub struct EnvironmentEvent {
    message_properties: SymbolEventMessageProperties,
    event: EnvironmentEventType,
}

impl EnvironmentEvent {
    pub fn event(
        event: EnvironmentEventType,
        message_properties: SymbolEventMessageProperties,
    ) -> Self {
        Self {
            event,
            message_properties,
        }
    }

    pub fn message_properties(&self) -> SymbolEventMessageProperties {
        self.message_properties.clone()
    }

    pub fn event_type(self) -> EnvironmentEventType {
        self.event
    }

    pub fn is_lsp_event(&self) -> bool {
        matches!(self.event, EnvironmentEventType::LSP(_))
    }
}
