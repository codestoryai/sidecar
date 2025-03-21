use llm_client::clients::types::{LLMClientError, LLMType};
use thiserror::Error;

use crate::repomap::error::RepoMapError;

use super::{
    code_symbol::types::CodeSymbolError, file::types::FileImportantError,
    filtering::errors::CodeToEditFilteringError, kw_search::types::KeywordsReplyError,
    r#type::ToolType, rerank::base::ReRankError, search::iterative::IterativeSearchError,
};

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("Unable to grab the context")]
    UnableToGrabContext,

    #[error("LLM not supported for tool")]
    LLMNotSupported,

    #[error("Wrong tool input found: {0}")]
    WrongToolInput(ToolType),

    #[error("LLM Client call error: {0}")]
    LLMClientError(#[from] LLMClientError),

    #[error("Missing tool")]
    MissingTool,

    #[error("Error converting serde json to string")]
    SerdeConversionFailed,

    #[error("Communication with editor failed")]
    ErrorCommunicatingWithEditor,

    #[error("Language not supported")]
    NotSupportedLanguage,

    #[error("ReRanking error: {0}")]
    ReRankingError(ReRankError),

    #[error("Code Symbol Error: {0}")]
    CodeSymbolError(CodeSymbolError),

    #[error("Symbol not found: {0}")]
    SymbolNotFound(String),

    #[error("Code to edit filtering error: {0}")]
    CodeToEditFiltering(CodeToEditFilteringError),

    #[error("Code not formatted properly: {0}")]
    CodeNotFormatted(String),

    #[error("Invoking SWE Bench test failed")]
    SWEBenchTestEndpointError,

    #[error("Not supported LLM: {0}")]
    NotSupportedLLM(LLMType),

    #[error("Missing xml tags")]
    MissingXMLTags,

    #[error("Retries exhausted")]
    RetriesExhausted,

    #[error("File important error, {0}")]
    FileImportantError(FileImportantError),

    #[error("Big search error: {0}")]
    BigSearchError(String),

    #[error("Keyword search error: {0}")]
    KeywordSearchError(KeywordsReplyError),

    #[error("IterativeSearch error: {0}")]
    IterativeSearchError(IterativeSearchError),

    #[error("IO Error: {0}")]
    IOError(#[from] std::io::Error),

    #[error("Output stream not present")]
    OutputStreamNotPresent,

    #[error("Repo map error: {0}")]
    RepoMapError(#[from] RepoMapError),

    #[error("Readline error")]
    ReadLineError,

    #[error("Glob error")]
    GlobError(#[from] globset::Error),

    #[error("Cancelled by user")]
    UserCancellation,

    #[error("No operation ID found for this PR. Please include an operation ID in the PR title or description.")]
    MissingOperationID,

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Invocation error: {0}")]
    InvocationError(String),
}