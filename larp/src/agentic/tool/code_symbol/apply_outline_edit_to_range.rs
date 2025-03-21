//! Apply outline edits to a range allows us to rewrite a range with a gist
//! of the changes which need to be made

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage},
};

use crate::{
    agentic::{
        symbol::{
            identifier::{LLMProperties, SymbolIdentifier},
            ui_event::UIEventWithID,
        },
        tool::{
            errors::ToolError,
            input::ToolInput,
            output::ToolOutput,
            r#type::{Tool, ToolRewardScale},
        },
    },
    chunking::text_document::Range,
};

#[derive(Debug, Clone)]
pub struct ApplyOutlineEditsToRangeRequest {
    user_instruction: String,
    symbol_identifier: SymbolIdentifier,
    edited_file: String,
    code_in_selection: String,
    code_changes_outline: String,
    root_request_id: String,
    outline_range: Range,
    llm_properties: LLMProperties,
    edit_request_id: String,
    ui_sender: UnboundedSender<UIEventWithID>,
    session_id: String,
    exchange_id: String,
}

impl ApplyOutlineEditsToRangeRequest {
    pub fn new(
        user_instruction: String,
        symbol_identifier: SymbolIdentifier,
        edited_file: String,
        code_in_selection: String,
        code_changes_outline: String,
        root_request_id: String,
        outline_range: Range,
        llm_properties: LLMProperties,
        edit_request_id: String,
        ui_sender: UnboundedSender<UIEventWithID>,
        session_id: String,
        exchange_id: String,
    ) -> Self {
        Self {
            user_instruction,
            symbol_identifier,
            edited_file,
            code_in_selection,
            code_changes_outline,
            root_request_id,
            outline_range,
            llm_properties,
            edit_request_id,
            ui_sender,
            session_id,
            exchange_id,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApplyOutlineEditsToRangeResponse {
    code: String,
}

impl ApplyOutlineEditsToRangeResponse {
    fn parse_response(response: &str) -> Result<Self, ToolError> {
        let response_final_output = response
            .lines()
            .skip_while(|line| !line.contains("```"))
            .skip(1)
            .take_while(|line| !line.contains("```"))
            .collect::<Vec<_>>()
            .join("\n");
        if response.is_empty() {
            Err(ToolError::SerdeConversionFailed)
        } else {
            Ok(ApplyOutlineEditsToRangeResponse {
                code: response_final_output,
            })
        }
    }

    pub fn code(self) -> String {
        self.code
    }
}

pub struct ApplyOutlineEditsToRange {
    llm_client: Arc<LLMBroker>,
    fail_over_llm: LLMProperties,
    stream_apply: bool,
}

impl ApplyOutlineEditsToRange {
    pub fn new(
        llm_client: Arc<LLMBroker>,
        fail_over_llm: LLMProperties,
        stream_apply: bool,
    ) -> Self {
        Self {
            llm_client,
            fail_over_llm,
            stream_apply,
        }
    }

    fn system_message(&self) -> String {
        format!("You are an expert software engineer who is an expert at applying edits made by another engineer to the code.
- The junior engineer was tasked with making changes to the code which is present in <code_in_selection> and they made higher level changes which are present in <code_changes_outline>
- The instruction which was passed to the junior engineer is given in <user_instruction> section.
- You have to apply the changes made in <code_changes_outline> to <code_in_selection> and rewrite the code in <code_in_selection> after the changes have been made.
- Do not leave any placeholder comments or leave any logic out, applying the changes is pretty easy.
- We will show you some examples so you can understand how the changes need to be applied:

<user_instruction>
I want you to add log statements to this function
</user_instruction>

<code_in_selection>
```py
def add_values(a, b):
    return a + b

def subtract(a, b):
    return a - b
```
</code_in_selection>

<code_changes_outline>
def add_values(a, b, logger):
    logger.info(a, b)
    # rest of the code

def subtract(a, b, logger):
    logger.info(a, b)
    # rest of the code
</code_changes_outline>

<reply>
```py
def add_values(a, b, logger):
    logger.info(a, b)
    return a + b

def subtract(a, b, logger):
    logger.info(a, b)
    return a - b
```
</reply>")
    }

    fn user_message(&self, context: ApplyOutlineEditsToRangeRequest) -> String {
        let user_instruction = context.user_instruction;
        let code_in_selection = context.code_in_selection;
        let code_changes_outline = context.code_changes_outline;
        format!(
            r#"<user_instruction>
{user_instruction}
</user_instruction>

<code_in_selection>
{code_in_selection}
</code_in_selection>

<code_changes_outline>
{code_changes_outline}
</code_changes_outline>"#
        )
    }
}

#[async_trait]
impl Tool for ApplyOutlineEditsToRange {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.apply_outline_edits_to_range()?;
        let ui_sender = context.ui_sender.clone();
        let _code_in_selection = context.code_in_selection.to_owned();
        let symbol_identifier = context.symbol_identifier.clone();
        let edited_range = context.outline_range.clone();
        let edit_request_id = context.edit_request_id.to_owned();
        let root_request_id = context.root_request_id.to_owned();
        let exchange_id = context.exchange_id.to_owned();
        let session_id = context.session_id.to_owned();
        let llm_properties = context.llm_properties.clone();
        let fs_file_path = context.edited_file.to_owned();
        let system_message = LLMClientMessage::system(self.system_message());
        let user_message = LLMClientMessage::user(self.user_message(context));
        let llm_request = LLMClientCompletionRequest::new(
            llm_properties.llm().clone(),
            vec![system_message, user_message],
            0.2,
            None,
        );
        let mut retries = 0;
        // if we are stream applying the changes, we get a single chance over here
        // otherwise we can take our time and give multiple tries over here
        let retries_limit = if self.stream_apply { 1 } else { 4 };
        loop {
            if retries >= retries_limit {
                println!("apply_outline_edits_to_range::retries_exhausted::retries({})::retries_limit({})", retries, retries_limit);
                return Err(ToolError::RetriesExhausted);
            }
            let (llm, api_key, provider) = if retries % 2 == 0 {
                (
                    llm_properties.llm().clone(),
                    llm_properties.api_key().clone(),
                    llm_properties.provider().clone(),
                )
            } else {
                (
                    self.fail_over_llm.llm().clone(),
                    self.fail_over_llm.api_key().clone(),
                    self.fail_over_llm.provider().clone(),
                )
            };
            let cloned_message = llm_request.clone().set_llm(llm);
            let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
            let mut stream_future = Box::pin(
                self.llm_client.stream_completion(
                    api_key,
                    cloned_message,
                    provider,
                    vec![
                        (
                            "event_type".to_owned(),
                            "apply_outline_edits_to_range".to_owned(),
                        ),
                        ("root_id".to_owned(), root_request_id.to_owned()),
                        ("retries".to_owned(), retries.to_string()),
                    ]
                    .into_iter()
                    .collect(),
                    sender,
                ),
            );

            let stream_result;

            // send over a start event over here
            let _ = ui_sender.send(UIEventWithID::start_edit_streaming(
                root_request_id.to_owned(),
                symbol_identifier.clone(),
                edit_request_id.to_owned(),
                edited_range.clone(),
                fs_file_path.to_owned(),
                session_id.to_owned(),
                exchange_id.to_owned(),
                None,
            ));

            loop {
                tokio::select! {
                    stream_msg = receiver.recv() => {
                        match stream_msg {
                            Some(msg) => {
                                let delta = msg.delta();
                                if let Some(delta) = delta {
                                    // we want to send over the delta over here
                                    let _ = ui_sender.send(UIEventWithID::delta_edit_streaming(
                                        root_request_id.to_owned(),
                                        symbol_identifier.clone(),
                                        delta.to_owned(),
                                        edit_request_id.to_owned(),
                                        edited_range.clone(),
                                        fs_file_path.to_owned(),
                                        session_id.to_owned(),
                                        exchange_id.to_owned(),
                                        None,
                                    ));
                                }
                            }
                            None => {
                                // we still need to wait for the stream future to complete
                            }, // Channel closed
                        }
                    }
                    result = &mut stream_future => {
                        if let Ok(ref _result) = result {
                            let _ = ui_sender.send(UIEventWithID::end_edit_streaming(
                                root_request_id.to_owned(),
                                symbol_identifier.clone(),
                                edit_request_id.to_owned(),
                                edited_range.clone(),
                                fs_file_path.to_owned(),
                                session_id.to_owned(),
                                exchange_id.to_owned(),
                                None,
                            ));
                        } else {
                            // send over the original selection over here since we had an error
                            let _ = ui_sender.send(UIEventWithID::end_edit_streaming(
                                root_request_id.to_owned(),
                                symbol_identifier.clone(),
                                edit_request_id.to_owned(),
                                edited_range.clone(),
                                fs_file_path.to_owned(),
                                session_id.to_owned(),
                                exchange_id.to_owned(),
                                None,
                            ));
                        }
                        stream_result = Some(result);
                        break;
                    }
                }
            }
            match stream_result {
                Some(Ok(response)) => {
                    if let Ok(parsed_response) = ApplyOutlineEditsToRangeResponse::parse_response(
                        response.answer_up_until_now(),
                    ) {
                        return Ok(ToolOutput::apply_outline_edits_to_range(parsed_response));
                    } else {
                        retries = retries + 1;
                        continue;
                    }
                }
                _ => {
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
