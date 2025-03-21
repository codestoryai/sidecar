//! Contains the erorrs from running inference
//! The error type over here is a bit more refined to be able
//! to react to the various error types since the feedback is essential

use crate::agentic::{symbol::errors::SymbolError, tool::errors::ToolError};

#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    #[error("Tool Error: {0}")]
    ToolError(#[from] ToolError),

    #[error("Empty trajectory")]
    EmptyTrajectory,

    #[error("Wrong tool type")]
    WrongToolOutput,

    #[error("Symbol error: {0}")]
    SymbolError(#[from] SymbolError),
}
