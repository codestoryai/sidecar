use crate::agentic::tool::errors::ToolError;

#[derive(Debug, thiserror::Error)]
pub enum DeciderError {
    #[error("Tool Error: {0}")]
    ToolError(#[from] ToolError),

    #[error("No nodes to check")]
    NoNodesToCheck,

    #[error("No completed node")]
    NoCompletedNode,

    #[error("No root node")]
    NoRootNode,

    #[error("No root message found")]
    NoRootMessageFound,
}
