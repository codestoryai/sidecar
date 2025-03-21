use crate::agentic::tool::errors::ToolError;

#[derive(Debug, thiserror::Error)]
pub enum FeedbackError {
    #[error("Empty trajectory")]
    EmptyTrajectory,

    #[error("Root not found")]
    RootNotFound,

    #[error("Problemstatement not found")]
    ProblemStatementNotFound,

    #[error("Tool error: {0}")]
    ToolError(#[from] ToolError),
}
