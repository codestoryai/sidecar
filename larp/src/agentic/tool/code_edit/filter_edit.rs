//! Contains the context for filtering on an edit opeeration, this is part of the
//! symbol's internal monologue and it is checking what to do about the edit_operation
//! if the edit_operation should be even worked on, this is assuming that edits happen
//! anyways

use async_trait::async_trait;
use serde_xml_rs::from_str;
use std::sync::Arc;

use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage, LLMType},
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct FilterEditOperationRequest {
    code_in_selection: String,
    symbol_name: String,
    fs_file_path: String,
    user_instruction: String,
    reason_to_edit: String,
    llm_properties: LLMProperties,
    root_id: String,
}

impl FilterEditOperationRequest {
    pub fn new(
        code_in_selection: String,
        symbol_name: String,
        fs_file_path: String,
        user_instruction: String,
        reason_to_edit: String,
        llm_properties: LLMProperties,
        root_id: String,
    ) -> Self {
        Self {
            code_in_selection,
            symbol_name,
            fs_file_path,
            user_instruction,
            reason_to_edit,
            llm_properties,
            root_id,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FilterEditOperationResponse {
    thinking: String,
    should_edit: bool,
}

impl FilterEditOperationResponse {
    pub fn new(thinking: String, should_edit: bool) -> Self {
        Self {
            thinking,
            should_edit,
        }
    }

    pub fn should_edit(&self) -> bool {
        self.should_edit
    }

    pub fn thinking(&self) -> &str {
        &self.thinking
    }
}

pub struct FilterEditOperationBroker {
    llm_client: Arc<LLMBroker>,
    fail_over_llm: LLMProperties,
}

impl FilterEditOperationBroker {
    pub fn new(llm_client: Arc<LLMBroker>, fail_over_llm: LLMProperties) -> Self {
        Self {
            llm_client,
            fail_over_llm,
        }
    }
}

impl FilterEditOperationBroker {
    fn llm_support(&self, llm_type: &LLMType) -> bool {
        match llm_type {
            LLMType::ClaudeHaiku | LLMType::ClaudeOpus | LLMType::ClaudeSonnet => true,
            LLMType::GeminiProFlash | LLMType::GeminiPro => true,
            LLMType::Llama3_1_8bInstruct => true,
            _ => false,
        }
    }

    fn system_message(&self, symbol_name: &str) -> String {
        format!(
            r#"You are an expert senior software engineer whose is going to check if the reason to editing the junior engineer has come up with is correct and needs to be done.

- You are working with a junior engineer who is a fast coder but might repeat work they have already done.
- Your job is to look at the code present in <code_to_edit> section and the reason for editing which is given in <reason_to_edit> section and reply with yes or no in xml format (which we will show you) and your thinking
- This is part of a greater change which a user wants to get done on the codebase which is given in <user_instruction>
- Before replying you should think for a bit less than 5 sentences and then decide if you want to edit or not and put `true` or `false` in the should_edit section
- You should be careful to decide the following:
- - We are right now working with {symbol_name} so if the change instruction is not related to it we should reject it
- - If the changes to {symbol_name} are already done to satisfy the task then we should reject it
- - If the changes are absolutely necessary then we should do it
- - Just be careful since you are the senior engineer and you have to provide feedback to the junior engineer and let them know the reason for your verdict on true or false

Now to show you the reply format:
<reply>
<thinking>
{{your thoughts here if the edit should be done}}
</thinking>
<should_edit>
{{true or false}}
</should_edit>
</reply>

The input will be in the following format:
<user_instruction>
{{the user instruction which generated this edit request}}
</user_instruction>
<reason_to_edit>
{{the reason for selecting this section of code for editing}}
</reason_to_edit>
<code_to_edit>
{{code which we want to edit}}
</code_to_edit>"#
        )
    }

    fn user_message(&self, context: FilterEditOperationRequest) -> String {
        let user_instruction = context.user_instruction;
        let edit_operation = context.reason_to_edit;
        let code_to_edit = context.code_in_selection;
        let file_path = context.fs_file_path;
        format!(
            r#"<user_instruction>
{user_instruction}
</user_instruction>
<reason_to_edit>
{edit_operation}
</reason_to_edit>
<code_to_edit>
```rust
FILEPATH: {file_path}
{code_to_edit}
```
</code_to_edit>"#
        )
    }
}

#[async_trait]
impl Tool for FilterEditOperationBroker {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.filter_edit_operation_request()?;
        let llm_properties = context.llm_properties.clone();
        let root_id = context.root_id.to_owned();
        // we do not support the llm type
        if !self.llm_support(llm_properties.llm()) {
            return Err(ToolError::LLMNotSupported);
        }
        let system_message = self.system_message(&context.symbol_name);
        let user_message = self.user_message(context);
        let llm_request = LLMClientCompletionRequest::new(
            llm_properties.llm().clone(),
            vec![
                LLMClientMessage::system(system_message),
                LLMClientMessage::user(user_message),
            ],
            0.2,
            None,
        );
        let llm_request_ref = &llm_request;
        let mut retries = 0;
        loop {
            if retries > 2 {
                return Ok(ToolOutput::filter_edit_operation(
                    FilterEditOperationResponse::new(
                        "retries_exhausted_assume_true".to_owned(),
                        true,
                    ),
                ));
            }
            retries = retries + 1;
            let retry_llm_properties = if retries % 2 == 0 {
                llm_properties.clone()
            } else {
                self.fail_over_llm.clone()
            };
            let llm_request = llm_request_ref.clone();
            let retry_llm_request = llm_request.set_llm(retry_llm_properties.llm().clone());
            let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
            let response = self
                .llm_client
                .stream_completion(
                    retry_llm_properties.api_key().clone(),
                    retry_llm_request,
                    retry_llm_properties.provider().clone(),
                    vec![
                        ("event_type".to_owned(), "filter_edit_operation".to_owned()),
                        ("root_id".to_owned(), root_id.to_owned()),
                    ]
                    .into_iter()
                    .collect(),
                    sender,
                )
                .await;
            match response {
                Ok(response) => {
                    let parsed_response =
                        from_str::<FilterEditOperationResponse>(response.answer_up_until_now());
                    match parsed_response {
                        Ok(parsed_response) => {
                            return Ok(ToolOutput::filter_edit_operation(parsed_response));
                        }
                        Err(_e) => {
                            retries = retries + 1;
                            continue;
                        }
                    }
                }
                Err(_) => {
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
