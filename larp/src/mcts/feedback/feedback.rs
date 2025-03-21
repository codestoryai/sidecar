//! The feedback generator over here takes in the node we are on and tries to generate
//! feedback to pick an action which will lead to diversity

use llm_client::clients::types::LLMClientMessage;

use crate::{
    agentic::{
        symbol::events::message_event::SymbolEventMessageProperties,
        tool::{
            feedback::feedback::FeedbackGenerationRequest,
            input::ToolInput,
            r#type::{Tool, ToolType},
        },
    },
    mcts::{
        action_node::{ActionNode, SearchTree},
        agent_settings::settings::AgentSettings,
    },
};

use super::error::FeedbackError;

pub struct FeedbackToNode {
    // Analysis of the current task we are on and the different trajectories we have explored
    _analysis: String,
    // Direct feedback to the AI agent
    feedback: String,
}

impl FeedbackToNode {
    pub fn feedback(&self) -> &str {
        &self.feedback
    }
}

pub struct FeedbackGenerator {
    agent_settings: AgentSettings,
}

impl FeedbackGenerator {
    pub fn new(agent_settings: AgentSettings) -> Self {
        Self { agent_settings }
    }
}

impl FeedbackGenerator {
    pub async fn generate_feedback_for_node(
        &self,
        mut nodes_trajectory: Vec<&ActionNode>,
        search_tree: &SearchTree,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Option<FeedbackToNode>, FeedbackError> {
        if nodes_trajectory.is_empty() {
            return Err(FeedbackError::EmptyTrajectory);
        }

        let leaf = nodes_trajectory.pop();
        if leaf.is_none() {
            return Err(FeedbackError::EmptyTrajectory);
        }
        let leaf = leaf.expect("is_none to hold");
        let root_to_leaf = nodes_trajectory;

        let siblings = search_tree.get_sibling_nodes(leaf.index());
        // if we have no siblings right now, then we can't really generate feedback
        if siblings.is_empty() {
            return Ok(None);
        }

        // - the system prompt with all the actions avaiable right now
        let mut messages = vec![LLMClientMessage::system(
            self.system_message_for_feedback(search_tree),
        )];
        // - generate the analysis message from the siblings
        messages.extend(self.message_for_feedback(leaf, root_to_leaf, siblings, search_tree)?);

        // - generate the feedback to the node
        let tool_input = ToolInput::FeedbackGeneration(FeedbackGenerationRequest::new(
            messages,
            message_properties,
        ));
        let feedback = search_tree
            .tool_box()
            .tools()
            .invoke(tool_input)
            .await
            .map_err(|e| FeedbackError::ToolError(e))?
            .get_feedback_generation_response()
            .ok_or(FeedbackError::RootNotFound)?;

        Ok(Some(FeedbackToNode {
            _analysis: feedback.analysis().to_owned(),
            feedback: feedback.feedback().to_owned(),
        }))
    }

    fn system_message_for_feedback(&self, search_tree: &SearchTree) -> String {
        let system_message = if self.agent_settings.is_midwit() {
            // TODO(skcd): Pick up from here
            // give feedback about the use of tests to figure out the solution and
            // the solution space for the agent
            // safe guarding against the tests will help quite a lot
            // we can even run a repair agent to make sure that the tests are perfectly setup and
            // what changes we need to do for running a test
            format!(
                r#"Your task is to provide strategic feedback to guide the next execution of an action by another AI assistant. The AI assistant is working as a software engineer and is tasked with solving a <pr_description>.

**Context you will receive:**

    * Task Description: The main problem or objective that needs to be addressed wrapped in a <pr_description> tag.
    * Available Actions: The list of actions available to the agent.
    * History: The conversation leading up to the current state.
    * Hypothetical Attempts: Considred actions that NOT been executed in the current branch. They are hypothetical and serve as insights.
    * Warnings: Any duplicate attempts that have already been tried.

**Your role is to:**

    * Analyze What to Do Next: Combine your understanding of the task with insights from considred attempts (which are hypothetical and unexecuted) to determine the best next step.
    * Provide Feedback: Offer strategic guidance that focuses on novel and diverse solutions to address the task directly.
    * Avoid Duplicates: Strongly discourage repeating any actions flagged as duplicates.
    * Reproducing the error: Direct the AI assistant towards generating a SMALL reproduction of the <pr_description>. Often times there are test setup issues and your feedback should try to fix as many of them in a single go as possible.

**Instructions:**

    * Analysis: Begin with a thorough analysis that combines understanding the task and insights from considered (hypothetical) attempts, focusing on what should be done next.
    * Direct Feedback: Provide one concrete and innovative suggestion for the next action, specifying which available action to use (using the exact name from Available Actions) and how it addresses the task. Focus on details over here and try to avoid taking the same action. Your feedback should either help the AI assistant not make the same mistakes or provide a new novel direction to approach.

**Guiding philosophy**
    * Focus on the task's objectives and encourage a novel solution that hasn't been explored yet.
    * Make sure to check the History to understand the trajectory the agent is one.
    * You are not allowed to make any changes to the tests directory, and we are NOT ALLOWED TO RUN TESTS. This means that advising the AI assistant to run tests is not useful and instead focussing on getting the reproduce_error script to work is of utmost importance.
    * I've already taken care of all changes to any of the test files described in the <pr_description>. This means we can't modify the testing logic or any of the tests in any way!
    * If you notice that the AI assistant is going down a rabbit hole of changes, your feedback should help the assistant recognise that and try for a simpler solution. SIMPLER SOLUTIONS are always preferred by the system.

Remember: Focus on the task's objectives and encourage a novel solution that hasn't been explored yet. Use previous attempts as learning points but do not let them constrain your creativity in solving the task. The considered attempts are hypothetical and should inform, but not limit, your suggested action."#
            )
        } else {
            format!(
                r#"Your task is to provide strategic feedback to guide the next execution of an action by another AI assistant.
    
**Context you will receive:**

    * Task Description: The main problem or objective that needs to be addressed wrapped in a <task> tag.
    * History: The conversation leading up to the current state.
    * Hypothetical Attempts: Considred actions that NOT been executed in the current branch. They are hypothetical and serve as insights.
    * Warnings: Any duplicate attempts that have already been tried.

**Your role is to:**

    * Analyze What to Do Next: Combine your understanding of the task with insights from considred attempts (which are hypothetical and unexecuted) to determine the best next step.
    * Provide Feedback: Offer strategic guidance that focuses on novel and diverse solutions to address the task directly.
    * Avoid Duplicates: Strongly discourage repeating any actions flagged as duplicates.

**Instructions:**

    * Analysis: Begin with a brief analysis that combines understanding the task and insights from considered (hypothetical) attempts, focusing on what should be done next.
    * Direct Feedback: Provide one concrete and innovative suggestion for the next action, specifying which available action to use (using the exact name from Available Actions) and how it addresses the task.
    
Remember: Focus on the task's objectives and encourage a novel solution that hasn't been explored yet. Use previous attempts as learning points but do not let them constrain your creativity in solving the task. The considered attempts are hypothetical and should inform, but not limit, your suggested action."#
            )
        };

        // get the tools from the search tree over here so we can show that to the system message
        let tool_box = search_tree.tool_box();
        let action_message = search_tree
            .tools()
            .into_iter()
            .filter_map(|tool_type| tool_box.tools().get_tool_description(&tool_type))
            .collect::<Vec<_>>();

        let action_message = format!(
            r#"#  Available Actions:
The following actions were available for the AI assistant to choose from:

{}"#,
            action_message.join("\n\n")
        );

        // Now we talk about the output format
        let output_format = format!(
            r#"#  Output format:
Your final answer should look like this:
<feedback_generation>
<analysis>
Analysis of the current task we are on and the different trajectories we have explored
</analysis>
<feedback>
Direct feedback to the AI agent
</feedback>
</feedback_generation>

All the xml tags should be in a new line because we are going to parse it line by line.
Make sure to follow the output format to the letter and make not mistakes."#
        );

        format!(
            r#"{}
{}
{}"#,
            system_message, action_message, output_format
        )
    }

    fn message_for_feedback(
        &self,
        leaf: &ActionNode,
        root_to_leaf: Vec<&ActionNode>,
        siblings: Vec<&ActionNode>,
        search_tree: &SearchTree,
    ) -> Result<Vec<LLMClientMessage>, FeedbackError> {
        let root_node = search_tree.root();
        if let None = root_node {
            return Err(FeedbackError::RootNotFound);
        }
        let root_node = root_node.expect("if let None to hold");

        let problem_statement = root_node.message();
        if let None = problem_statement {
            return Err(FeedbackError::ProblemStatementNotFound);
        }
        let problem_statement = problem_statement.expect("if let None to hold");

        let mut messages = vec![];
        // generate the messages for the trajectory
        messages.extend(self.message_for_trajectory(
            leaf,
            root_to_leaf,
            problem_statement,
            search_tree,
        ));

        // generate the message for the sibling analysis
        messages.extend(self.generate_sibling_analysis(siblings));

        // push the reminder message over here for the format
        messages.push(vec![LLMClientMessage::user(format!(
            r#"# Reminder for Output format:
Your final answer should look like this:
<feedback_generation>
<analysis>
Analysis of the current task we are on and the different trajectories we have explored
</analysis>
<feedback>
Direct feedback to the AI agent
</feedback>
</feedback_generation>

All the xml tags should be in a new line because we are going to parse it line by line.
Make sure to follow the output format to the letter and make not mistakes."#
        ))]);

        Ok(messages.into_iter().flatten().collect())
    }

    fn generate_sibling_analysis(
        &self,
        siblings: Vec<&ActionNode>,
    ) -> Result<Vec<LLMClientMessage>, FeedbackError> {
        // have we tried to complete at this level? (look at siblings and check for finished)
        let is_finished = siblings.iter().any(|sibiling| {
            sibiling
                .action()
                .map(|action| action.to_tool_type())
                .flatten()
                .map(|tool_type| matches!(tool_type, ToolType::AttemptCompletion))
                .unwrap_or_default()
        });

        // gather the actions which we took at siblings
        let sibling_analysis = siblings.iter().enumerate().filter_map(|(idx, sibling)| {
            if let Some(action) = sibling.action() {
                if let None = action.to_tool_type() {
                    return None;
                }
                let tool_type = action.to_tool_type().expect("if let None to hold");
                let sibling_analysis = format!(
                    r#"## Attempt {}
**Action**: {}
{}
"#,
                    idx + 1,
                    tool_type.to_string(),
                    action.to_string(),
                );
                let duplicate_message = if sibling.is_duplicate() {
                    format!(r#"


**WARNING: DUPLICATE ATTEMPT DETECTED!**
This attempt was identical to a previous one. Repeating this exact approach would be ineffective and should be avoided.
"#).to_owned()
                } else {
                    "".to_owned()
                };

                let observation_message = if let Some(observation) = sibling.observation() {
                    format!(r#"

**Hypothetical observation**:
{}

"#, observation.message())
                } else {
                    "".to_owned()
                };

                let divider = r#"
---

"#;
                Some(format!(r#"{}{}{}{}"#, sibling_analysis, duplicate_message, observation_message, divider))
            } else {
                None
            }
        }).collect::<Vec<_>>().join("\n");

        // adding sibling analysis
        let sibling_analysis = format!(
            r#"# Hypothetical Attempts

{sibling_analysis}"#
        );

        // talk about the finished action
        let sibling_analysis = if is_finished {
            format!(
                r#"{sibling_analysis}

**WARNING: ATTEMPT COMPLETION HAS ALREADY BEEN ATTEMPTED!**
- Trying to attemp_completion again would be ineffective
- Focus on exploring alternative solutions instead"#
            )
        } else {
            sibling_analysis
        };

        Ok(vec![LLMClientMessage::user(sibling_analysis)])
    }

    /// Generates the message for the trajectory so we can give that as feedback
    /// to the agent
    /// root
    ///      -> parent
    ///           -> sibling
    ///           -> sibling
    /// git_patch_until_parent
    /// agent_context_until_parent
    fn message_for_trajectory(
        &self,
        leaf: &ActionNode,
        root_to_leaf: Vec<&ActionNode>,
        current_message: String,
        search_tree: &SearchTree,
    ) -> Result<Vec<LLMClientMessage>, FeedbackError> {
        let root_node = search_tree.root();
        if let None = root_node {
            return Err(FeedbackError::RootNotFound);
        }
        let root_node = root_node.expect("if let None to hold");

        let problem_statement = root_node.message();
        if let None = problem_statement {
            return Err(FeedbackError::ProblemStatementNotFound);
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
            r#"<pr_description>
{current_message}
</pr_description>

<action_observations>
{action_observations}
</action_observations>

The file context the agent has access to:
<files>
{}
</files>

The git diff of the changes until the last action:
<git_patch>
{}
</git_patch>"#,
            &file_content_vec.join("\n"),
            git_patch_diff.join("\n")
        );

        Ok(vec![LLMClientMessage::user(current_message)])
    }
}
