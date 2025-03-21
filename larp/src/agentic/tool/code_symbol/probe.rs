//! Contains information if the current symbol has all the information or we
//! need to go deeper into one of the sub-symbols

use async_trait::async_trait;
use std::sync::Arc;

use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage},
};

use crate::agentic::{
    symbol::identifier::LLMProperties,
    tool::{
        code_symbol::types::CodeSymbolError,
        errors::ToolError,
        input::ToolInput,
        jitter::jitter_sleep,
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale},
    },
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProbeEnoughOrDeeperRequest {
    symbol_name: String,
    xml_string: String,
    query: String,
    llm_properties: LLMProperties,
    root_request_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename = "code_snippet")]
pub struct ProbeDeeperSnippet {
    id: usize,
    reason_to_probe: String,
}

impl ProbeDeeperSnippet {
    pub fn new(id: usize, reason_to_probe: String) -> Self {
        Self {
            id,
            reason_to_probe,
        }
    }
    pub fn id(&self) -> usize {
        self.id.clone()
    }

    pub fn reason_to_probe(&self) -> &str {
        &self.reason_to_probe
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename = "code_to_probe_list")]
pub struct ProbeDeeperSnippetList {
    #[serde(rename = "$value")]
    snippets: Vec<ProbeDeeperSnippet>,
}

impl ProbeDeeperSnippetList {
    pub fn get_snippets(&self) -> &[ProbeDeeperSnippet] {
        self.snippets.as_slice()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename = "tool_use")]
pub enum ProbeEnoughOrDeeperResponse {
    #[serde(rename = "answer_user_query")]
    AnswerUserQuery(String),
    #[serde(rename = "probe_deeper")]
    ProbeDeeper(ProbeDeeperSnippetList),
}

impl ProbeEnoughOrDeeperResponse {
    fn from_string(response: &str) -> Result<ProbeEnoughOrDeeperResponse, ToolError> {
        let tags_to_search = vec!["<tool_use>", "</tool_use>"];
        if tags_to_search
            .into_iter()
            .any(|tag| !response.contains(&tag))
        {
            Err(ToolError::MissingXMLTags)
        } else {
            // we have to parse the sections behind the <tool_use> tags
            let tool_use = response
                .lines()
                .into_iter()
                .skip_while(|line| !line.contains("<tool_use>"))
                .skip(1)
                .take_while(|line| !line.contains("</tool_use>"))
                .collect::<Vec<&str>>()
                .join("\n");
            quick_xml::de::from_str::<ProbeEnoughOrDeeperResponse>(&tool_use).map_err(|e| {
                println!("{:?}", &e);
                ToolError::SerdeConversionFailed
            })
        }
    }
}

impl ProbeEnoughOrDeeperRequest {
    pub fn new(
        symbol_name: String,
        xml_string: String,
        query: String,
        llm_properties: LLMProperties,
        root_request_id: String,
    ) -> Self {
        Self {
            symbol_name,
            xml_string,
            query,
            llm_properties,
            root_request_id,
        }
    }
}

pub struct ProbeEnoughOrDeeper {
    llm_client: Arc<LLMBroker>,
    fallback_llm: LLMProperties,
}

impl ProbeEnoughOrDeeper {
    pub fn new(llm_client: Arc<LLMBroker>, fallback_llm: LLMProperties) -> Self {
        Self {
            llm_client,
            fallback_llm,
        }
    }

