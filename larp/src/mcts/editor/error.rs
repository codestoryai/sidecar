use crate::agentic::tool::errors::ToolError;

#[derive(Debug, thiserror::Error)]
pub enum AnthropicEditorError {
    #[error("Tool Error: {0}")]
    ToolError(#[from] ToolError),

    #[error("Missing input parameters {0}")]
    InputParametersMissing(String),

    #[error("Error reading file {0}")]
    ReadingFileError(String),

    #[error("View command error: {0}")]
    ViewCommandError(String),
}
