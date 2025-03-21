use llm_client::clients::types::LLMClientMessage;
use serde::{Deserialize, Serialize};

use crate::{
    agentic::{
        symbol::events::message_event::SymbolEventMessageProperties,
        tool::{
            input::ToolInput,
            r#type::{Tool, ToolRewardScale, ToolType},
            reward::client::RewardGenerationRequest,
        },
    },
    mcts::action_node::{ActionNode, ActionToolParameters, SearchTree},
};

use super::error::RewardError;

/// The reward for execution on an action node and the value generated out of it
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reward {
    /// An explanation and the reasoning behind your decision.
    explanation: String,
    /// Feedback to the alternative branch.
    feedback: Option<String>,
    /// A single integer value between -100 and 100 based on your confidence in the correctness of the action and its likelihood of resolving the issue
    value: i32,
}

impl Reward {
    pub fn new(explanation: String, feedback: Option<String>, value: i32) -> Self {
        Self {
            explanation,
            feedback,
            value,
        }
    }

    pub fn with_explanation(explanation: String, value: i32) -> Self {
        Self {
            explanation,
            value,
            feedback: None,
        }
    }

    pub fn value(&self) -> i32 {
        self.value
    }

    pub fn feedback(&self) -> Option<String> {
        self.feedback.clone()
    }

    pub fn explanation(&self) -> &str {
        &self.explanation
    }
}

/// Generates the reward for the code and the trajectory
#[derive(Clone)]
pub struct RewardGeneration {}

impl RewardGeneration {
    pub fn new() -> Self {
        RewardGeneration {}
    }

