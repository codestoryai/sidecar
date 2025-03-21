//! Contains the required logic for supporting test correction logic through
//! code editing

use async_trait::async_trait;
use std::sync::Arc;

use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage, LLMType},
    provider::{LLMProvider, LLMProviderAPIKeys},
};

use crate::agentic::tool::{
    errors::ToolError,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale},
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct TestOutputCorrectionRequest {
    fs_file_path: String,
    file_contents: String,
    user_instructions: String,
    code_above: Option<String>,
    code_below: Option<String>,
    code_in_selection: String,
    original_code: String,
    language: String,
    test_output_logs: String,
    llm: LLMType,
    provider: LLMProvider,
    api_keys: LLMProviderAPIKeys,
    extra_code_context: String,
    root_request_id: String,
}

impl TestOutputCorrectionRequest {
    pub fn new(
        fs_file_path: String,
        file_contents: String,
        user_instructions: String,
        code_above: Option<String>,
        code_below: Option<String>,
        code_in_selection: String,
        original_code: String,
        language: String,
        test_output_logs: String,
        llm: LLMType,
        provider: LLMProvider,
        api_keys: LLMProviderAPIKeys,
        extra_code_context: String,
        root_request_id: String,
    ) -> Self {
        Self {
            fs_file_path,
            file_contents,
            user_instructions,
            code_above,
            code_below,
            code_in_selection,
            original_code,
            language,
            test_output_logs,
            llm,
            provider,
            api_keys,
            extra_code_context,
            root_request_id,
        }
    }

