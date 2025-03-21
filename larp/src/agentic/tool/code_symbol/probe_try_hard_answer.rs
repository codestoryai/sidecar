use async_trait::async_trait;
use std::sync::Arc;

use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage},
};

use crate::agentic::{
    symbol::identifier::LLMProperties,
    tool::{
        errors::ToolError,
        input::ToolInput,
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale},
    },
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProbeTryHardAnswerSymbolRequest {
    original_request: String,
    probe_request: String,
    symbol_content: String,
    llm_properties: LLMProperties,
    root_request_id: String,
}

impl ProbeTryHardAnswerSymbolRequest {
    pub fn new(
        original_request: String,
        probe_request: String,
        symbol_content: String,
        llm_properties: LLMProperties,
        root_request_id: String,
    ) -> Self {
        Self {
            original_request,
            probe_request,
            symbol_content,
            llm_properties,
            root_request_id,
        }
    }
}

pub struct ProbeTryHardAnswer {
    llm_client: Arc<LLMBroker>,
    fallback_llm: LLMProperties,
}

impl ProbeTryHardAnswer {
    pub fn new(llm_client: Arc<LLMBroker>, fallback_llm: LLMProperties) -> Self {
        Self {
            llm_client,
            fallback_llm,
        }
    }

    fn system_message(&self) -> String {
        r#"You are an expert software engineer who is helping a user explore the codebase. During the exploration we have reached a point where there are no further code symbols to follow and we have to reply to the user.
- The original question which the user had asked when we started exploring the codebase is given in <user_query>.
- The question which we are asking at this point in the codebase to the current code in selection is given in <current_question>.
- You have to look at the code provided to you in <code> section and create a reply for the user. The reply should be at the most 100 words and be concise."#.to_owned()
    }

    fn user_message(&self, request: ProbeTryHardAnswerSymbolRequest) -> String {
        let user_query = request.original_request;
        let current_question = request.probe_request;
        let symbol_content = request.symbol_content;
        format!(
            r#"<user_query>
{user_query}
</user_query>

<current_question>
{current_question}
</current_question>

<code>
{symbol_content}
</code>"#
        )
    }
}

#[async_trait]
impl Tool for ProbeTryHardAnswer {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.get_probe_try_hard_answer_request()?;
        let root_request_id = context.root_request_id.to_owned();
        let llm_properties = context.llm_properties.clone();
        let system_message = LLMClientMessage::system(self.system_message());
        let user_message = LLMClientMessage::user(self.user_message(context));
        let llm_request = LLMClientCompletionRequest::new(
            llm_properties.llm().clone(),
            vec![system_message, user_message],
            0.2,
            None,
        );
        let mut retries = 0;
        loop {
            if retries > 4 {
                return Err(ToolError::MissingXMLTags);
            }
            let (llm, api_key, provider) = if retries % 2 == 0 {
                (
                    llm_properties.llm().clone(),
                    llm_properties.api_key().clone(),
                    llm_properties.provider().clone(),
                )
            } else {
                (
                    self.fallback_llm.llm().clone(),
                    self.fallback_llm.api_key().clone(),
                    self.fallback_llm.provider().clone(),
                )
            };
            let cloned_request = llm_request.clone().set_llm(llm);
            let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
            let response = self
                .llm_client
                .stream_completion(
                    api_key,
                    cloned_request,
                    provider,
                    vec![
                        (
                            "event_type".to_owned(),
                            "probe_try_hard_to_answer".to_owned(),
                        ),
                        ("root_id".to_owned(), root_request_id.to_owned()),
                    ]
                    .into_iter()
                    .collect(),
                    sender,
                )
                .await
                .map_err(|e| ToolError::LLMClientError(e));
            match response {
                Ok(response) => {
                    if response.answer_up_until_now().is_empty() {
                        retries = retries + 1;
                        continue;
                    } else {
                        return Ok(ToolOutput::ProbeTryHardAnswer(
                            response.answer_up_until_now().to_owned(),
                        ));
                    }
                }
                Err(e) => {
                    println!("tool::probe_try_hard_answer::invoke::error({:?})", e);
                    retries = retries + 1;
                    continue;
                }
            }
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
