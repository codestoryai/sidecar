use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CommunicationError {
    #[error("Input Error: {0}")]
    InputError(String),

    #[error("IO Error: {0}")]
    IoError(#[from] io::Error),
}
