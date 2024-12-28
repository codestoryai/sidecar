use serde::{Deserialize, Serialize};
use async_trait::async_trait;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileOperation {
    Open,
    View,
    Edit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicComputerRequest {
    pub operation: FileOperation,
    pub fs_file_path: String,
    pub editor_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<String>,
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicComputerResponse {
    pub content: String,
    pub language: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamUpdate {
    pub content: String,
    pub language: Option<String>,
    pub error: Option<String>,
}

#[async_trait]
pub trait StreamProcessor {
    async fn send_update(&self, update: StreamUpdate) -> Result<(), String>;
    async fn finalize(&self, final_response: AnthropicComputerResponse) -> Result<String, String>;
}

impl Default for AnthropicComputerResponse {
    fn default() -> Self {
        Self {
            content: String::new(),
            language: None,
            error: None,
        }
    }
}
