//! Contains the messages or the request which the agent is sending

use crate::agentic::symbol::anchored::AnchoredSymbol;

#[derive(Debug)]
pub enum AgentMessage {
    ReferenceCheck(AgentIntentMessage),
}

impl AgentMessage {
    pub fn user_intent_for_references(
        user_intent: String,
        anchor_symbols: Vec<AnchoredSymbol>,
    ) -> Self {
        AgentMessage::ReferenceCheck(AgentIntentMessage {
            user_intent,
            anchor_symbols,
        })
    }
}

#[derive(Debug)]
pub struct AgentIntentMessage {
    user_intent: String,
    anchor_symbols: Vec<AnchoredSymbol>,
}

impl AgentIntentMessage {
    pub fn get_user_intent(&self) -> &str {
        &self.user_intent
    }

    pub fn anchor_symbols(self) -> Vec<AnchoredSymbol> {
        self.anchor_symbols
    }
}
