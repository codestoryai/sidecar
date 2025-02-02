//! The wait tool allows the LLM to wait ... taking a step back when required
//! and focussing attention on something, all new llms are used to doing this internall
//! this can be used to also reduce the context from exploding

use axum::async_trait;
use serde::{Deserialize, Serialize};
use crate::agentic::tool::{
    r#type::{Tool, ToolType, ToolRewardScale},
    input::ToolInput,
    output::ToolOutput,
    errors::ToolError,
};
use crate::repo::types::RepoRef;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitInputPartial {
    pub repo_ref: RepoRef,
}

pub struct Wait;

impl Wait {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for Wait {
    async fn invoke(&self, _input: ToolInput) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::CodeEditTool("Wait operation completed".to_string()))
    }

    fn tool_description(&self) -> String {
        "Tool to perform a wait operation".to_string()
    }

    fn tool_input_format(&self) -> String {
        r#"<wait>
    <repo_ref>Repository reference</repo_ref>
</wait>"#.to_string()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec!["Wait operation completed successfully".to_string()]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![ToolRewardScale::new(0, 1, "Wait operation completed")]
    }
}