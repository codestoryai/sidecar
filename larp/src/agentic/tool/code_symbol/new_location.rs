//! Finds the location of the new code symbol which we are inserting

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

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename = "reply")]
pub struct CodeSymbolNewLocationResponse {
    thinking: String,
    idx: usize,
}

impl CodeSymbolNewLocationResponse {
    fn parse_response(response: &str) -> Result<Self, ToolError> {
        let lines = response
            .lines()
            .skip_while(|line| !line.contains("<reply>"))
            .skip(1)
            .take_while(|line| !line.contains("</reply>"))
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

    pub fn idx(&self) -> usize {
        self.idx
    }
}

#[derive(Debug, Clone)]
pub struct CodeSymbolNewLocationRequest {
    fs_file_path: String,
    outline_nodes: Vec<String>,
    symbol_name: String,
    add_request: String,
    llm_properties: LLMProperties,
    root_request_id: String,
}

impl CodeSymbolNewLocationRequest {
    pub fn new(
        fs_file_path: String,
        outline_nodes: Vec<String>,
        symbol_name: String,
        add_request: String,
        llm_properties: LLMProperties,
        root_request_id: String,
    ) -> Self {
        Self {
            fs_file_path,
            outline_nodes,
            symbol_name,
            add_request,
            llm_properties,
            root_request_id,
        }
    }
}

pub struct CodeSymbolNewLocation {
    llm_client: Arc<LLMBroker>,
    fail_over_llm: LLMProperties,
}

impl CodeSymbolNewLocation {
    pub fn new(llm_client: Arc<LLMBroker>, fail_over_llm: LLMProperties) -> Self {
        Self {
            llm_client,
            fail_over_llm,
        }
    }

    fn system_message(&self) -> String {
        "r#You are an expert software engineer who is tasked with finding the right location to place new code.
- The symbol we want to add is mentioned in <symbol_name> section and the reason we are adding it is present in <reason_to_add> section.
- We will be presented a list of sections of the code section where the number of the section is mentioned in:
<idx>
{id number for the section}
</idx>
- The user has asked you to find the location where we should add this new code, you have to reply in the following format:
<reply>
<thinking>
{your thoughts on how it should work}
</thinking>
<idx>
{the number of the section where we should be adding the code, we will add the code at the top of that section}
</idx>
</reply>
- You will first think for a bit, use 2 or less sentences to plan out where we should add the new code and then reply with the section number where we should add it, we are adding the code at the top of the section.
- The edge case when you want to add the code at the end of the file, just give back the last empty section number which is empty and we can add it to that#".to_owned()
    }

    fn user_message(&self, context: CodeSymbolNewLocationRequest) -> String {
        let add_request = context.add_request;
        let symbol_name = context.symbol_name;
        let fs_file_path = context.fs_file_path;
        let mut outlines = context
            .outline_nodes
            .into_iter()
            .enumerate()
            .map(|(idx, outline_node)| {
                format!(
                    r#"<section>
<idx>
{idx}
</idx>
<content>
{outline_node}
</content>
</section>"#
                )
            })
            .collect::<Vec<_>>();
        let outlines_len = outlines.len();
        outlines.push(format!(
            r#"<section>
<idx>
{outlines_len}
</idx>
<content>
EOF
</content>
</section>"#
        ));
        let outlines_str = outlines.join("\n");
        format!(
            r#"<symbol_name>
{symbol_name}
</symbol_name>
<reason_to_add>
{add_request}
</reason_to_add>
<fs_file_path>
{fs_file_path}
</fs_file_path>
<outlines>
{outlines_str}
</outlines>"#
        )
    }
}

#[async_trait]
impl Tool for CodeSymbolNewLocation {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.code_symbol_new_location()?;
        let system_message = self.system_message();
        let root_id = context.root_request_id.to_owned();
        let llm_properties = context.llm_properties.clone();
        let user_message = self.user_message(context);
        let request = LLMClientCompletionRequest::new(
            llm_properties.llm().clone(),
            vec![
                LLMClientMessage::system(system_message),
                LLMClientMessage::user(user_message),
            ],
            0.2,
            None,
        );
        let mut retries = 0;
        loop {
            if retries >= 4 {
                return Err(ToolError::RetriesExhausted);
            }
            let llm_properties = if retries % 2 == 0 {
                llm_properties.clone()
            } else {
                self.fail_over_llm.clone()
            };
            let cloned_llm_message = request.clone().set_llm(llm_properties.llm().clone());
            let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
            let response = self
                .llm_client
                .stream_completion(
                    llm_properties.api_key().clone(),
                    cloned_llm_message,
                    llm_properties.provider().clone(),
                    vec![
                        (
                            "event_type".to_owned(),
                            "code_symbol_new_location".to_owned(),
                        ),
                        ("root_id".to_owned(), root_id.to_owned()),
                    ]
                    .into_iter()
                    .collect(),
                    sender,
                )
                .await
                .map_err(|e| ToolError::LLMClientError(e))
                .map(|answer| {
                    CodeSymbolNewLocationResponse::parse_response(answer.answer_up_until_now())
                });
            match response {
                Ok(Ok(parsed_answer)) => {
                    return Ok(ToolOutput::code_symbol_new_location(parsed_answer))
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
