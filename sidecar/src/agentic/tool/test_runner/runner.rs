use crate::agentic::tool::{errors::ToolError, input::ToolInput, output::ToolOutput, r#type::Tool};
use async_trait::async_trait;

pub struct TestRunner;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestRunnerRequest {
    fs_file_paths: Vec<String>,
    editor_url: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestRunnerResponse {
    test_output: String,
    exit_code: i32,
}

impl TestRunnerResponse {
    pub fn test_output(&self) -> &str {
        &self.test_output
    }

    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }
}

impl TestRunnerRequest {
    pub fn new(fs_file_paths: Vec<String>, editor_url: String) -> Self {
        Self {
            fs_file_paths,
            editor_url,
        }
    }
}

#[async_trait]
impl Tool for TestRunner {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let request = input.is_test_runner()?;

        let editor_endpoint = request.editor_url.to_owned() + "/run_tests";
        println!("{:?}", editor_endpoint);

        // TODO: move reqwest client creation to new, this is expensive to create
        // all the time
        let client = reqwest::Client::new();
        let response = client
            .post(editor_endpoint)
            .body(serde_json::to_string(&request).map_err(|_e| ToolError::SerdeConversionFailed)?)
            .send()
            .await
            .map_err(|e| ToolError::LLMClientError(e.into()))?;

        let output: TestRunnerResponse = response
            .json()
            .await
            .map_err(|e| ToolError::LLMClientError(e.into()))?;

        println!("run_tests output: {:?}", output);

        Ok(ToolOutput::TestRunner(output))
    }

    fn tool_description(&self) -> String {
        r#"### test_runner
Runs the tests in the provided files"#
            .to_owned()
    }

    fn tool_input_format(&self) -> String {
        r#"Parameters:
- fs_file_paths: (required) A list of file paths to run tests for, separated by newlines
Usage:
<test_runner>
<fs_file_paths>
path/to/file1.py
path/to/file2.py
</fs_file_paths>
</test_runner>"#
            .to_owned()
    }
}
