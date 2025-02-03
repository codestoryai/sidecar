use async_trait::async_trait;
use crate::agentic::tool::{
    errors::ToolError,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale},
};

pub struct WaitTool;

impl WaitTool {
    pub fn new() -> Self {
        WaitTool
    }
}

#[async_trait]
impl Tool for WaitTool {
    async fn invoke(&self, _input: ToolInput) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::code_edit_output("Waiting...".to_owned()))
    }

    fn tool_description(&self) -> String {
        "Wait tool: provides a waiting state for post completion sanity check".to_owned()
    }

    fn tool_input_format(&self) -> String {
        "{}".to_owned() // No specific input
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}