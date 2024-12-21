//! Attemps to complete the session and reply to the user with the required output

use async_trait::async_trait;

use crate::agentic::tool::{
    errors::ToolError,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale},
};

pub struct AttemptCompletionClient {}

impl AttemptCompletionClient {
    pub fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AttemptCompletionClientRequest {
    result: String,
    command: Option<String>,
}

impl AttemptCompletionClientRequest {
    pub fn new(result: String, command: Option<String>) -> Self {
        Self { result, command }
    }

    pub fn result(&self) -> &str {
        &self.result
    }

    pub fn command(&self) -> Option<String> {
        self.command.clone()
    }

    pub fn to_string(&self) -> String {
        format!(
            r#"<thinking>
...
</thinking>
<attempt_completion>
<result>
{}
</result>
<command>
{}
</command>
</attempt_completion>"#,
            self.result,
            self.command
                .clone()
                .unwrap_or("no command provided".to_owned())
        )
    }

    pub fn to_json() -> serde_json::Value {
        serde_json::json!({
            "name": "attempt_completion",
            "description": r#"Use this when you have resolved the Github Issue and solved it completely or you have enough evidence to suggest that the Github Issue has been resolved after your changes."#,
            "input_schema": {
                "type": "object",
                "properties": {
                    "result": {
                        "type": "string",
                        "description": "(required) The result of the task. Formulate this result in a way that is final and does not require further input from the user. Don't end your result with questions or offers for further assistance.",
                    }
                },
                "required": ["result"],
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct AttemptCompletionClientResponse {
    result: String,
    command: Option<String>,
}

impl AttemptCompletionClientResponse {
    pub fn new(result: String, command: Option<String>) -> Self {
        Self { result, command }
    }

    pub fn reply(&self) -> &str {
        &self.result
    }

    pub fn command(&self) -> Option<String> {
        self.command.clone()
    }
}

#[async_trait]
impl Tool for AttemptCompletionClient {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_attempt_completion()?;
        let request = context.result.to_owned();
        let command = context.command.clone();
        Ok(ToolOutput::AttemptCompletion(
            AttemptCompletionClientResponse::new(request, command),
        ))
    }

    fn tool_description(&self) -> String {
        r#"### attempt_completion
After each tool use, the user will respond with the result of that tool use, i.e. if it succeeded or failed, along with any reasons for failure. Once you've received the results of tool uses and can confirm that the task is complete, use this tool to present the result of your work to the user. Optionally you may provide a CLI command to showcase the result of your work. The user may respond with feedback if they are not satisfied with the result, which you can use to make improvements and try again."#.to_owned()
    }

    fn tool_input_format(&self) -> String {
        r#"Parameters:
- result: (required) The result of the task. Formulate this result in a way that is final and does not require further input from the user. Don't end your result with questions or offers for further assistance.
- command: (optional) A CLI command to execute to show a live demo of the result to the user. For example, use \`open index.html\` to display a created html website, or \`open localhost:3000\` to display a locally running development server. But DO NOT use commands like \`echo\` or \`cat\` that merely print text. This command should be valid for the current operating system. Ensure the command is properly formatted and does not contain any harmful instructions.
Usage:
<attempt_completion>
<result>
Your final result description here
</result>
<command>
Command to demonstrate result (optional)
</command>
</attempt_completion>"#.to_owned()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![
            "**Full Trajectory Review:** Evaluate the complete sequence of actions taken by the agent leading to this finish action. Assess whether the trajectory represents an efficient and logical path to the solution.",
            "**Solution Correctness and Quality:** Verify that all changes made throughout the trajectory logically address the problem statement. Ensure the changes fit contextually within the existing codebase without introducing new issues. Confirm syntactic correctness and that there are no syntax errors or typos.",
            "**Testing Requirements (Critical):**",
            " * **Mandatory Test Updates:** The trajectory MUST include actions that either update existing tests or add new tests to verify the solution. A score of 75 or higher CANNOT be given without proper test coverage.",
            " * **Test Coverage Quality:** Evaluate whether the tests added or modified throughout the trajectory adequately cover the changes, including edge cases and error conditions.",
            " * **Test Execution Results:** Verify that all tests are passing after the complete sequence of changes.",
            "**Assessment of Complete Trajectory:** Evaluate if the sequence of actions taken represents the most optimal path to the solution, or if unnecessary steps were taken.",
            "**Verification of Task Completion:** Confirm that all aspects of the original issue have been addressed through the sequence of actions, including implementation, testing, and documentation where applicable.",
        ].into_iter().map(|evaluation_criteria| evaluation_criteria.to_owned()).collect()
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![
            ToolRewardScale::new(
                90,
                100,
                "The complete trajectory perfectly resolves the issue with optimal code modifications AND includes comprehensive test updates/additions. All tests pass and cover all relevant scenarios. No further improvements needed.",
            ),
            ToolRewardScale::new(
                75,
                89,
                "The trajectory successfully resolves the issue AND includes proper test updates/additions. All tests pass, though minor improvements to test coverage might be beneficial. REQUIRES test modifications to qualify for this range.",
            ),
            ToolRewardScale::new(
                50,
                74,
                "The trajectory resolves the core issue but has gaps in test coverage OR the solution path wasn't optimal. May include cases where implementation is correct but tests were not adequately updated.",
            ),
            ToolRewardScale::new(
                25,
                49,
                "The trajectory partially resolves the issue but lacks proper test coverage AND has other significant gaps such as incomplete implementation or inefficient solution path.",
            ),
            ToolRewardScale::new(
                0,
                24,
                "The trajectory shows some progress but fails to properly resolve the issue AND lacks necessary test updates. The finish action was premature.",
            ),
            ToolRewardScale::new(
                -49,
                -1,
                "The trajectory is inappropriate with major gaps in both implementation and testing. The finish action indicates a clear misunderstanding of the requirements.",
            ),
            ToolRewardScale::new(
                -100,
                -50,
                "The trajectory is entirely incorrect, potentially introducing new issues, and completely lacks test coverage. The finish action is entirely premature.",
            ),
        ]
    }
}
