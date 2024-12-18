use async_trait::async_trait;

use crate::agentic::tool::r#type::{Tool, ToolRewardScale};
use crate::agentic::tool::input::ToolInput;
use crate::agentic::tool::output::ToolOutput;
use crate::agentic::tool::errors::ToolError;

use super::{AnthropicComputerTool, AnthropicComputerRequest};

#[async_trait]
impl Tool for AnthropicComputerTool {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let request: AnthropicComputerRequest = serde_json::from_value(match input {
            ToolInput::AnthropicComputer(value) => value,
            _ => return Err(ToolError::BigSearchError("Invalid input type".to_string())),
        })
        .map_err(|e| ToolError::BigSearchError(format!("Failed to parse request: {}", e)))?;

        // Delegate to editor communication implementation
        crate::agentic::tool::anthropic_computer::editor::send_to_editor(request).await
    }

    fn tool_description(&self) -> String {
        "Tool for integrating Anthropic's computer use capabilities with editor operations".to_string()
    }

    fn tool_input_format(&self) -> String {
        r#"{
            "fs_file_path": "path to the file",
            "editor_url": "URL of the editor",
            "operation": "open|view|edit",
            "changes": "optional changes for edit operation"
        }"#.to_string()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![
            "File operations executed successfully".to_string(),
            "Proper error handling".to_string(),
            "Streaming updates provided".to_string(),
        ]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![ToolRewardScale::new(
            0,
            1,
            "Success rate of file operations",
        )]
    }
}
