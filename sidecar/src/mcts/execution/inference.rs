//! Executes the nodes for coding its main purpose is the following:
//! fn execute(nodes: Vec<&ActionNode>) -> Result<ToolOutput, ExecutionError>;

use colored::Colorize;
use std::{collections::HashMap, sync::Arc, time::Duration};

use llm_client::clients::types::{LLMClientMessage, LLMClientToolReturn, LLMClientToolUse};

use crate::{
    agentic::{
        symbol::{
            errors::SymbolError, events::message_event::SymbolEventMessageProperties,
            tool_box::ToolBox,
        },
        tool::{
            input::{ToolInput, ToolInputPartial},
            lsp::{open_file::OpenFileRequest, search_file::SearchFileContentInput},
            r#type::Tool,
            repo_map::generator::RepoMapGeneratorRequest,
            session::{
                chat::SessionChatMessage,
                tool_use_agent::{
                    ToolUseAgent, ToolUseAgentInput, ToolUseAgentInputOnlyTools,
                    ToolUseAgentOutput, ToolUseAgentOutputWithTools,
                },
            },
            terminal::terminal::TerminalInput,
            test_runner::runner::TestRunnerRequest,
        },
    },
    mcts::{
        action_node::{ActionNode, ActionObservation, ActionToolParameters, SearchTree},
        agent_settings::settings::AgentSettings,
        editor::anthropic_computer::AnthropicCodeEditor,
    },
};

use super::error::InferenceError;

pub struct InferenceEngineResult {
    action_observation: Option<ActionObservation>,
    action_tool_parameters: ActionToolParameters,
    is_duplicate: bool,
}

impl InferenceEngineResult {
    pub fn new(
        action_observation: Option<ActionObservation>,
        action_tool_parameters: ActionToolParameters,
        is_duplicate: bool,
    ) -> Self {
        Self {
            action_observation,
            action_tool_parameters,
            is_duplicate,
        }
    }

    pub fn action_observation(&self) -> Option<ActionObservation> {
        self.action_observation.clone()
    }

    pub fn action_tool_parameters(&self) -> ActionToolParameters {
        self.action_tool_parameters.clone()
    }

    pub fn is_duplicate(&self) -> bool {
        self.is_duplicate
    }
}

pub struct InferenceEngine {
    agent_settings: AgentSettings,
}

impl InferenceEngine {
    pub fn new(agent_settings: AgentSettings) -> Self {
        Self { agent_settings }
    }

