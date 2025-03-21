//! We might have multiple questions which we want to ask the code symbol
//! from a previous symbol (if they all lead to the same symbol)
//! The best way to handle this is to let a LLM figure out what is the best
//! question we can ask the symbol

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

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProbeQuestionForSymbolRequest {
    symbol_name: String,
    next_symbol_name: String,
    next_symbol_file_path: String,
    history: Vec<String>,
    // all the places we are linked with the current symbol
    hyperlinks: Vec<String>,
    original_user_query: String,
    llm_properties: LLMProperties,
    root_request_id: String,
}

impl ProbeQuestionForSymbolRequest {
    pub fn new(
        symbol_name: String,
        next_symbol_name: String,
        next_symbol_file_path: String,
        hyperlinks: Vec<String>,
        history: Vec<String>,
        original_user_query: String,
        llm_properties: LLMProperties,
        root_request_id: String,
    ) -> Self {
        Self {
            symbol_name,
            next_symbol_file_path,
            next_symbol_name,
            history,
            hyperlinks,
            original_user_query,
            llm_properties,
            root_request_id,
        }
    }
}

pub struct ProbeQuestionForSymbol {
    llm_client: Arc<LLMBroker>,
    fallback_llm: LLMProperties,
}

impl ProbeQuestionForSymbol {
    pub fn new(llm_client: Arc<LLMBroker>, fallback_llm: LLMProperties) -> Self {
        Self {
            llm_client,
            fallback_llm,
        }
    }

    fn system_message(&self) -> String {
        format!(
            r#"You are an expert software engineer who is going to write a question to pass on to another engineer.
- The symbol we will be asking the question to finally is given in <next_symbol_name> section.
- We are currently in the <symbol_name> section of the codebase, and we have figured out sections of the code where we want to jump to the <next_symbol_name>
- We are also given the list of code snippets which lead us to the new symbol which we are going to ask the question to.
- The position in the code which links the position we were on to the new symbol is given in the <hyperlink> section. The hyperlink section contains the code outline and the section of code which links to the <next_symbol_name> along with a thinking of why we want to go to <next_symbol_name>
- You are also given the thought process on why we decided to follow the symbol in the <hyperlink> section and that is present in the <thinking> section.
- We are also given the history of questions in the <history_of_questions> section which has lead us here along with the original question which the user asked in <original_query>, we want to answer the user query, so coming up with a question which helps us answer it is very essential.
- You are also given the history of all the question we have asked to various symbols so you can check the path the engineer has taken to reach this point.
- Your task is to write a new question to ask toe the <next_symbol_name> given the context in the <hyperlink> section
- A good question is one which helps uncover the maximum amount of details for the answer to the user query.
- When replying with the question the output format which you should follow is strictly in this format:
<question>
{{your question here}}
</question>
- Your reply should have <question> tag in the first line followed by the question and end with the </question> tag"#
        )
    }

    fn user_message(&self, request: ProbeQuestionForSymbolRequest) -> String {
        let original_user_query = request.original_user_query;
        let hyperlinks = request.hyperlinks.join("\n");
        let previous_symbol_name = request.symbol_name;
        let next_symbol_name = request.next_symbol_name;
        let fs_file_path = request.next_symbol_file_path;
        let history = request.history.join("\n");
        format!(
            r#"<user_query>
{original_user_query}
<user_query>

<symbol_name>
{previous_symbol_name}
</symbol_name>

<history_of_questions>
{history}
</history_of_questions>

<hyperlinks>
{hyperlinks}
</hyperlinks>

<next_symbol_name>
{next_symbol_name}
</next_symbol_name>
<next_symbol_file_path>
{fs_file_path}
</next_symbol_file_path>"#
        )
    }
}

#[async_trait]
impl Tool for ProbeQuestionForSymbol {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.get_probe_create_question_for_symbol()?;
        let root_request_id = context.root_request_id.to_owned();
        let llm_properties = context.llm_properties.clone();
        let system_message = LLMClientMessage::system(self.system_message());
        let user_message = LLMClientMessage::user(self.user_message(context));
        let request = LLMClientCompletionRequest::new(
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
            let mut cloned_request = request.clone();
            let llm_properties = if retries % 2 == 1 {
                self.fallback_llm.clone()
            } else {
                llm_properties.clone()
            };
            cloned_request = cloned_request.set_llm(llm_properties.llm().clone());
            let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
            let response = self
                .llm_client
                .stream_completion(
                    llm_properties.api_key().clone(),
                    cloned_request,
                    llm_properties.provider().clone(),
                    vec![
                        (
                            "event_type".to_owned(),
                            "probe_question_generation_for_symbol".to_owned(),
                        ),
                        ("root_id".to_owned(), root_request_id.to_owned()),
                    ]
                    .into_iter()
                    .collect(),
                    sender,
                )
                .await;
            if let Ok(response) = response {
                if response.answer_up_until_now().contains("<question>")
                    && response.answer_up_until_now().contains("</question>")
                {
                    // then we can grab the response here between the question tags and the send it back
                    let parsed_response = response
                        .answer_up_until_now()
                        .lines()
                        .into_iter()
                        .skip_while(|line| !line.contains("<question>"))
                        .skip(1)
                        .take_while(|line| !line.contains("</question>"))
                        .collect::<Vec<&str>>()
                        .join("\n");
                    return Ok(ToolOutput::ProbeCreateQuestionForSymbol(parsed_response));
                } else {
                    retries = retries + 1;
                }
            } else {
                retries = retries + 1;
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
