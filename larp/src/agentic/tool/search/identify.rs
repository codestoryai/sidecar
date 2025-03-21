use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename = "response", default)]
pub struct IdentifyResponse {
    #[serde(rename = "item", default)]
    pub items: Vec<IdentifiedFile>,
    pub scratch_pad: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentifiedFile {
    path: PathBuf,
    thinking: String,
}

impl IdentifiedFile {
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn thinking(&self) -> &str {
        &self.thinking
    }
}
