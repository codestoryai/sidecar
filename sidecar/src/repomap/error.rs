use thiserror::Error;
use tokio::task::JoinError;

use super::file::errors::FileError;

#[derive(Debug, Error)]
pub enum RepoMapError {
    #[error("I/O error")]
    IoError,

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Symbol analysis error: {0}")]
    SymbolAnalysisError(String),

    #[error("Graph analysis error: {0}")]
    GraphAnalysisError(String),

    #[error("Tree generation error: {0}")]
    TreeGenerationError(String),

    #[error("Task failed: {0}")]
    TaskFailed(JoinError),

    #[error("From FS helper error: {0}")]
    FileSystemError(#[from] FileError),
}