    pub async fn execute(
        &self,
        mut nodes_trajectory: Vec<&ActionNode>,
        search_tree: &SearchTree,
        is_duplicate_allowed: bool,
        tool_box: Arc<ToolBox>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<InferenceEngineResult, InferenceError> {
        // split the trajectories between the root and the leaf right now
        if nodes_trajectory.is_empty() {
            return Err(InferenceError::EmptyTrajectory);
        }

        let leaf = nodes_trajectory.pop();
        if leaf.is_none() {
            return Err(InferenceError::EmptyTrajectory);
        }
        let leaf = leaf.expect("is_none to hold");
        let root_to_leaf_direction = nodes_trajectory;

        // keep track of the last updated file
        let mut last_updated_file: HashMap<String, usize> = Default::default();

        root_to_leaf_direction
            .iter()
            .enumerate()
            .for_each(|(index, current_node)| {
                let node_parent = search_tree.parent(current_node);
                let updated_files = match node_parent {
                    None => current_node
                        .user_context()
                        .file_paths()
                        .into_iter()
                        .collect(),
                    Some(node_parent) => current_node
                        .user_context()
                        .get_updated_files(node_parent.user_context()),
                };

                updated_files.into_iter().for_each(|file_path| {
                    last_updated_file.insert(file_path, index);
                });
            });

        // message history
        let mut message_history = vec![];

        let mut problem_statment = None;

        // Now create the messages for the previous nodes which we have
        for (_index, current_node) in root_to_leaf_direction.iter().enumerate() {
            if let Some(message) = current_node.message() {
                problem_statment = Some(message.to_owned());
                message_history.push(LLMClientMessage::user(message));
            }

            // always give the full observation message and not just the summary
            // since we will be generating new actions and they might be based
            // on the read_file output or the code_edit output
            if let (Some(action), Some(observation)) =
                (current_node.action(), current_node.observation())
            {
                if self.agent_settings.is_json() {
                    match action {
                        ActionToolParameters::Errored(errored_string) => {
                            message_history.push(LLMClientMessage::assistant(errored_string));
                            // add the observation over here as well
                            message_history
                                .push(LLMClientMessage::user(observation.message().to_owned()));
                        }
                        ActionToolParameters::Tool(tool_parameters) => {
                            let tool_schema = tool_parameters.tool_input_partial().to_json_value();
                            match tool_schema {
                                Some(value) => {
                                    // add the observation over here as well
                                    let llm_client_message = if self.agent_settings.is_midwit() {
                                        match observation.thinking() {
                                            Some(thinking) => {
                                                LLMClientMessage::assistant(thinking.to_owned())
                                            }
                                            None => LLMClientMessage::assistant(
                                                "<thinking>\n...\n</thinking>".to_owned(),
                                            ),
                                        }
                                    } else {
                                        LLMClientMessage::assistant(
                                            "<thinking>\n...\n</thinking>".to_owned(),
                                        )
                                    }
                                    .insert_tool_use(LLMClientToolUse::new(
                                        tool_parameters
                                            .tool_input_partial()
                                            .to_tool_type()
                                            .to_string(),
                                        tool_parameters.tool_use_id().to_owned(),
                                        value,
                                    ));
                                    message_history.push(llm_client_message);
                                    message_history.push(
                                        LLMClientMessage::user("".to_owned())
                                            .insert_tool_return_values(vec![
                                                LLMClientToolReturn::new(
                                                    tool_parameters.tool_use_id().to_owned(),
                                                    tool_parameters
                                                        .tool_input_partial()
                                                        .to_tool_type()
                                                        .to_string(),
                                                    observation.message().to_owned(),
                                                ),
                                            ]),
                                    );
                                }
                                None => {
                                    message_history.push(LLMClientMessage::assistant(
                                        tool_parameters.tool_input_partial().to_string(),
                                    ));
                                    message_history.push(LLMClientMessage::user(
                                        observation.message().to_owned(),
                                    ));
                                }
                            }
                        }
                    }
                } else {
                    message_history.push(LLMClientMessage::assistant(action.to_string()));
                    message_history.push(LLMClientMessage::user(observation.message().to_owned()));
                }
            }
        }

        // do not do anything with the last updated files (yet)
        let _last_updated_files = last_updated_file;

        if let Some(feedback) = leaf.feedback() {
            message_history.push(LLMClientMessage::user(feedback));
        }

        // Now that we have the messages setup we ask the agent to generate the final tool which we want to use
        let execution_and_observe = if self.agent_settings.is_json() {
            self.generate_observation_for_node_json_mode(
                leaf,
                search_tree,
                is_duplicate_allowed,
                message_history,
                problem_statment.expect("to be alway present in the tree"),
                tool_box,
                message_properties,
            )
            .await
        } else {
            self.generate_observation_for_node(
                leaf,
                search_tree,
                message_history,
                tool_box,
                message_properties,
            )
            .await
        };
        execution_and_observe
    }

    async fn generate_observation_for_node_json_mode(
        &self,
        current_node: &ActionNode,
        search_tree: &SearchTree,
        is_duplicate_allowed: bool,
        messages: Vec<LLMClientMessage>,
        problem_statement: String,
        tool_box: Arc<ToolBox>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<InferenceEngineResult, InferenceError> {
        let tool_use_agent = ToolUseAgent::new(
            search_tree.llm_client(),
            search_tree.root_directory(),
            "linux".to_owned(),
            "bash".to_owned(),
            Some(search_tree.repo_name()),
        );

        let session_messages = messages
            .into_iter()
            .map(|message| SessionChatMessage::from_llm_message(message))
            .collect::<Vec<_>>();

        // add a reminder for the output format so it never forgets the thinking tag
        // we can't add that when we are in the tool mode
        //         session_messages.push(SessionChatMessage::user(
        //             r#" Output format reminder:
        // Always include the <thinking></thinking> section before using the tool."#
        //                 .to_owned(),
        //             vec![],
        //         ));

        let tool_agent_input = ToolUseAgentInputOnlyTools::new(
            session_messages,
            search_tree
                .tools()
                .into_iter()
                .filter_map(|tool_type| tool_box.tools().get_tool_json(&tool_type))
                .collect(),
            problem_statement,
            self.agent_settings.is_midwit(),
            None,
            message_properties.clone(),
        );

        // have a retry logic here which tries hard to make sure there are no errors
        // when creating the tool which needs to be used
        let mut tool_retry_index = 0;
        // we can try a max of 3 times before giving up
        let max_tool_retry = 3;

        let mut tool_use_output: Result<ToolUseAgentOutputWithTools, SymbolError>;
        loop {
            tool_use_output = tool_use_agent
                .invoke_json_tool_swe_bench(tool_agent_input.clone())
                .await;
            if tool_use_output.is_ok() {
                // check if the result of running the tool use output is empty
                if let Ok(tool_use_output) = tool_use_output.as_ref() {
                    match tool_use_output {
                        ToolUseAgentOutputWithTools::Success((tools, _input)) => {
                            if tools.is_empty() {
                                println!(
                                    "{}",
                                    format!("inference::enging::retrying_empty_tool_output").red()
                                );
                                tokio::time::sleep(Duration::from_secs(1)).await;
                                tool_retry_index = tool_retry_index + 1;
                                if tool_retry_index >= max_tool_retry {
                                    break;
                                }
                                continue;
                            }
                        }
                        _ => {}
                    }
                }
                break;
            } else {
                println!(
                    "{}",
                    format!("inference::engine::retrying_tool_call::erroredbefore").red()
                );
                tokio::time::sleep(Duration::from_secs(1)).await;
                // just give it a plain retry and call it a day
                tool_retry_index = tool_retry_index + 1;
            }
            if tool_retry_index >= max_tool_retry {
                break;
            }
        }

        // Now we get the tool use output
        match tool_use_output {
            Ok(tool_use_parameters) => match tool_use_parameters {
                // we are going to execute this branch of the code so we can get the output
                // over here
                ToolUseAgentOutputWithTools::Success((tool_input_partial, thinking)) => {
                    if tool_input_partial.is_empty() {
                        return Ok(InferenceEngineResult::new(
                            Some(ActionObservation::errored(
                                "failed to use any tool, please be careful".to_owned(),
                                // we failed to parse the tool output, so we can expect an correction
                                // over here
                                Some(thinking),
                                true,
                                false,
                            )),
                            ActionToolParameters::errored(
                                "failed to use any tool, please be careful".to_owned(),
                            ),
                            false,
                        ));
                    }
                    let (tool_use_id, tool_input_partial) = tool_input_partial[0].clone();
                    let tool_parameters =
                        ActionToolParameters::tool(tool_use_id, tool_input_partial.clone());
                    // we should also detect duplicates over here before we start executing
                    // before executing the tool, check if the tool parameters are equal
                    // we can start with doing something very simple before we do a hard thing
                    let is_duplicate = search_tree.is_duplicate(current_node, &tool_parameters);
                    // if duplicates are not allowed stop hard since we can't make progress
                    if !is_duplicate_allowed && is_duplicate {
                        Ok(InferenceEngineResult::new(None, tool_parameters, true))
                    } else {
                        let node_execution_output = self
                            .execute_tool_and_generate_observation(
                                tool_input_partial,
                                thinking.to_owned(),
                                tool_box.clone(),
                                message_properties.clone(),
                            )
                            .await;
                        match node_execution_output {
                            Ok(observation) => Ok(InferenceEngineResult::new(
                                Some(observation),
                                tool_parameters,
                                false,
                            )),
                            Err(e) => Ok(InferenceEngineResult::new(
                                // when we have an execution error on the tool we are royally
                                // messed up because we try our best to create an observation
                                // even for the failure cases, generally this means an infra
                                // failure so this is terminal
                                Some(ActionObservation::errored(
                                    e.to_string(),
                                    Some(thinking.to_owned()),
                                    false,
                                    true,
                                )),
                                tool_parameters,
                                false,
                            )),
                        }
                    }
                }
                ToolUseAgentOutputWithTools::Failure(thinking) => {
                    Ok(InferenceEngineResult::new(
                        Some(ActionObservation::errored(
                            "Failed to generate a tool to use".to_owned(),
                            // we failed to parse the tool output, so we can expect an correction
                            // over here
                            thinking,
                            true,
                            false,
                        )),
                        ActionToolParameters::errored(
                            "Failed to generate a tool to use".to_owned(),
                        ),
                        false,
                    ))
                }
            },
            Err(e) => Ok(InferenceEngineResult::new(
                // This is an infra error so we can't expect a correction and this is terminal
                // ideally we should expect a correction over here so that we do not mess
                // up our trajectory and the LLM gets confused and put into a box
                Some(ActionObservation::errored(e.to_string(), None, false, true)),
                ActionToolParameters::errored(e.to_string()),
                false,
            )),
        }
    }

    async fn generate_observation_for_node(
        &self,
        current_node: &ActionNode,
        search_tree: &SearchTree,
        messages: Vec<LLMClientMessage>,
        tool_box: Arc<ToolBox>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<InferenceEngineResult, InferenceError> {
        let tool_use_agent = ToolUseAgent::new(
            search_tree.llm_client(),
            search_tree.root_directory(),
            "linux".to_owned(),
            "bash".to_owned(),
            Some(search_tree.repo_name()),
        );

        let mut session_messages = messages
            .into_iter()
            .map(|message| SessionChatMessage::from_llm_message(message))
            .collect::<Vec<_>>();

        // add a reminder for the output format so it never forgets the thinking tag
        session_messages.push(SessionChatMessage::user(
            r#" Output format reminder:
Always include the <thinking></thinking> section before using the tool."#
                .to_owned(),
            vec![],
        ));

        let tool_agent_input = ToolUseAgentInput::new(
            session_messages,
            search_tree
                .tools()
                .into_iter()
                .filter_map(|tool_type| tool_box.tools().get_tool_description(&tool_type))
                .collect(),
            None,
            message_properties.clone(),
        );

        // now create the input for the tool use agent
        let tool_use_output = tool_use_agent.invoke(tool_agent_input).await;

        // Now we get the tool use output
        match tool_use_output {
            Ok(tool_use_parameters) => match tool_use_parameters {
                // we are going to execute this branch of the code so we can get the output
                // over here
                ToolUseAgentOutput::Success((tool_input_partial, thinking)) => {
                    let tool_parameters = ActionToolParameters::tool(
                        "tool_use".to_owned(),
                        tool_input_partial.clone(),
                    );
                    // we should also detect duplicates over here before we start executing
                    // before executing the tool, check if the tool parameters are equal
                    // we can start with doing something very simple before we do a hard thing
                    let is_duplicate = search_tree.is_duplicate(current_node, &tool_parameters);
                    if is_duplicate {
                        Ok(InferenceEngineResult::new(None, tool_parameters, true))
                    } else {
                        // TODO(skcd): Execute the tool and generate the observation we need
                        // for the node
                        let node_execution_output = self
                            .execute_tool_and_generate_observation(
                                tool_input_partial,
                                thinking.to_owned(),
                                tool_box.clone(),
                                message_properties.clone(),
                            )
                            .await;
                        match node_execution_output {
                            Ok(observation) => Ok(InferenceEngineResult::new(
                                Some(observation),
                                tool_parameters,
                                false,
                            )),
                            Err(e) => Ok(InferenceEngineResult::new(
                                // when we have an execution error on the tool we are royally
                                // messed up because we try our best to create an observation
                                // even for the failure cases, generally this means an infra
                                // failure so this is terminal
                                Some(ActionObservation::errored(
                                    e.to_string(),
                                    Some(thinking),
                                    false,
                                    true,
                                )),
                                tool_parameters,
                                false,
                            )),
                        }
                    }
                }
                ToolUseAgentOutput::Failure(failed_string) => Ok(InferenceEngineResult::new(
                    Some(ActionObservation::errored(
                        failed_string.to_owned(),
                        // we failed to parse the tool output, so we can expect an correction
                        // over here
                        None,
                        true,
                        false,
                    )),
                    ActionToolParameters::errored(failed_string),
                    false,
                )),
            },
            Err(e) => Ok(InferenceEngineResult::new(
                // This is an infra error so we can't expect a correction and this is terminal
                Some(ActionObservation::errored(e.to_string(), None, false, true)),
                ActionToolParameters::errored(e.to_string()),
                false,
            )),
        }
    }

    async fn execute_tool_and_generate_observation(
        &self,
        tool_input_partial: ToolInputPartial,
        tool_thinking: String,
        tool_box: Arc<ToolBox>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<ActionObservation, InferenceError> {
        match tool_input_partial {
            ToolInputPartial::AskFollowupQuestions(_) => {
                // we never hit this branch for ask followup
                Err(InferenceError::WrongToolOutput)
            }
            ToolInputPartial::AttemptCompletion(attemp_completion) => {
                let message = attemp_completion.to_string();
                Ok(ActionObservation::new(
                    message.to_owned(),
                    message,
                    Some(tool_thinking),
                    true,
                ))
            }
            ToolInputPartial::CodeEditing(_) => {
                todo!("code editing is not supported with the inference engine for code editing, use anthropic computer use api instead")
            }
            ToolInputPartial::LSPDiagnostics(_) => {
                todo!("LSP diagnostics are not supported right now")
            }
            ToolInputPartial::ListFiles(list_files) => {
                let directory_path = list_files.directory_path().to_owned();
                let input = ToolInput::ListFiles(list_files);
                let response = tool_box
                    .tools()
                    .invoke(input)
                    .await
                    .map_err(|e| InferenceError::ToolError(e))?;
                let list_files_output = response
                    .get_list_files_directory()
                    .ok_or(InferenceError::WrongToolOutput)?;
                let response = list_files_output
                    .files()
                    .into_iter()
                    .map(|file_path| file_path.to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                let message = format!(
                    r#"Content for directory {directory_path}
{}"#,
                    response.to_owned()
                );
                Ok(ActionObservation::new(
                    message.to_owned(),
                    message.to_owned(),
                    Some(tool_thinking),
                    false,
                ))
            }
            ToolInputPartial::OpenFile(open_file) => {
                let open_file_path = open_file.fs_file_path().to_owned();
                let request = OpenFileRequest::new(
                    open_file_path.to_owned(),
                    message_properties.editor_url(),
                );
                let input = ToolInput::OpenFile(request);
                let response = tool_box
                    .tools()
                    .invoke(input)
                    .await
                    .map_err(|e| InferenceError::ToolError(e))?
                    .get_file_open_response()
                    .ok_or(InferenceError::WrongToolOutput)?;
                Ok(ActionObservation::new(
                    format!(
                        r#"Here's the full content of the file:
{}"#,
                        &response.to_string()
                    ),
                    format!(
                        "Showed the content of the following file:\n{}",
                        &open_file_path
                    ),
                    Some(tool_thinking),
                    false,
                )
                .file_content_updated(open_file_path.to_owned(), response.to_content()))
            }
            ToolInputPartial::RepoMapGeneration(repo_map_request) => {
                let directory_path = repo_map_request.directory_path().to_owned();
                let request = ToolInput::RepoMapGeneration(RepoMapGeneratorRequest::new(
                    repo_map_request.directory_path().to_owned(),
                    3000,
                ));
                let tool_output = tool_box
                    .tools()
                    .invoke(request)
                    .await
                    .map_err(|e| InferenceError::ToolError(e))?
                    .repo_map_generator_response()
                    .ok_or(InferenceError::WrongToolOutput)?;
                let repo_map_str = tool_output.repo_map().to_owned();
                let message = format!(
                    r#"Here's the outline of classes and functions present in the directory {directory_path}
{repo_map_str}"#
                );
                Ok(ActionObservation::new(
                    message.to_owned(),
                    message,
                    Some(tool_thinking),
                    false,
                ))
            }
            ToolInputPartial::SearchFileContentWithRegex(search_file) => {
                let request = SearchFileContentInput::new(
                    search_file.directory_path().to_owned(),
                    search_file.regex_pattern().to_owned(),
                    search_file.file_pattern().map(|s| s.to_owned()),
                    message_properties.editor_url(),
                );
                let input = ToolInput::SearchFileContentWithRegex(request);
                let response = tool_box
                    .tools()
                    .invoke(input)
                    .await
                    .map_err(|e| InferenceError::ToolError(e))?
                    .get_search_file_content_with_regex()
                    .ok_or(InferenceError::WrongToolOutput)?;
                let response = response.response();
                let message = format!(
                    r#"Here's the result of running the search query
{}"#,
                    response
                );
                Ok(ActionObservation::new(
                    message.to_owned(),
                    message,
                    Some(tool_thinking),
                    false,
                ))
            }
            ToolInputPartial::TerminalCommand(terminal_command) => {
                let command = terminal_command.command().to_owned();
                let request =
                    TerminalInput::new(command.to_owned(), message_properties.editor_url());
                let input = ToolInput::TerminalCommand(request);
                let tool_output = tool_box
                    .tools()
                    .invoke(input)
                    .await
                    .map_err(|e| InferenceError::ToolError(e))?
                    .terminal_command()
                    .ok_or(InferenceError::WrongToolOutput)?;
                let output = tool_output.output().to_owned();
                let message = format!(
                    r#"Here's the output from running the terminal command
Command: {}
Terminal output: {}"#,
                    command, output
                );
                Ok(ActionObservation::new(
                    message.to_owned(),
                    message,
                    Some(tool_thinking),
                    false,
                ))
            }
            ToolInputPartial::TestRunner(test_runner_output) => {
                let editor_url = message_properties.editor_url().to_owned();
                let fs_file_paths = test_runner_output.fs_file_paths();
                let input =
                    ToolInput::RunTests(TestRunnerRequest::new(fs_file_paths.to_vec(), editor_url));
                let response = tool_box
                    .tools()
                    .invoke(input)
                    .await
                    .map_err(|e| InferenceError::ToolError(e))?
                    .get_test_runner()
                    .ok_or(InferenceError::WrongToolOutput)?;
                let message = format!(
                    r#"Here's the result of running the tests on the following files:
{}
                
Test Output from the script (we also have to setup the test runner):
Exit code: {}
Output:
{}"#,
                    fs_file_paths.join("\n"),
                    response.exit_code(),
                    response.test_output()
                );
                Ok(ActionObservation::new(
                    message.to_owned(),
                    message,
                    Some(tool_thinking),
                    false,
                ))
            }
            ToolInputPartial::CodeEditorParameters(code_editor_parameters) => {
                let editor = AnthropicCodeEditor::new(tool_thinking.to_owned());
                let observation = editor.run_command(code_editor_parameters).await;
                match observation {
                    Ok(observation_ok) => Ok(observation_ok),
                    Err(e) => Ok(ActionObservation::errored(
                        e.to_string(),
                        Some(tool_thinking),
                        true,
                        false,
                    )),
                }
            }
        }
    }
}
