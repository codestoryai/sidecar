//! Generates the feedback for the trajectory

use async_trait::async_trait;
use std::sync::Arc;

use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage},
};

use crate::agentic::{
    symbol::events::message_event::SymbolEventMessageProperties,
    tool::{
        errors::ToolError,
        input::ToolInput,
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale},
    },
};

#[derive(Debug, Clone)]
pub struct FeedbackGenerationRequest {
    llm_messages: Vec<LLMClientMessage>,
    message_properties: SymbolEventMessageProperties,
}

impl FeedbackGenerationRequest {
    pub fn new(
        llm_messages: Vec<LLMClientMessage>,
        message_properties: SymbolEventMessageProperties,
    ) -> Self {
        Self {
            llm_messages,
            message_properties,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename = "feedback_generation")]
#[serde(rename_all = "lowercase")]
pub struct FeedbackGenerationResponse {
    analysis: String,
    feedback: String,
}

impl FeedbackGenerationResponse {
    pub fn analysis(&self) -> &str {
        &self.analysis
    }

    pub fn feedback(&self) -> &str {
        &self.feedback
    }

    fn parse_response(response: String) -> Result<Self, ToolError> {
        let lines = response
            .lines()
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        enum FeedbackParsing {
            NoBlock,
            BlockStart,
            AnalysisStart,
            FeedbackStart,
        }
        let mut state = FeedbackParsing::NoBlock;
        let mut analysis = vec![];
        let mut feedback = vec![];
        for line in lines.into_iter() {
            match state {
                FeedbackParsing::NoBlock => {
                    if line == "<feedback_generation>" {
                        state = FeedbackParsing::BlockStart;
                    }
                }
                FeedbackParsing::BlockStart => {
                    if line == "<analysis>" {
                        state = FeedbackParsing::AnalysisStart;
                    }
                    if line == "<feedback>" {
                        state = FeedbackParsing::FeedbackStart;
                    }
                    if line == "</feedback_generation>" {
                        state = FeedbackParsing::NoBlock;
                    }
                }
                FeedbackParsing::AnalysisStart => {
                    if line == "</analysis>" {
                        state = FeedbackParsing::BlockStart;
                    } else {
                        analysis.push(line);
                    }
                }
                FeedbackParsing::FeedbackStart => {
                    if line == "</feedback>" {
                        state = FeedbackParsing::BlockStart;
                    } else {
                        feedback.push(line);
                    }
                }
            }
        }

        Ok(FeedbackGenerationResponse {
            analysis: analysis.join("\n"),
            feedback: feedback.join("\n"),
        })
    }
}

pub struct FeedbackClientGenerator {
    llm_client: Arc<LLMBroker>,
}

impl FeedbackClientGenerator {
    pub fn new(llm_client: Arc<LLMBroker>) -> Self {
        Self { llm_client }
    }
}

#[async_trait]
impl Tool for FeedbackClientGenerator {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_feedback_generation_request()?;
        let message_properties = context.message_properties.clone();
        let llm_properties = message_properties.llm_properties().clone();
        let request = LLMClientCompletionRequest::new(
            llm_properties.llm().clone(),
            context.llm_messages,
            0.2,
            None,
        );

        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let response = self
            .llm_client
            .stream_completion(
                llm_properties.api_key().clone(),
                request,
                llm_properties.provider().clone(),
                vec![
                    (
                        "root_id".to_owned(),
                        message_properties.root_request_id().to_owned(),
                    ),
                    ("event_type".to_owned(), "feedback_generation".to_owned()),
                ]
                .into_iter()
                .collect(),
                sender,
            )
            .await;

        match response {
            Ok(response) => {
                let output = FeedbackGenerationResponse::parse_response(
                    response.answer_up_until_now().to_owned(),
                )?;
                Ok(ToolOutput::FeedbackGeneration(output))
            }
            Err(e) => Err(ToolError::LLMClientError(e)),
        }
    }

    fn tool_description(&self) -> String {
        "".to_owned()
    }

    fn tool_input_format(&self) -> String {
        "".to_owned()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}

#[cfg(test)]
mod tests {

    use super::FeedbackGenerationResponse;

    #[test]
    fn test_feedback_generation_parsing() {
        let output = r#"<feedback_generation>
<analysis>
Analysis of the current task we are on and the different trajectories we have explored
</analysis>
<feedback>
Direct feedback to the AI agent
</feedback>
</feedback_generation>
"#;
        let parsed_output = FeedbackGenerationResponse::parse_response(output.to_owned());
        println!("{:?}", &parsed_output);
        assert!(parsed_output.is_ok());
    }
}
