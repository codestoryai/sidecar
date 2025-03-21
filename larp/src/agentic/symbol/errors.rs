use llm_client::clients::types::LLMClientError;
use serde_json;
use thiserror::Error;
use tokio::sync::{mpsc::error::SendError, oneshot::error::RecvError};

use crate::{
    agentic::tool::{errors::ToolError, lsp::diagnostics::DiagnosticSnippetError},
    user_context::types::UserContextError,
};

use super::events::message_event::SymbolEventMessage;

#[derive(Debug, Error)]
pub enum SymbolError {
    #[error("Tool error: {0}")]
    ToolError(ToolError),

    #[error("Wrong tool output")]
    WrongToolOutput,

    #[error("Expected file to exist")]
    ExpectedFileToExist,

    #[error("Symbol not found")]
    SymbolNotFound,

    #[error("Unable to read file contents")]
    UnableToReadFileContent,

    #[error("channel recieve error: {0}")]
    RecvError(RecvError),

    #[error("No definition found: {0}")]
    DefinitionNotFound(String),

    #[error("Symbol not contained in a child")]
    SymbolNotContainedInChild,

    #[error("No containing symbol found")]
    NoContainingSymbolFound,

    #[error("No outline node satisfy position")]
    NoOutlineNodeSatisfyPosition,

    #[error("No outline node with name found: {0}")]
    OutlineNodeNotFound(String),

    #[error("Snippet not found")]
    SnippetNotFound,

    #[error("Symbol: {0} not found in the line content: {1}")]
    SymbolNotFoundInLine(String, String),

    #[error("Outline node editing not supported")]
    OutlineNodeEditingNotSupported,

    #[error("Cached query failed")]
    CachedQueryFailed,

    #[error("User context empty")]
    UserContextEmpty,

    #[error("File type not supported: {0}")]
    FileTypeNotSupported(String),

    #[error("Full symbol edit failure: {0}")]
    SymbolError(String),

    #[error("Edit not required: {0}")]
    EditNotRequired(String),

    #[error("Symbol event send error: {0}")]
    SymbolEventSendError(SendError<SymbolEventMessage>),

    #[error("Diagnostic snippet error: {0}")]
    DiagnosticSnippetError(DiagnosticSnippetError),

    #[error("IO error: {0}")]
    IOError(std::io::Error),

    #[error("User context error: {0}")]
    UserContextError(UserContextError),

    #[error("LLM Client error: {0}")]
    LLMClientError(LLMClientError),

    #[error("Cancelled Response")]
    CancelledResponseStream,

    #[error("Failed to get tool")]
    FailedToGetTool,

    #[error("Test case is passing")]
    TestCaseIsPassing,

    #[error("Invalid JSON: {0}")]
    InvalidJson(serde_json::Error),
}
