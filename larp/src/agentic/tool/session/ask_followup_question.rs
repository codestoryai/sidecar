//! Asks followup questions to the user

use async_trait::async_trait;

use crate::agentic::tool::{
    errors::ToolError,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale},
};

pub struct AskFollowupQuestions {}

impl AskFollowupQuestions {
    pub fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AskFollowupQuestionsRequest {
    question: String,
}

impl AskFollowupQuestionsRequest {
    pub fn new(question: String) -> Self {
        Self { question }
    }

    pub fn question(&self) -> &str {
        &self.question
    }

    pub fn to_string(&self) -> String {
        format!(
            r#"<ask_followup_question>
<question>
{}
</question>
</ask_followup_question>"#,
            self.question
        )
    }
}

#[derive(Debug, Clone)]
pub struct AskFollowupQuestionsResponse {
    user_question: String,
}

impl AskFollowupQuestionsResponse {
    pub fn user_question(&self) -> &str {
        &self.user_question
    }
}

impl AskFollowupQuestionsResponse {
    pub fn new(user_question: String) -> Self {
        Self { user_question }
    }
}

#[async_trait]
impl Tool for AskFollowupQuestions {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_ask_followup_questions()?;
        let response = AskFollowupQuestionsResponse::new(context.question);
        Ok(ToolOutput::AskFollowupQuestions(response))
    }

    fn tool_description(&self) -> String {
        r#"### ask_followup_question
Ask the user a question to gather additional information needed to complete the task. This tool should be used when you encounter ambiguities, need clarification, or require more details to proceed effectively. It allows for interactive problem-solving by enabling direct communication with the user. Use this tool judiciously to maintain a balance between gathering necessary information and avoiding excessive back-and-forth."#.to_owned()
    }

    fn tool_input_format(&self) -> String {
        r#"Parameters:
- question: (required) The question to ask the user. This should be a clear, specific question that addresses the information you need.
Usage:
<ask_followup_question>
<question>
Your question here
</question>
</ask_followup_question>"#.to_owned()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}
