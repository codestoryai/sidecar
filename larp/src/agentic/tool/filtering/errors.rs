use llm_client::clients::types::LLMClientError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CodeToEditFilteringError {
    #[error("LLM Client error: {0}")]
    LLMClientError(LLMClientError),

    #[error("serde error: {0}")]
    SerdeError(#[from] serde_xml_rs::Error),

    #[error("invalid response")]
    InvalidResponse,

    #[error("Exhausted retries")]
    RetriesExhausted,
}
