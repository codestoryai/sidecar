use async_trait::async_trait;
use tokio::time::{sleep, Duration};

use super::{
    errors::ToolError,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale, ToolType},
};

pub struct WaitTool;

impl WaitTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for WaitTool {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let duration_ms = if let Ok(wait_request) = input.is_wait() {
            wait_request.duration_ms
        } else {
            return Err(ToolError::WrongToolInput(ToolType::Wait));
        };

        sleep(Duration::from_millis(duration_ms)).await;
        Ok(ToolOutput::Wait)
    }

    fn tool_description(&self) -> String {
        "Waits for a specified duration in milliseconds".to_owned()
    }

    fn tool_input_format(&self) -> String {
        r#"<wait>
<duration_ms>number of milliseconds to wait</duration_ms>
</wait>"#
            .to_owned()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}