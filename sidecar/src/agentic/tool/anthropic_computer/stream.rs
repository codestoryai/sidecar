use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::agentic::tool::errors::ToolError;

use super::types::{StreamProcessor, StreamUpdate, AnthropicComputerResponse};

pub struct AnthropicStreamProcessor {
    tx: mpsc::Sender<StreamUpdate>,
}

impl AnthropicStreamProcessor {
    pub fn new(tx: mpsc::Sender<StreamUpdate>) -> Self {
        Self { tx }
    }

    async fn send_update(&self, update: StreamUpdate) -> Result<(), ToolError> {
        self.tx
            .send(update)
            .await
            .map_err(|e| ToolError::BigSearchError(format!("Failed to send stream update: {}", e)))?;

        Ok(())
    }
}

#[async_trait]
impl StreamProcessor for AnthropicStreamProcessor {
    async fn send_update(&self, update: StreamUpdate) -> Result<(), String> {
        self.send_update(update)
            .await
            .map_err(|e| e.to_string())
    }

    async fn finalize(&self, final_response: AnthropicComputerResponse) -> Result<String, String> {
        let final_update = StreamUpdate {
            content: final_response.content.clone(),
            language: final_response.language.clone(),
            error: final_response.error.clone(),
        };

        self.send_update(final_update)
            .await
            .map_err(|e| e.to_string())?;

        Ok(serde_json::to_string(&final_response).map_err(|e| e.to_string())?)
    }
}
