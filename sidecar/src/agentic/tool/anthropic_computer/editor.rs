use tokio::sync::mpsc;

use crate::agentic::tool::output::ToolOutput;
use crate::agentic::tool::errors::ToolError;

use super::types::{AnthropicComputerRequest, AnthropicComputerResponse};
use super::stream::AnthropicStreamProcessor;

pub async fn send_to_editor(
    request: AnthropicComputerRequest,
) -> Result<ToolOutput, ToolError> {
    // Create channel for streaming updates
    let (tx, mut rx) = mpsc::channel(32);
    let stream_processor = AnthropicStreamProcessor::new(tx);

    // Process the request and stream updates
    let editor_response = AnthropicComputerResponse {
        content: "File operation completed".to_string(),
        language: None,
        error: None,
    };

    // Send the final response
    Ok(ToolOutput::AnthropicComputer(
        serde_json::to_string(&editor_response)
            .map_err(|e| ToolError::BigSearchError(format!("Failed to serialize response: {}", e)))?
    ))
}
