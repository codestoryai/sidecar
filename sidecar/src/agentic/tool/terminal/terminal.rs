use async_trait::async_trait;

use crate::agentic::tool::{
    errors::ToolError,
    input::ToolInput,
    output::ToolOutput,
    r#type::{Tool, ToolRewardScale},
};

pub struct TerminalTool {
    client: reqwest::Client,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TerminalInputPartial {
    command: String,
}

impl TerminalInputPartial {
    pub fn new(command: String) -> Self {
        Self { command }
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn sanitise_for_repro_script(self) -> Self {
        if self.command.contains("reproduce_error.py") && self.command.contains("python") {
            Self {
                command: "python reproduce_error.py".to_owned(),
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
</execute_command>"#,
            self.command
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
}

impl TerminalInput {
    pub fn new(command: String, editor_url: String) -> Self {
        Self {
            command,
            editor_url,
        }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TerminalOutput {
    output: String,
}

impl TerminalOutput {
    pub fn output(&self) -> &str {
        &self.output
    }
}

impl TerminalTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
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

        Ok(ToolOutput::TerminalCommand(terminal_response))
    }

    // credit Cline.
    // Current working directory will be known to LLM from higher level context
    fn tool_description(&self) -> String {
        format!(
            r#"### execute_command
Request to execute a CLI command on the system.
Use this when you need to perform system operations or run specific commands to accomplish any step in the user's task.
You must tailor your command to the user's system and provide a clear explanation of what the command does.
Prefer to execute complex CLI commands over creating executable scripts, as they are more flexible and easier to run.
Commands will be executed in the current working directory.
Note: You MUST append a `sleep 0.05` to the end of the command for commands that will complete in under 50ms, as this will circumvent a known issue with the terminal tool where it will sometimes not return the output when the command completes too quickly."#
        )
    }

    fn tool_input_format(&self) -> String {
        format!(
            r#"Parameters:
- command: (required) The CLI command to execute. This should be valid for the current operating system. Ensure the command is properly formatted and does not contain any harmful instructions.

Usage:
<execute_command>
<command>
Your command here
</command>
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
