use async_trait::async_trait;
use crate::agentic::tool::{errors::ToolError, input::ToolInput, output::ToolOutput, r#type::{Tool, ToolRewardScale}};

#[derive(Clone)]
pub struct WaitRequest {}

impl WaitRequest {
    pub fn new() -> Self {
        Self {}
    }
}

pub struct Wait {}

impl Wait {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl Tool for Wait {
    async fn invoke(&self, _input: ToolInput) -> Result<ToolOutput, ToolError> {
        // Wait tool just returns a success with no data
        Ok(ToolOutput::success("Wait operation completed".to_string()))
    }

    fn tool_description(&self) -> String {
        "Allows the agent to pause and think, exploring other options before proceeding".to_string()
    }

    fn tool_input_format(&self) -> String {
        "<Wait>\n</Wait>".to_string()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![
            "The Wait operation allows for reconsideration of approach".to_string(),
        ]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![ToolRewardScale::new(0, 1, "Wait operation completed successfully")]
    }
}