    pub async fn generate_reward(
        &self,
        mut nodes_trajectory: Vec<&ActionNode>,
        search_tree: &SearchTree,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Reward, RewardError> {
        let tool_box = search_tree.tool_box();
        if nodes_trajectory.is_empty() {
            return Err(RewardError::EmptyTrajectory);
        }

        let leaf = nodes_trajectory.pop();
        if leaf.is_none() {
            return Err(RewardError::EmptyTrajectory);
        }
        let leaf = leaf.expect("is_none to hold");
        let root_to_leaf_direction = nodes_trajectory;

        if let Some(observation) = leaf.observation() {
            // we require a correction, no reward
            if observation.expect_correction() {
                return Ok(Reward::with_explanation(
                    "Expects a correction".to_owned(),
                    0,
                ));
            }
        }

        // check if the action was an error
        if let Some(ActionToolParameters::Errored(_)) = leaf.action() {
            return Ok(Reward::with_explanation(
                "Error action, assigning reward -100".to_owned(),
                -100,
            ));
        }

        // current message
        let current_message = match leaf.action() {
            Some(ActionToolParameters::Errored(_)) => {
                return Ok(Reward::with_explanation(
                    "Error action, assigning reward -100".to_owned(),
                    -100,
                ))
            }
            Some(ActionToolParameters::Tool(tool_input_partial)) => {
                let tool_input_partial = tool_input_partial.tool_input_partial();
                let tool_type = tool_input_partial.to_tool_type();
                match tool_type {
                    ToolType::AttemptCompletion => tool_input_partial.to_string(),
                    _ => {
                        format!(
                            r#"## Last Executed Action:
Here is the most recent action that was executed and its output. This is the subject of your evaluation.
<executed_action>
{}
</executed_action>

## Output:
{}"#,
                            tool_input_partial.to_string(),
                            leaf.observation()
                                .map(|observation| observation.message().to_owned())
                                .unwrap_or("No observation found.".to_owned())
                        )
                    }
                }
            }
            None => {
                return Ok(Reward::with_explanation(
                    "Error, no action assigning reward -100".to_owned(),
                    -100,
                ))
            }
        };

        // messages for the trajectory
        let messages =
            self.messages_for_reward(leaf, root_to_leaf_direction, current_message, search_tree)?;

        // invoke the tool to get the reward output
        let reward_output = tool_box
            .tools()
            .invoke(ToolInput::RewardGeneration(RewardGenerationRequest::new(
                messages,
                message_properties.clone(),
            )))
            .await
            .map_err(|e| RewardError::ToolError(e))?
            .get_reward_generation_response()
            .ok_or(RewardError::WrongTool)?;

        Ok(Reward::new(
            reward_output.explanation().to_owned(),
            reward_output.feedback(),
            reward_output.value(),
        ))
    }

    fn messages_for_reward(
        &self,
        leaf: &ActionNode,
        root_to_leaf: Vec<&ActionNode>,
        current_message: String,
        search_tree: &SearchTree,
    ) -> Result<Vec<LLMClientMessage>, RewardError> {
        let root_node = search_tree.root();
        if let None = root_node {
            return Err(RewardError::RootError);
        }
        let root_node = root_node.expect("if let None to hold");

        let problem_statement = root_node.message();
        if let None = problem_statement {
            return Err(RewardError::ProblemStatementNotFound);
        }
        let problem_statement = problem_statement.expect("if let None to hold");

        let root_to_leaf_len = root_to_leaf.len();

        let action_observations = root_to_leaf
            .into_iter()
            .enumerate()
            .map(|(idx, current_node)| {
                let action = current_node.action();
                match action {
                    Some(action) => {
                        let action_part =
                            format!(r#"## {} Action: {}"#, idx + 1, action.to_string());
                        let action_part = format!(
                            r#"{}
{}"#,
                            action_part,
                            action.to_string()
                        );

                        let action_observation = match current_node.observation() {
                            Some(observation) => {
                                if observation.summary().is_some() && idx < root_to_leaf_len - 1 {
                                    format!(
                                        r#"{action_part}
Observation: {}"#,
                                        observation.summary().unwrap_or_default()
                                    )
                                } else {
                                    format!(
                                        r#"{action_part}
Observation: {}"#,
                                        observation.message()
                                    )
                                }
                            }
                            None => {
                                format!(
                                    r#"{action_part}
Observation: No output found."#
                                )
                            }
                        };
                        action_observation
                    }
                    None => format!(r#"## {} No action taken at this stage"#, idx + 1),
                }
            })
            .collect::<Vec<String>>();

        let action_observations = format!(
            r#"{problem_statement}

Below is the history of previously executed actions and their observations.
<history>
{}
</history>
        "#,
            action_observations.join("\n")
        );

        // - Now we create the file content (it would be better if we can only keep track
        // of the interested code instead of the whole file)
        let file_content_vec = leaf
            .user_context()
            .variables
            .iter()
            .filter(|variable_information| variable_information.is_file())
            .map(|variable_information| variable_information.clone().to_xml())
            .collect::<Vec<_>>();

        let parent_node = search_tree.parent(leaf);

        let git_patch_diff = if let Some(parent_node) = parent_node {
            parent_node
                .user_context()
                .variables
                .iter()
                .filter(|variable_information| variable_information.is_file())
                .filter_map(|variable_information| {
                    let patch = variable_information.patch_from_root();
                    match patch {
                        Some(patch) => Some(format!(
                            r#"## Changes in {}
{}"#,
                            &variable_information.fs_file_path, patch
                        )),
                        None => None,
                    }
                })
                .collect::<Vec<_>>()
        } else {
            vec!["".to_owned()]
        };

        let current_message = format!(
            r#"{current_message}

The file context the agent has access to:
<files>
{}
</files>

The git diff of the changes until the last action:
{}"#,
            &file_content_vec.join("\n"),
            git_patch_diff.join("\n")
        );

        let system_message = self.system_message(leaf, search_tree)?;

        let format_reminder = r#"
# Reminder for Output format:
Your final answer should look like this:
<reward>
<explanation>
An explanation and the reasoning behind your decision.
</explanation>
<feedback>
Feedback to the alternative branch.
</feedback>
<value>
A single integer value between -100 and 100 based on your confidence in the correctness of the action and its likelihood of resolving the issue
</value>
</reward>

All the xml tags should be in a new line because we are going to parse it line by line.
Make sure to follow the output format to the letter and make not mistakes."#.to_owned();

        // generate the system message over here
        Ok(vec![
            LLMClientMessage::system(system_message),
            LLMClientMessage::user(action_observations),
            LLMClientMessage::user(current_message),
            LLMClientMessage::user(format_reminder),
        ])
    }

    // TODO(skcd): Pick up the system message from here and make it work properly
    fn system_message(
        &self,
        action_node: &ActionNode,
        search_tree: &SearchTree,
    ) -> Result<String, RewardError> {
        let tool_box = search_tree.tool_box();
        let trajectory_length = search_tree.trajectory(action_node.index()).len();
        // generate the system message where we have to show it the format
        // for generating the output
        if let Some(action) = action_node.action() {
            let tool_type = action.to_tool_type();
            let tool_reward_criteria = match tool_type.clone() {
                Some(ToolType::AttemptCompletion) => {
                    r#"Your role is to evaluate the executed action of the search tree that our AI agents are traversing, with the goal of ensuring that a complete and verified solution is in place. The agent believes that it has finished solving the programming issue."#.to_owned()
                }
                _ => {
                    r#"Your role is to evaluate the **last executed action** of the search tree that our AI agents are traversing, to help us determine the best trajectory to solve a programming issue. The agent is responsible for identifying and modifying the correct file(s) in response to the problem statement.

At this stage, the agent is still working on the solution. Your task is twofold:
1. **Evaluation**: Assess whether the change done by the **last executed action** is appropriate for addressing the problem and whether the agent is on the right path to resolving the issue.
2. **Alternative Feedback**: Independently of your evaluation, provide guidance for an alternative problem-solving branch. This ensures parallel exploration of different solution paths."#.to_owned()
                }
            };

            let reward_criteria = match tool_type.clone() {
                Some(tool_type) => tool_box
                    .tools()
                    .generate_evaluation_criteria(tool_type, trajectory_length),
                None => {
                    if trajectory_length < 3 {
                        vec![
                            "Exploratory Actions: Recognize that initial searches and information-gathering steps are essential and should not be heavily penalized if they don't yield immediate results.",
                            "Appropriateness of Action: Evaluate if the action is logical given the agent's current knowledge and the early stage of problem-solving.",
                        ].into_iter().map(|evaluation_criteria| evaluation_criteria.to_owned()).collect()
                    } else {
                        vec![
                            "Solution Quality: Assess the logical changes, contextual fit, and overall improvement without introducing new issues.",
                            "Progress Assessment: Evaluate the agent's awareness of solution history, detection of repetitive actions, and planned next steps.",
                            "Repetitive or Redundant Actions: Detect if the agent is repeating the same unsuccessful or redundant actions without making progress. Pay close attention to the agent's history and outputs indicating lack of progress.",
                        ].into_iter().map(|evaluation_criteria| evaluation_criteria.to_owned()).collect()
                    }
                }
            };

            let mut reward_scale = match tool_type.clone() {
                Some(tool_type) => tool_box
                    .tools()
                    .generate_reward_scale(tool_type, trajectory_length),
                None => {
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
            };

            reward_scale.sort_by_key(|reward| -reward.maximum());

            let (minimum_reward, maximum_reward) = match tool_type.clone() {
                Some(tool_type) => {
                    let reward_scaling = tool_box
                        .tools()
                        .generate_reward_scale(tool_type, trajectory_length);
                    (
                        reward_scaling
                            .iter()
                            .map(|reward_scale| reward_scale.minimum())
                            .min()
                            .unwrap_or(-100),
                        reward_scaling
                            .iter()
                            .map(|reward_scale| reward_scale.maximum())
                            .max()
                            .unwrap_or(100),
                    )
                }
                None => (-100, 100),
            };

            let reward_scale_prompt = reward_scale
                .into_iter()
                .map(|reward| {
                    if reward.minimum() == reward.maximum() {
                        format!(r#"* **{}**: {}"#, reward.minimum(), reward.description())
                    } else {
                        format!(
                            r#"* **{}** to **{}**: {}"#,
                            reward.minimum(),
                            reward.maximum(),
                            reward.description()
                        )
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");

            let reward_prompt = format!(
                r#"The reward value must be an integer between {} and {}, where:

{}"#,
                minimum_reward, maximum_reward, reward_scale_prompt
            );

            let system_prompt = format!(
                r#"{tool_reward_criteria}

# Evaluation Criteria:
{}

# Reward Scale and Guidelines:
{}

# Feedback Structure:

* **Explanation**: Offer a detailed explanation and reasoning behind your decision, focusing on the **last executed action**, its relation to previous actions and its impact.
* **Feedback to Alternative Branch**: Offer guidance for a parallel problem-solving branch. Suggest conceptual alternative approaches or strategies without providing actual code implementations.
* **Reward**: Assign a single integer value between {minimum_reward} and {maximum_reward} based on your confidence in the correctness of the action and its likelihood of resolving the issue.
"#,
                reward_criteria
                    .into_iter()
                    .map(|reward_criteria| format!(r#"* {reward_criteria}"#))
                    .collect::<Vec<_>>()
                    .join("\n"),
                reward_prompt,
            );

            let tools_present = search_tree.tools();
            let tool_description = tools_present
                .into_iter()
                .filter_map(|tool_type| tool_box.tools().get_tool_description(&tool_type))
                .collect::<Vec<_>>();
            let system_prompt = format!(
                r#"{system_prompt}

# Available Actions:
The following actions are avaiable for the agent to choose from:
{}


# Output format:
Your final answer should look like this:
<reward>
<explanation>
An explanation and the reasoning behind your decision.
</explanation>
<feedback>
Feedback to the alternative branch.
</feedback>
<value>
A single integer value between -100 and 100 based on your confidence in the correctness of the action and its likelihood of resolving the issue
</value>
</reward>

All the xml tags should be in a new line because we are going to parse it line by line.
Make sure to follow the output format to the letter and make not mistakes."#,
                tool_description.join("\n")
            );

            Ok(system_prompt)
        } else {
            Err(RewardError::ActionNotFound)
        }
    }
}