    pub fn root_request_id(&self) -> &str {
        &self.root_request_id
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestOuptutCorrectionResponse {
    thinking: String,
    corrected_code: String,
}

impl TestOuptutCorrectionResponse {
    fn parse_reponse(response: &str) -> Result<Self, ToolError> {
        // The output we get here has the thinking in <thinking> and then
        // the corrected code in <code_corrected> section.
        // Another check here is to figure out if we have the <thinking> and the <code_corrected> tags
        // all present
        let tags_to_check = vec![
            "<thinking>",
            "</thinking>",
            "<corrected_code>",
            "</corrected_code>",
        ];
        if tags_to_check.into_iter().any(|tag| !response.contains(tag)) {
            return Err(ToolError::MissingXMLTags);
        }
        let thinking = response
            .lines()
            .into_iter()
            .skip_while(|line| !line.contains("<thinking>"))
            .skip(1)
            .take_while(|line| !line.contains("</thinking>"))
            .collect::<Vec<&str>>()
            .join("\n");
        // The corrected code is often contained inside backticks so we want
        // to remove the first and last line since those are backticks
        let corrected_code = response
            .lines()
            .into_iter()
            .skip_while(|line| !line.contains("<corrected_code>"))
            .skip(1)
            .take_while(|line| !line.contains("</corrected_code>"))
            .collect::<Vec<&str>>()
            .into_iter()
            .skip_while(|line| !line.contains("```"))
            .skip(1)
            .take_while(|line| !line.contains("```"))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(Self {
            thinking,
            corrected_code,
        })
    }
}

pub struct TestCorrection {
    llm_client: Arc<LLMBroker>,
}

impl TestCorrection {
    pub fn new(llm_client: Arc<LLMBroker>) -> Self {
        Self { llm_client }
    }

    fn system_message(&self) -> String {
        format!("You are an expert software engineer who is tasked with fixing broken written written by a junior engineer.
`- All the definitions of code symbols which you might require are also provided to you in <extra_data> section, these are important as they show which functions or parameters you can use on different classes.
- The junior engineer has taken the instructions which were provided in <user_instructions> and made edits to the code which is now present in <code_in_selection> section.
- The original code before any changes were made is present in <original_code> , this should help you understand how the junior engineer went about making changes.
- Make sure that the indentation of the code is same the code which is in <code_in_selection> since we are working with langauges where indentation really matters, use spaces or tabs as required.
- You are also shown the whole file content in the <file> section, this will be useful for you to understand the overall context in which the change was made.
- The user has also noticed some test failures with the modified code which is present in <code_in_selection> and the output of the test failures is given in <test_output> section.
- You have to rewrite the code which is present only in <code_in_selection> making sure that the test failures which were present in <test_output> section no longer happen because of the code which is present in <code_in_selection>
- Make sure to rewrite the whole code present in <code_in_selection> without leaving any comments or using place-holders.
- The user will use the code which you generated directly without looking at it or taking care of any additional comments, so make sure that the code is complete.
- Even if the code outside the <code_in_selection> requires changes to fix the tests, you SHOULD NOT EDIT any code outside the <code_in_selection> section.
- Your reply should be in 2 parts, one of them is the <thinking> section where you explain what changes you are going to make and then the <corrected_code> section where you write the corrected code.

An example is shown below to you:

<user_instruction>
We want to be able to subtract 4 numbers instead of 2
</user_instruction>

<file>
<file_path>
testing/maths.py
</file_path>
<code_above>
```python
class Maths:
    def add(a, b):
        return a + b
```
</code_above>
<code_below>
```python
    def multiply(a, b):
        return a * b
```
</code_below>
<code_in_selection>
```python
    def subtract(a, b, c, d):
        return a - b - c + d
</code_in_selection>
</file>

<original_code>
```python
    def subtract(a, b):
        return a - b
```
</original_code>

<test_output>
test_subtract_new failed with:
subtract(1, 2, 3, 4) != -8
</test_output>

Your reply is:
<thinking>
The output for the test case subtract(1, 2, 3, 4) is wrong because we are not subtracting the last part in the `subtract` function since it is `a - b - c + d` instead it should be `a - b - c - d`
</thinking>
<corrected_code>
```python
    def subtract(a, b, c, d):
        return a - b - c - d
```
</corrected_code>").to_owned()
    }

    fn user_message(&self, request: TestOutputCorrectionRequest) -> String {
        let user_query = &request.user_instructions;
        let file_path = request.fs_file_path;
        let code_above = request.code_above.unwrap_or("".to_owned());
        let code_below = request.code_below.unwrap_or("".to_owned());
        let code_in_selection = request.code_in_selection;
        let original_code = request.original_code;
        let language = request.language;
        let test_output = request.test_output_logs;
        format!(
            r#"<user_query>
{user_query}
</user_query>

<file>
<file_path>
{file_path}
</file_path>
<code_above>
```{language}
{code_above}
```
</code_above>
<code_below>
```{language}
{code_below}
```
</code_below>
<code_in_selection>
```{language}
{code_in_selection}
```
</code_in_selection>
</file>


<original_code>
```{language}
{original_code}
```
</original_code>

<test_output>
{test_output}
</test_output>"#
        )
        .to_owned()
    }
}

#[async_trait]
impl Tool for TestCorrection {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_test_output()?;
        let root_id = context.root_request_id().to_owned();
        let llm = context.llm.clone();
        let provider = context.provider.clone();
        let api_keys = context.api_keys.clone();
        let system_message = LLMClientMessage::system(self.system_message());
        let user_message = LLMClientMessage::user(self.user_message(context));
        let request =
            LLMClientCompletionRequest::new(llm, vec![system_message, user_message], 0.2, None);
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let response = self
            .llm_client
            .stream_completion(
                api_keys,
                request,
                provider,
                vec![
                    ("event_type".to_owned(), "tool_correction".to_owned()),
                    ("root_id".to_owned(), root_id.to_owned()),
                ]
                .into_iter()
                .collect(),
                sender,
            )
            .await
            .map_err(|e| ToolError::LLMClientError(e))?;
        let output = TestOuptutCorrectionResponse::parse_reponse(response.answer_up_until_now())?;
        Ok(ToolOutput::TestCorrectionOutput(output.corrected_code))
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
