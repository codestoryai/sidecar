//! Client which generates the reward for the action we have taken

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
pub struct RewardGenerationRequest {
    llm_messages: Vec<LLMClientMessage>,
    message_properties: SymbolEventMessageProperties,
}

impl RewardGenerationRequest {
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
#[serde(rename = "reward")]
#[serde(rename_all = "lowercase")]
pub struct RewardGenerationResponse {
    explanation: String,
    feedback: Option<String>,
    value: i32,
}

impl RewardGenerationResponse {
    pub fn explanation(&self) -> &str {
        &self.explanation
    }

    pub fn feedback(&self) -> Option<String> {
        self.feedback.clone()
    }

    pub fn value(&self) -> i32 {
        self.value
    }

    fn parse_output(output: String) -> Result<Self, ToolError> {
        let lines = output
            .lines()
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        enum RewardParsing {
            NoBlock,
            BlockStart,
            ExplanationStart,
            FeedbackStart,
            ValueStart,
        }
        let mut state = RewardParsing::NoBlock;
        let mut explanation = vec![];
        let mut feedback = vec![];
        let mut value = None;
        for line in lines.into_iter() {
            match state {
                RewardParsing::NoBlock => {
                    if line == "<reward>" {
                        state = RewardParsing::BlockStart;
                    }
                }
                RewardParsing::BlockStart => {
                    if line == "<explanation>" {
                        state = RewardParsing::ExplanationStart;
                    }
                    if line == "<feedback>" {
                        state = RewardParsing::FeedbackStart;
                    }
                    if line == "<value>" {
                        state = RewardParsing::ValueStart;
                    }
                    if line == "</reward>" {
                        state = RewardParsing::NoBlock;
                    }
                }
                RewardParsing::ExplanationStart => {
                    if line == "</explanation>" {
                        state = RewardParsing::BlockStart;
                    } else {
                        explanation.push(line);
                    }
                }
                RewardParsing::FeedbackStart => {
                    if line == "</feedback>" {
                        state = RewardParsing::BlockStart;
                    } else {
                        feedback.push(line);
                    }
                }
                RewardParsing::ValueStart => {
                    if line == "</value>" {
                        state = RewardParsing::BlockStart;
                    } else {
                        value = line.parse::<i32>().ok();
                    }
                }
            }
        }

        match value {
            Some(value) => Ok(RewardGenerationResponse {
                explanation: explanation.join("\n"),
                feedback: Some(feedback.join("\n")),
                value,
            }),
            None => Err(ToolError::SerdeConversionFailed),
        }
    }
}

pub struct RewardClientGenerator {
    llm_client: Arc<LLMBroker>,
}

impl RewardClientGenerator {
    pub fn new(llm_client: Arc<LLMBroker>) -> Self {
        Self { llm_client }
    }
}

#[async_trait]
impl Tool for RewardClientGenerator {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_reward_generation_request()?;
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
                    ("event_type".to_owned(), "reward_generation".to_owned()),
                ]
                .into_iter()
                .collect(),
                sender,
            )
            .await;

        match response {
            Ok(response) => {
                let output = RewardGenerationResponse::parse_output(
                    response.answer_up_until_now().to_owned(),
                )?;
                Ok(ToolOutput::RewardGeneration(output))
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
    use super::RewardGenerationResponse;

    #[test]
    fn test_parsing_bad_output() {
        let raw_input = format!(
            r#"<reward>
<explanation>
The last executed action was a search for the definition of the `escape` function in Python files, which is directly relevant to the task at hand. The search successfully located the target function in django/utils/html.py, which is exactly where we need to make changes. This is a good first step as it confirms the location of the code we need to modify and allows us to proceed with implementing the replacement using Python's stdlib html.escape(). The search parameters were well-defined and specific, yielding precisely the relevant result needed.
</explanation>
<feedback>
While the current approach is to directly replace the implementation, an alternative branch could:
1. First analyze the test suite to understand all use cases of the current escape() function
2. Consider creating a wrapper function that maintains backwards compatibility for the '&#39' vs '&#x27' difference
3. Add deprecation warnings for any cases where the behavior might differ
4. Implement a gradual migration strategy across multiple Django versions
</feedback>
<value>
85
</value>
</reward>"#
        );
        let parsed_outcome = RewardGenerationResponse::parse_output(raw_input);
        assert!(parsed_outcome.is_ok());
    }
}
