use async_trait::async_trait;
use logging::new_client;

use crate::agentic::tool::{
    errors::ToolError,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale},
};

pub struct TerminalTool {
    client: reqwest_middleware::ClientWithMiddleware,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TerminalInputPartial {
    command: String,
    #[serde(default)]
    wait_for_exit: bool,
}

impl TerminalInputPartial {
    pub fn new(command: String, wait_for_exit: bool) -> Self {
        Self {
            command,
            wait_for_exit,
        }
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn wait_for_exit(&self) -> bool {
        self.wait_for_exit
    }

    pub fn sanitise_for_repro_script(self) -> Self {
        if self.command.contains("reproduce_error.py") && self.command.contains("python") {
            Self {
                command: "python reproduce_error.py".to_owned(),
                wait_for_exit: self.wait_for_exit,
            }
        } else {
            self
        }
    }

    pub fn to_string(&self) -> String {
        format!(
            r#"<execute_command>
<command>
{}
</command>
<wait_for_exit>
{}
</wait_for_exit>
</execute_command>"#,
            self.command, self.wait_for_exit
        )
    }

    pub fn to_json() -> serde_json::Value {
        serde_json::json!({
            "name": "execute_command",
            "description": r#"Request to execute a CLI command on the system. Commands will be executed in the current working directory."#,
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "(required) The CLI command to execute. This should be valid for the current operating system. Ensure the command is properly formatted and does not contain any harmful instructions.",
                    },
                    "wait_for_exit": {
                        "type": "boolean",
                        "description": "(optional) Whether to wait for the command to exit before proceeding. Set to false for long-running commands like servers. Defaults to true.",
                    }
                },
                "required": ["command"],
            },
        })
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct TerminalInput {
    command: String,
    editor_url: String,
    #[serde(default)]
    wait_for_exit: bool,
}

impl TerminalInput {
    pub fn new(command: String, editor_url: String, wait_for_exit: bool) -> Self {
        Self {
            command,
            editor_url,
            wait_for_exit,
        }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TerminalOutput {
    output: String,
}

impl TerminalOutput {
    pub fn new(output: String) -> Self {
        // Collect all lines
        let lines: Vec<_> = output.lines().collect();
        let total_lines = lines.len();

        // Take last 3000 lines
        let limited_lines: Vec<_> = lines.iter().rev().take(3000).rev().cloned().collect();

        // Add truncation prefix if needed
        let limited_output = if total_lines > 3000 {
            format!(
                "... truncated {} lines\n{}",
                total_lines - 3000,
                limited_lines.join("\n")
            )
        } else {
            limited_lines.join("\n")
        };

        Self {
            output: limited_output,
        }
    }

    pub fn output(&self) -> &str {
        &self.output
    }
}

impl TerminalTool {
    pub fn new() -> Self {
        Self {
            client: new_client(),
        }
    }
}

#[async_trait]
impl Tool for TerminalTool {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.is_terminal_command()?;
        let editor_endpoint = context.editor_url.to_owned() + "/execute_terminal_command";

        let response = self
            .client
            .post(editor_endpoint)
            .body(serde_json::to_string(&context).map_err(|_e| ToolError::SerdeConversionFailed)?)
            .send()
            .await
            .map_err(|_e| ToolError::ErrorCommunicatingWithEditor)?;

        let terminal_response: TerminalOutput = response
            .json()
            .await
            .map_err(|_e| ToolError::SerdeConversionFailed)?;

        // Apply line limiting after JSON deserialization
        Ok(ToolOutput::TerminalCommand(TerminalOutput::new(
            terminal_response.output,
        )))
    }

    // credit Cline.
    // Current working directory will be known to LLM from higher level context
    fn tool_description(&self) -> String {
        format!(
            r#"### execute_command
Request to execute a shell or CLI command on the system.
Use this when you need to perform system operations or run specific commands to accomplish any step in the user's task.
You must tailor your command to the user's system and provide a clear explanation of what the command does.
Prefer to execute complex shell commands over creating executable scripts, as they are more flexible and easier to run.
Commands will be executed in the current working directory.

For long-running commands like servers (e.g., 'npm run dev', 'python -m http.server'), set wait_for_exit to false.
This allows the command to continue running while proceeding with subsequent steps.

Note: You MUST append a `sleep 0.05` to the end of the command for commands that will complete in under 50ms, as this will circumvent a known issue with the terminal tool where it will sometimes not return the output when the command completes too quickly."#
        )
    }

    fn tool_input_format(&self) -> String {
        format!(
            r#"Parameters:
- command: (required) The shell or CLI command to execute. This should be valid for the current operating system. Ensure the command is properly formatted and does not contain any harmful instructions.
- wait_for_exit: (optional) Set to false for long-running commands like servers that shouldn't block execution. Defaults to true.

Usage:
<execute_command>
<command>
Your command here
</command>
<wait_for_exit>
true
</wait_for_exit>
</execute_command>"#
        )
    }

    fn get_evaluation_criteria(&self, trajectory_length: usize) -> Vec<String> {
        let evaluation_criteria = if trajectory_length < 3 {
            vec![
                "Exploratory Actions: Recognize that initial searches and information-gathering steps are essential and should not be heavily penalized if they don't yield immediate results.",
                "Appropriateness of Action: Evaluate if the action is logical given the agent's current knowledge and the early stage of problem-solving.",
            ]
        } else {
            vec![
                "Solution Quality: Assess the logical changes, contextual fit, and overall improvement without introducing new issues.",
                "Progress Assessment: Evaluate the agent's awareness of solution history, detection of repetitive actions, and planned next steps.",
                "Repetitive or Redundant Actions: Detect if the agent is repeating the same unsuccessful or redundant actions without making progress. Pay close attention to the agent's history and outputs indicating lack of progress.",
            ]
        };
        evaluation_criteria
            .into_iter()
            .map(|evaluation_criteria| evaluation_criteria.to_owned())
            .collect()
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![
            ToolRewardScale::new(
                75,
                100,
                "The action significantly advances the solution.",
            ),
            ToolRewardScale::new(
                50,
                74,
                "The action contributes positively towards solving the problem.",
            ),
            ToolRewardScale::new(
                25,
                49,
                "The action is acceptable but may have some issues.",
            ),
            ToolRewardScale::new(
                0,
                24,
                "The action has minimal impact or minor negative consequences.",
            ),
            ToolRewardScale::new(
                -49,
                -1,
                "The code change is inappropriate, unhelpful, introduces new issues, or redundantly repeats previous changes without making further progress. The Git diff does not align with instructions or is unnecessary.",
            ),
            ToolRewardScale::new(
                -100,
                -50,
                "The code change is counterproductive, causing significant setbacks or demonstrating persistent repetition without learning. The agent fails to recognize completed tasks and continues to attempt redundant actions.",
            ),
        ]
    }
}
