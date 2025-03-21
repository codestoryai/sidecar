use llm_client::clients::types::{LLMClientError, LLMType};
use std::error::Error;
use thiserror::Error;

use crate::user_context::types::UserContextError;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename = "keywords")]
pub struct KeywordsReply {
    #[serde(default)]
    keywords: Vec<String>,
}

impl KeywordsReply {
    pub fn parse_response(response: &str) -> Result<Self, KeywordsReplyError> {
        if response.is_empty() {
            return Err(KeywordsReplyError::EmptyResponse);
        }

        let lines = response
            .lines()
            .skip_while(|line| !line.contains("<keywords>"))
            .skip(1)
            .take_while(|line| !line.contains("</keywords>"))
            .map(|line| line.to_owned())
            .collect::<Vec<String>>();

        Ok(Self { keywords: lines })
    }

    pub fn keywords(&self) -> &Vec<String> {
        &self.keywords
    }
}

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
pub enum KeywordsReplyError {
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
}