    fn example_message(&self) -> String {
        format!(
            r#"Some possible formats for <tool_use> output are given below:
- Example 1 : we can answer the user query
<tool_use>
<answer_user_query>
{{Answering the user query with your own thoughts}}
</answer_user_query>
</too_use>

- Example 2: We want to probe deeper into some symbols
<tool_use>
<probe_deeper>
<code_to_probe>
<id>0</id>
<reason_to_probe>
{{Your reason for deeply understanding and asking more questions to this code snippet}}
</reason_to_probe>
</code_to_probe>
</probe_deeper>
</tool_use>

- Example 3: We want to probe deeper into 2 such code snippets
<tool_use>
<probe_deeper>
<code_to_probe>
<id>0</id>
<reason_to_probe>
{{Your reason for deeply understanding and asking more questions to this code snippet}}
</reason_to_probe>
</code_to_probe>
<code_to_probe>
<id>1</id>
<reason_to_probe>
{{Your reason for deeply understanding and asking more questions to this code snippet}}
</reason_to_probe>
</code_to_probe>
</probe_deeper>
</tool_use>

Notice when using <probe_deeper> tool I always make sure to properly close the XML tags.

- WRONG Example 4 to AVOID: An example of WRONG format to use for using the <probe_deeper> tool
<tool_use>
<probe_deeper>
<code_to_probe_list>
<code_to_probe>
<id>0</id>
<reason_to_probe>
{{Your reason for probing this}}
</reason_to_probe>
</code_to_probe_list>
</probe_deeper>
</tool_use>
This is wrong because we are missing the </code_to_probe> ending tag, please be careful when replying in the XML format."#
        )
    }

    fn system_message(&self, symbol_name: &str) -> String {
        let example_message = self.example_message();
        format!(
            r#"You are an expert software engineer. Another software engineer has reached to the current symbol following code in the editor. You have to decide if we have enough information to answer the user query or we need to spend more time looking at particular sections of the code.
- We have reached {symbol_name} and you will be shown all the code snippets which belong to {symbol_name}.
- The code snippets which belong to the {symbol_name} will be provided to you in <code_snippet> question.
- Each <code_snippet> has an <id> which contains the id of the snippet and the <content> section which has the content for the snippet.
- You have to choose one of the following 2 tools:
<answer_user_query>
<tool_data>
- This allows you to answer the user query without going any deeper into the codebase
- Using <answer_user_query> implies you have all the information required to answer the user query and can reply with the answer.
- To use <answer_user_query> you have to output it in the following fashion:
<tool_data>
<answer_user_query>
{{your answer over here}}
</answer_user_query>
</tool_data>
The other tool which you have is:
<probe_deeper>
<too_data>
- You choose this when you believe we need to more deeply understand a section of the code.
- The code snippet which you select will be passed to another software engineer who is going to use it and deeply understand it to help answer the user query.
- The code snippet which you select might also have code symbols (variables, classes, function calls etc) inside it which we can click and follow to understand and gather more information, remember this when selecting the code snippets.
- You have to order the code snippets in the order of important, and only include the sections which will be part of the additional understanding or contain the answer to the user query, pug these code symbols in the <code_to_probe_list>
- To use <probe_deeper> tool you need to reply in the following fashion (assuming you are probing the code snippet with id 0):
<code_to_probe>
<id>
0
</id>
<reason_to_probe>
{{your reason for probing}}
</reason_to_probe>
</code_to_probe>

- If you want to edit more code sections follow the similar pattern as described above and as an example again:
<code_to_probe>
<id>
{{id of the code snippet you are interested in}}
</id>
<reason_to_probe>
{{your reason for probing or understanding this section more deeply and the details on what you want to understand}}
</reason_to_probe>
</code_to_probe>
{{... more code sections here which you might want to select}}
- The <id> section should ONLY contain an id from the listed code snippets.
</tool_data>

We are showing you an example of how to answer given a certain condition:

{example_message}

This example is for reference. You must strictly follow the format shown for the tool usage and reply your too use contained in the <tool_use> section."#
        )
    }

    fn user_message(&self, input: ProbeEnoughOrDeeperRequest) -> String {
        let user_query = &input.query;
        let xml_symbol = &input.xml_string;
        let symbol_name = &input.symbol_name;
        format!(
            r#"<user_query>
{user_query}
</user_query>

<symbol_name>
{symbol_name}
</symbol_name>
<code_snippet>
{xml_symbol}
</code_snippet>"#
        )
    }
}

