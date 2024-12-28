use serde_json::json;

use crate::agentic::tool::input::ToolInput;
use crate::agentic::tool::output::ToolOutput;
use crate::agentic::tool::r#type::Tool;

mod editor;
mod operations;
mod stream;
mod types;

use types::AnthropicComputerRequest;

#[derive(Debug)]
pub enum AnthropicComputerError {
    FileOperationError(String),
}

pub struct AnthropicComputerTool {
    editor_url: String,
}

impl AnthropicComputerTool {
    pub fn new(editor_url: String) -> Self {
        Self { editor_url }
    }

    pub async fn open_file(&self, fs_file_path: String) -> Result<String, AnthropicComputerError> {
        let input = ToolInput::AnthropicComputer(json!({
            "fs_file_path": fs_file_path,
            "editor_url": self.editor_url.clone(),
            "operation": "open",
            "changes": null
        }));

        self.invoke(input)
            .await
            .map(|output| match output {
                ToolOutput::AnthropicComputer(content) => content,
                _ => String::new(),
            })
            .map_err(|e| AnthropicComputerError::FileOperationError(e.to_string()))
    }

    pub async fn view_file(&self, fs_file_path: String) -> Result<String, AnthropicComputerError> {
        let input = ToolInput::AnthropicComputer(json!({
            "fs_file_path": fs_file_path,
            "editor_url": self.editor_url.clone(),
            "operation": "view",
            "changes": null
        }));

        self.invoke(input)
            .await
            .map(|output| match output {
                ToolOutput::AnthropicComputer(content) => content,
                _ => String::new(),
            })
            .map_err(|e| AnthropicComputerError::FileOperationError(e.to_string()))
    }

    pub async fn edit_file(
        &self,
        fs_file_path: String,
        changes: String,
    ) -> Result<String, AnthropicComputerError> {
        let input = ToolInput::AnthropicComputer(json!({
            "fs_file_path": fs_file_path,
            "editor_url": self.editor_url.clone(),
            "operation": "edit",
            "changes": changes
        }));

        self.invoke(input)
            .await
            .map(|output| match output {
                ToolOutput::AnthropicComputer(content) => content,
                _ => String::new(),
            })
            .map_err(|e| AnthropicComputerError::FileOperationError(e.to_string()))
    }
}
