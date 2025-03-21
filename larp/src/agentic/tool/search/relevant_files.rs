use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename = "response", default)]
pub struct QueryRelevantFilesResponse {
    #[serde(default)]
    pub files: QueryRelevantFiles,
    pub scratch_pad: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct QueryRelevantFiles {
    #[serde(default)]
    file: Vec<QueryRelevantFile>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct QueryRelevantFile {
    path: PathBuf,
    thinking: String,
}
