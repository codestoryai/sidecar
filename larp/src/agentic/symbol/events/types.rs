//! The different kind of events which the symbols can invoke and needs to work
//! on

use super::{
    edit::SymbolToEditRequest, initial_request::InitialRequestData, probe::SymbolToProbeRequest,
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct AskQuestionRequest {
    question: String,
}

impl AskQuestionRequest {
    pub fn new(question: String) -> Self {
        Self { question }
    }

    pub fn get_question(&self) -> &str {
        &self.question
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum SymbolEvent {
    InitialRequest(InitialRequestData),
    AskQuestion(AskQuestionRequest), // todo(zi) remove this shit everywhere...
    UserFeedback,
    Delete,
    Edit(SymbolToEditRequest),
    Outline,
    // Probe
    Probe(SymbolToProbeRequest),
}
