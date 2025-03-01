//! Allows the agent to ask an expert for help or clarification

use async_trait::async_trait;

use crate::agentic::tool::{
    errors::ToolError,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale},
};

pub struct AskExpert {}

impl AskExpert {
    pub fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AskExpertRequest {
    question: String,
}

impl AskExpertRequest {
    pub fn new(question: String) -> Self {
        Self { question }
    }

    pub fn question(&self) -> &str {
        &self.question
    }

    pub fn to_string(&self) -> String {
        format!(
            r#"<ask_expert>
<question>
{}
</question>
</ask_expert>"#,
            self.question
        )
    }
}

#[derive(Debug, Clone)]
pub struct AskExpertResponse {
    expert_question: String,
}

impl AskExpertResponse {
    pub fn expert_question(&self) -> &str {
        &self.expert_question
    }
}

impl AskExpertResponse {
    pub fn new(expert_question: String) -> Self {
        Self { expert_question }
    }
}

#[async_trait]
impl Tool for AskExpert {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_ask_expert()?;
        let response = AskExpertResponse::new(context.question);
        Ok(ToolOutput::ask_expert(response))
    }

    fn tool_description(&self) -> String {
        r#"### ask_expert
Ask an expert for help or clarification when facing a difficult problem or when you need specialized knowledge. This tool allows the agent to request assistance from a domain expert when it encounters complex issues, feels stuck, or needs guidance on a challenging task. Use this tool when you need insights that go beyond your current capabilities or when you need validation for a proposed approach."#.to_owned()
    }

    fn tool_input_format(&self) -> String {
        r#"Parameters:
- question: (required) The question or request for the expert. This should clearly explain the problem you're facing and what kind of expertise you need.
Usage:
<ask_expert>
<question>
Your question for the expert here
</question>
</ask_expert>"#.to_owned()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}