//! Should we edit the code or is it more of just a check

use async_trait::async_trait;
use quick_xml::de::from_str;
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

#[derive(Debug, Clone)]
pub struct ShouldEditCodeSymbolRequest {
    symbol_content: String,
    request: String,
    llm_properties: LLMProperties,
    root_request_id: String,
}

impl ShouldEditCodeSymbolRequest {
    pub fn new(
        symbol_content: String,
        request: String,
        llm_properties: LLMProperties,
        root_request_id: String,
    ) -> Self {
        Self {
            symbol_content,
            request,
            llm_properties,
            root_request_id,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename = "reply")]
pub struct ShouldEditCodeSymbolResponse {
    thinking: String,
    should_edit: bool,
}

impl ShouldEditCodeSymbolResponse {
    fn parse_response(response: &str) -> Result<Self, ToolError> {
        let lines = response
            .lines()
            .skip_while(|line| !line.contains(&format!("<reply>")))
            .skip(1)
            .take_while(|line| !line.contains(&format!("</reply>")))
            .collect::<Vec<_>>()
            .join("\n");
        let formatted_lines = format!(
            r#"<reply>
{lines}
</reply>"#
        );
        from_str::<Self>(&formatted_lines).map_err(|_e| ToolError::SerdeConversionFailed)
    }

    pub fn thinking(&self) -> &str {
        &self.thinking
    }

    pub fn should_edit(&self) -> bool {
        self.should_edit
    }
}

pub struct ShouldEditCodeSymbol {
    llm_client: Arc<LLMBroker>,
    _fail_over_llm: LLMProperties,
}

impl ShouldEditCodeSymbol {
    pub fn new(llm_client: Arc<LLMBroker>, fail_over_llm: LLMProperties) -> Self {
        Self {
            llm_client,
            _fail_over_llm: fail_over_llm,
        }
    }

    fn system_message(&self) -> String {
        r#"You are an expert software engineer who is tasked with figuring out if we need to edit the code to satisfy the user instruction of it all the changes are already present.
- You have to look carefully at the code which will be present in <code_to_edit> section
- The user query is given in <user_query> section and is that instruction which the user has asked us to perform.
- Before deciding if we want to edit, we are going to think for a bit, put your thoughts in the <thinking> section.
- Then we are going to output true if we want to edit or false if we do not need to edit any code.
- Your reply should be strictly in the following format:
<reply>
<thinking>
{your thoughts on if we should edit this code}
</thinking>
<should_edit>
{true of false depending on your analysis}
</should_edit>
</reply>"#.to_owned()
    }

    fn user_message(&self, context: ShouldEditCodeSymbolRequest) -> String {
        let instruction = context.request;
        let code_to_edit = context.symbol_content;
        format!(
            r#"<user_instruction>
{instruction}
</user_instruction>
<code_to_edit>
{code_to_edit}
</code_to_edit>"#
        )
    }
}

#[async_trait]
impl Tool for ShouldEditCodeSymbol {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.should_edit_code()?;
        let llm_properties = context.llm_properties.clone();
        let root_request_id = context.root_request_id.to_owned();
        let system_message = LLMClientMessage::system(self.system_message());
        let user_message = LLMClientMessage::user(self.user_message(context));
        let request = LLMClientCompletionRequest::new(
            llm_properties.llm().clone(),
            vec![system_message, user_message],
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
                        "event_type".to_owned(),
                        "should_edit_code_symbol".to_owned(),
                    ),
                    ("root_id".to_owned(), root_request_id),
                ]
                .into_iter()
                .collect(),
                sender,
            )
            .await
            .map_err(|e| ToolError::LLMClientError(e))?;
        let parsed_response =
            ShouldEditCodeSymbolResponse::parse_response(response.answer_up_until_now())?;
        Ok(ToolOutput::should_edit_code(parsed_response))
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
