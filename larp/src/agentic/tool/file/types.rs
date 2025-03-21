use llm_client::clients::types::{LLMClientError, LLMType};
use std::error::Error;
use thiserror::Error;

use crate::user_context::types::UserContextError;

#[derive(Debug)]
pub struct SerdeError {
    xml_error: serde_xml_rs::Error,
    content: String,
}

impl std::fmt::Display for SerdeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Serde error: {}\nContent:{}",
            self.xml_error, self.content
        )
    }
}

impl Error for SerdeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.xml_error)
    }
}

impl SerdeError {
    pub fn new(xml_error: serde_xml_rs::Error, content: String) -> Self {
        Self { xml_error, content }
    }
}

#[derive(Debug, Error)]
pub enum FileImportantError {
    #[error("Wrong LLM for input: {0}")]
    WrongLLM(LLMType),

    #[error("LLM Client erorr: {0}")]
    LLMClientError(#[from] LLMClientError),

    #[error("Serde error: {0}")]
    SerdeError(#[from] SerdeError),

    #[error("Quick xml error: {0}")]
    QuickXMLError(#[from] quick_xml::DeError),

    #[error("User context error: {0}")]
    UserContextError(#[from] UserContextError),

    #[error("Exhausted retries")]
    ExhaustedRetries,

    #[error("Empty response")]
    EmptyResponse,

    #[error("Wrong format: {0}")]
    WrongFormat(String),

    #[error("Tree printing failed for: {0}")]
    PrintTreeError(String),
}