#[async_trait]
impl Tool for ProbeEnoughOrDeeper {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.get_probe_enough_or_deeper()?;
        let root_request_id = context.root_request_id.to_owned();
        let symbol_name = context.symbol_name.to_owned();
        let llm_properties = context.llm_properties.clone();
        let llm_request = LLMClientCompletionRequest::new(
            context.llm_properties.llm().clone(),
            vec![
                LLMClientMessage::system(self.system_message(&symbol_name)),
                LLMClientMessage::user(self.user_message(context)),
            ],
            0.2,
            None,
        );
        // switches to gemini on odd
        let mut retries = 1;
        loop {
            if retries > 4 {
                return Err(ToolError::CodeSymbolError(
                    CodeSymbolError::ExhaustedRetries,
                ));
            }
            let mut provider = llm_properties.provider().clone();
            let mut api_key = llm_properties.api_key().clone();
            let mut llm_request_cloned = llm_request.clone();
            if retries % 2 == 0 {
                llm_request_cloned = llm_request_cloned.set_llm(llm_properties.llm().clone());
            } else {
                llm_request_cloned = llm_request_cloned.set_llm(self.fallback_llm.llm().clone());
                provider = self.fallback_llm.provider().clone();
                api_key = self.fallback_llm.api_key().clone();
            }
            if retries != 0 {
                jitter_sleep(10.0, retries as f64).await;
            }
            retries = retries + 1;
            let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
            let response = self
                .llm_client
                .stream_completion(
                    api_key.clone(),
                    llm_request_cloned.clone(),
                    provider.clone(),
                    vec![
                        ("event_type".to_owned(), "probe_enough_or_deeper".to_owned()),
                        ("root_id".to_owned(), root_request_id.to_owned()),
                    ]
                    .into_iter()
                    .collect(),
                    sender,
                )
                .await
                .map_err(|e| ToolError::CodeSymbolError(CodeSymbolError::LLMClientError(e)))?;
            let parsed_response =
                ProbeEnoughOrDeeperResponse::from_string(response.answer_up_until_now());
            match parsed_response {
                Ok(response) => return Ok(ToolOutput::ProbeEnoughOrDeeper(response)),
                Err(_) => continue,
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

#[cfg(test)]
mod tests {
    use super::ProbeEnoughOrDeeperResponse;

    #[test]
    fn test_parsing_answer_query() {
        let response = r#"something something else blah blah
<tool_use>
<answer_user_query>
Answer me something
</answer_user_query>
</tool_use>"#
            .to_owned();
        let output = ProbeEnoughOrDeeperResponse::from_string(&response);
        assert!(output.is_ok());
    }

    #[test]
    fn test_parsing_code_snippets_to_follow() {
        let response = r#"something something else
<tool_use>
<probe_deeper>
<code_snippet>
<id>
0
</id>
<reason_to_probe>
something else over here
</reason_to_probe>
</code_snippet>
<code_snippet>
<id>
1
</id>
<reason_to_probe>something else over here</reason_to_probe>
</code_snippet>
</probe_deeper>
</tool_use>"#;
        let output = ProbeEnoughOrDeeperResponse::from_string(&response);
        assert!(output.is_ok());
    }

    #[test]
    fn test_parsing_code_snippets_to_follow_example() {
        let response = r#"<tool_use>
        <probe_deeper>
        <code_to_probe>
        <id>0</id>
        <reason_to_probe>
        The code snippet shows the routes defined for the agent, but does not provide any information on how the agent communicates with the LLM. To understand that, we likely need to look at the implementation details of the functions like search_agent, hybrid_search, explain, and followup_chat.
        </reason_to_probe>
        </code_to_probe>
        </probe_deeper>
        </tool_use>"#;
        let output = ProbeEnoughOrDeeperResponse::from_string(&response);
        assert!(output.is_ok())
    }

    #[test]
    fn test_parsing_gemini_output_to_follow_example() {
        let response = r#"
        <tool_use>
        <probe_deeper>
        <code_to_probe>
        <id>
        0
        </id>
        <reason_to_probe>
        The agent router defines routes for different agent functionalities, including `/search_agent`, `/hybrid_search`, `/explain`, and `/followup_chat`.  I need to understand what each of these routes does, specifically looking for any interaction with an LLM.  For example, does `sidecar::webserver::agent::search_agent` call an LLM?
        </reason_to_probe>
        </code_to_probe>
        </probe_deeper>
        </tool_use>
        "#;
        let output = ProbeEnoughOrDeeperResponse::from_string(&response);
        assert!(output.is_ok());
    }
}
