use crate::agentic::tool::{
    errors::ToolError,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale},
};
use async_trait::async_trait;

pub struct TestRunner;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestRunnerRequest {
    fs_file_paths: Vec<String>,
    editor_url: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestRunnerRequestPartial {
    fs_file_paths: Vec<String>,
}

impl TestRunnerRequestPartial {
    pub fn new(file_paths: Vec<String>) -> Self {
        Self {
            fs_file_paths: file_paths,
        }
    }

    pub fn fs_file_paths(self) -> Vec<String> {
        self.fs_file_paths.to_vec()
    }

    pub fn to_string(&self) -> String {
        self.fs_file_paths.to_vec().join(" ,")
    }

    pub fn to_json() -> serde_json::Value {
        serde_json::json!({
            "name": "test_runner",
            "description": r#"Runs the tests in the provided files. You can run the tests to make sure that there are no regressions with the changes you have made.

For example say you want to run `test_function` in src/test_file.py
Then you should provide `src/test_file.py` as the input (notice how only the file name is provided) and NOT `src/test_file.py::test_function`
You are only allowed to run all the tests in a test file and not a single test in the file. So do not pass the test you are interested in running in the test file as input.
# Requirements:
You should verify where the test files are located, only use test_runner tool after you have this information"#,
            "input_schema": {
                "type": "object",
                "properties": {
                    "fs_file_paths": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        },
                        "description": "(required) A list of file paths to run tests for, separated by newlines",
                    },
                },
                "required": ["fs_file_paths"],
            },
        })
    }
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

        Ok(ToolOutput::TestRunner(output))
    }

    fn tool_description(&self) -> String {
        r#"### test_runner
Runs the tests in the provided files. You can run the tests to make sure that there are no regressions with the changes you have made.

#### Requirements:
You should verify where the test files are located, only use test_runner tool after you have this information"#
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

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![
            "Test Result Evaluation: Analyze test results in conjunction with the proposed code changes.",
            "Test Failures Categorization: Differentiate between minor, foreseeable, and unforeseeable failures.",
            " * Minor, Easily Fixable Failures: Lightly penalize or treat as neutral.",
            " * Foreseeable Failures: Penalize appropriately based on the complexity of the fix.",
            " * Unforeseeable Failures: Penalize very lightly or reward for providing new insights.",
            " * Backward Compatibility issues: Understand the context in which test cases are failing, if backward compatibility is required then its a big failure, if that is not really the case then the failures can be ignored",
            "Impact of Failures: Consider the overall impact of test failures on the solution's viability.",
            "Iterative Improvement: Encourage fixing minor issues in subsequent iterations.",
            "Explanation Requirement: In your explanation, describe any test failures, their likely causes, and suggest potential next steps.",
        ].into_iter().map(|evaluation_criteria| evaluation_criteria.to_owned()).collect()
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![
            ToolRewardScale::new(
                90,
                100,
                "All tests pass successfully, confirming the solution's correctness.",
            ),
            ToolRewardScale::new(
                75,
                89,
                "Most tests pass, with minor, easily fixable failures.",
            ),
            ToolRewardScale::new(
                50,
                74,
                // TODO(skcd): Added more instructions to check the test output more clearly.
                "Tests have some failures, but they are minor or unforeseeable, and the agent shows understanding in interpreting results. The test failures do not cause any regressions to the user task.",
            ),
            ToolRewardScale::new(
                25,
                49,
                "Tests have noticeable failures; some may have been foreseeable, but the agent can address them with effort.",
            ),
            ToolRewardScale::new(
                0,
                24,
                "Tests have significant failures; the agent's interpretation is minimal or incorrect.",
            ),
            ToolRewardScale::new(
                -49,
                -1,
                "Tests fail significantly; the agent misinterprets results or shows lack of progress, foreseeable failures are not addressed.",
            ),
            ToolRewardScale::new(
                -100,
                -50,
                "The action is counterproductive, demonstrating misunderstanding or causing setbacks, test failures are severe and could have been anticipated.",
            ),
        ]
    }
}
