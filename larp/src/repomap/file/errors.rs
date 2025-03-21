use std::io;

use gix::{head::peel::to_commit, reference};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FileError {
    #[error("Git reading error")]
    GixErr(#[from] gix::open::Error),

    #[error("Commit reading error")]
    CommitError(#[from] to_commit::Error),

    #[error("Reference find error")]
    ReferenceFindError(#[from] reference::find::existing::Error),

    #[error("IO Error")]
    IOError(#[from] io::Error),

    #[error("Gix Error")]
    GixError,
}
