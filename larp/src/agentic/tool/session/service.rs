//! Creates the service which handles saving the session and extending it

use std::{collections::HashMap, sync::Arc};

use color_eyre::owo_colors::OwoColorize;
use colored::Colorize;
use llm_client::{broker::LLMBroker, clients::types::LLMType};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{
    agentic::{
        symbol::{
            errors::SymbolError, events::message_event::SymbolEventMessageProperties,
            identifier::LLMProperties, manager::SymbolManager, scratch_pad::ScratchPadAgent,
            tool_box::ToolBox, ui_event::UIEventWithID,
        },
        tool::{
            code_edit::code_editor::EditorCommand,
            input::ToolInputPartial,
            plan::service::PlanService,
            r#type::ToolType,
            session::{
                session::AgentToolUseOutput,
                tool_use_agent::{
                    ToolUseAgent, ToolUseAgentOutputType, ToolUseAgentProperties,
                    ToolUseAgentReasoningParamsPartial,
                },
            },
        },
    },
    chunking::text_document::Range,
    mcts::action_node::{ActionNode, ActionToolParameters, SearchTreeMinimal},
    repo::types::RepoRef,
    user_context::types::UserContext,
};

use super::session::{AideAgentMode, Session};

/// The session service which takes care of creating the session and manages the storage
pub struct SessionService {
    tool_box: Arc<ToolBox>,
    symbol_manager: Arc<SymbolManager>,
    running_exchanges: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl SessionService {
    pub fn new(tool_box: Arc<ToolBox>, symbol_manager: Arc<SymbolManager>) -> Self {
        Self {
            tool_box,
            symbol_manager,
            running_exchanges: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn track_exchange(
        &self,
        session_id: &str,
        exchange_id: &str,
        cancellation_token: CancellationToken,
    ) {
        let hash_id = format!("{}-{}", session_id, exchange_id);
        let mut running_exchanges = self.running_exchanges.lock().await;
        running_exchanges.insert(hash_id, cancellation_token);
    }

    pub async fn get_cancellation_token(
        &self,
        session_id: &str,
        exchange_id: &str,
    ) -> Option<CancellationToken> {
        let hash_id = format!("{}-{}", session_id, exchange_id);
        let running_exchanges = self.running_exchanges.lock().await;
        running_exchanges
            .get(&hash_id)
            .map(|cancellation_token| cancellation_token.clone())
    }

    pub fn create_new_session_with_tools(
        &self,
        session_id: &str,
        project_labels: Vec<String>,
        repo_ref: RepoRef,
        storage_path: String,
        tools: Vec<ToolType>,
        user_context: UserContext,
    ) -> Session {
        Session::new(
            session_id.to_owned(),
            project_labels,
            repo_ref,
            storage_path,
            user_context,
            tools,
        )
    }

    fn create_new_session(
        &self,
        session_id: String,
        project_labels: Vec<String>,
        repo_ref: RepoRef,
        storage_path: String,
        global_user_context: UserContext,
    ) -> Session {
        Session::new(
            session_id,
            project_labels,
            repo_ref,
            storage_path,
            global_user_context,
            vec![],
        )
    }

    pub async fn human_message(
        &self,
        session_id: String,
        storage_path: String,
        exchange_id: String,
        human_message: String,
        user_context: UserContext,
        project_labels: Vec<String>,
        repo_ref: RepoRef,
        agent_mode: AideAgentMode,
        aide_rules: Option<String>,
        mut message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        println!("session_service::human_message::start");
        let mut session = if let Ok(session) = self.load_from_storage(storage_path.to_owned()).await
        {
            println!(
                "session_service::load_from_storage_ok::session_id({})",
                &session_id
            );
            session
        } else {
            self.create_new_session(
                session_id.to_owned(),
                project_labels.to_vec(),
                repo_ref.clone(),
                storage_path,
                user_context.clone(),
            )
        };

        println!("session_service::session_created");

        // truncate hidden messages
        session.truncate_hidden_exchanges();

        // add human message
        session = session.human_message(
            exchange_id.to_owned(),
            human_message,
            user_context,
            project_labels,
            repo_ref,
        );

        let plan_exchange_id = self
            .tool_box
            .create_new_exchange(session_id.to_owned(), message_properties.clone())
            .await?;

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        self.track_exchange(&session_id, &plan_exchange_id, cancellation_token.clone())
            .await;
        message_properties = message_properties
            .set_request_id(plan_exchange_id)
            .set_cancellation_token(cancellation_token);

        // now react to the last message
        session = session
            .reply_to_last_exchange(
                agent_mode,
                self.tool_box.clone(),
                exchange_id,
                aide_rules,
                message_properties,
            )
            .await?;

        // save the session to the disk
        self.save_to_storage(&session, None).await?;
        Ok(())
    }

    /// Takes the user iteration request and regenerates the plan a new
    /// by reacting according to the user request
    pub async fn plan_iteration(
        &self,
        session_id: String,
        storage_path: String,
        plan_storage_path: String,
        plan_id: String,
        plan_service: PlanService,
        exchange_id: String,
        iteration_request: String,
        user_context: UserContext,
        aide_rules: Option<String>,
        project_labels: Vec<String>,
        repo_ref: RepoRef,
        _root_directory: String,
        _codebase_search: bool,
        mut message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        // Things to figure out:
        // - should we rollback all the changes we did before over here or build
        // on top of it
        // - we have to send the messages again on the same request over here
        // which implies that the same exchange id will be used to reset the plan which
        // has already happened
        // - we need to also send an event stating that the review pane needs a refresh
        // since we are generating a new request over here
        println!("session_service::plan::plan_iteration::start");
        let mut session = if let Ok(session) = self.load_from_storage(storage_path.to_owned()).await
        {
            println!(
                "session_service::load_from_storage_ok::session_id({})",
                &session_id
            );
            session
        } else {
            self.create_new_session(
                session_id.to_owned(),
                project_labels.to_vec(),
                repo_ref.clone(),
                storage_path,
                user_context.clone(),
            )
        };

        // truncate hidden messages
        session.truncate_hidden_exchanges();

        // One trick over here which we can do for now is keep track of the
        // exchange which we are going to reply to this way we make sure
        // that we are able to get the right exchange properly
        let user_plan_request_exchange = session.get_parent_exchange_id(&exchange_id);
        if let None = user_plan_request_exchange {
            return Ok(());
        }
        let user_plan_request_exchange = user_plan_request_exchange.expect("if let None to hold");
        let user_plan_exchange_id = user_plan_request_exchange.exchange_id().to_owned();
        session = session.plan_iteration(
            user_plan_request_exchange.exchange_id().to_owned(),
            iteration_request.to_owned(),
            user_context,
        );
        // send a chat message over here telling the editor about the followup:
        let _ = message_properties
            .ui_sender()
            .send(UIEventWithID::chat_event(
                session_id.to_owned(),
                user_plan_exchange_id.to_owned(),
                "".to_owned(),
                Some(format!(
                    r#"\n### Followup:
{iteration_request}"#
                )),
            ));

        let user_plan_request_exchange =
            session.get_exchange_by_id(user_plan_request_exchange.exchange_id());
        self.save_to_storage(&session, None).await?;
        // we get the exchange using the parent id over here, since what we get
        // here is the reply_exchange and we want to get the parent one to which we
        // are replying since thats the source of truth
        // keep track of the user requests for the plan generation as well since
        // we are iterating quite a bit
        let cancellation_token = tokio_util::sync::CancellationToken::new();
        message_properties = message_properties
            .set_request_id(exchange_id.to_owned())
            .set_cancellation_token(cancellation_token);
        // now we can perform the plan generation over here
        session = session
            .perform_plan_generation(
                plan_service,
                plan_id,
                user_plan_exchange_id,
                user_plan_request_exchange,
                aide_rules,
                plan_storage_path,
                self.tool_box.clone(),
                self.symbol_manager.clone(),
                message_properties,
            )
            .await?;
        // save the session to the disk
        self.save_to_storage(&session, None).await?;

        println!("session_service::plan_iteration::stop");
        Ok(())
    }

    /// Generates the plan over here and upon invocation we take care of executing
    /// the steps
    pub async fn plan_generation(
        &self,
        session_id: String,
        storage_path: String,
        plan_storage_path: String,
        plan_id: String,
        plan_service: PlanService,
        exchange_id: String,
        query: String,
        user_context: UserContext,
        project_labels: Vec<String>,
        repo_ref: RepoRef,
        _root_directory: String,
        _codebase_search: bool,
        aide_rules: Option<String>,
        mut message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        println!("session_service::plan::agentic::start");
        let mut session = if let Ok(session) = self.load_from_storage(storage_path.to_owned()).await
        {
            println!(
                "session_service::load_from_storage_ok::session_id({})",
                &session_id
            );
            session
        } else {
            self.create_new_session(
                session_id.to_owned(),
                project_labels.to_vec(),
                repo_ref.clone(),
                storage_path,
                user_context.clone(),
            )
        };

        // truncate hidden messages
        session.truncate_hidden_exchanges();

        // add an exchange that we are going to genrate a plan over here
        session = session.plan(exchange_id.to_owned(), query, user_context);
        self.save_to_storage(&session, None).await?;

        let exchange_in_focus = session.get_exchange_by_id(&exchange_id);

        // create a new exchange over here for the plan
        let plan_exchange_id = self
            .tool_box
            .create_new_exchange(session_id.to_owned(), message_properties.clone())
            .await?;
        println!("session_service::plan_generation::create_new_exchange::session_id({})::plan_exchange_id({})", &session_id, &plan_exchange_id);

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        self.track_exchange(&session_id, &plan_exchange_id, cancellation_token.clone())
            .await;
        message_properties = message_properties
            .set_request_id(plan_exchange_id)
            .set_cancellation_token(cancellation_token);
        // now we can perform the plan generation over here
        session = session
            .perform_plan_generation(
                plan_service,
                plan_id,
                exchange_id.to_owned(),
                exchange_in_focus,
                aide_rules,
                plan_storage_path,
                self.tool_box.clone(),
                self.symbol_manager.clone(),
                message_properties,
            )
            .await?;
        // save the session to the disk
        self.save_to_storage(&session, None).await?;

        println!("session_service::plan_generation::stop");
        Ok(())
    }

    pub async fn tool_use_agentic(
        &self,
        session_id: String,
        storage_path: String,
        user_message: String,
        exchange_id: String,
        all_files: Vec<String>,
        open_files: Vec<String>,
        shell: String,
        project_labels: Vec<String>,
        repo_ref: RepoRef,
        root_directory: String,
        tools: Vec<ToolType>,
        tool_box: Arc<ToolBox>,
        llm_broker: Arc<LLMBroker>,
        user_context: UserContext,
        reasoning: bool,
        running_in_editor: bool,
        mcts_log_directory: Option<String>,
        tool_use_agent_properties: ToolUseAgentProperties,
        message_properties: SymbolEventMessageProperties,
        context_crunching_llm: Option<LLMProperties>,
    ) -> Result<(), SymbolError> {
        println!("session_service::tool_use_agentic::start");
        let mut session =
            if let Ok(session) = self.load_from_storage(storage_path.to_owned()).await {
                println!(
                    "session_service::load_from_storage_ok::session_id({})",
                    &session_id
                );
                session
            } else {
                self.create_new_session_with_tools(
                    &session_id,
                    project_labels.to_vec(),
                    repo_ref.clone(),
                    storage_path.to_owned(),
                    vec![],
                    UserContext::default(),
                )
            }
            // always update the tools over here, no matter what the session had before
            // this is essential because the same session might be crossing over from
            // a chat or edit
            .set_tools(tools);

        // truncate hidden messages
        session.truncate_hidden_exchanges();

        let tool_agent = ToolUseAgent::new(
            llm_broker.clone(),
            root_directory.to_owned(),
            // os can be passed over here safely since we can assume the sidecar is running
            // close to the vscode server
            // we should ideally get this information from the vscode-server side setting
            std::env::consts::OS.to_owned(),
            tool_use_agent_properties,
        )
        .set_context_crunching_llm(context_crunching_llm.clone());

        // only when it is json mode that we switch the human message
        if tool_agent.is_json_mode_and_eval() {
            session = session.pr_description(exchange_id.to_owned(), user_message.to_owned());
        } else {
            session = session
                .human_message_tool_use(
                    exchange_id.to_owned(),
                    user_message.to_owned(),
                    all_files,
                    open_files,
                    shell.to_owned(),
                    user_context.clone(),
                )
                .await;
        }
        let _ = self
            .save_to_storage(&session, mcts_log_directory.clone())
            .await;

        session = session.accept_open_exchanges_if_any(message_properties.clone());

        // now that we have saved it we can start the loop over here and look out for the cancellation
        // token which will imply that we should end the current loop
        if reasoning {
            // do the reasoning here and then send over the task to the agent_loop
            let mut action_nodes_from = session.action_nodes().len();
            let mut tool_use_reasoning_input = None;
            loop {
                let session = self.load_from_storage(storage_path.to_owned()).await;
                if let Err(_) = session {
                    break;
                }

                let mut session = session.expect("if let Err to hold");
                println!(
                    "session::action_nodes_len({})",
                    session.action_nodes().len()
                );

                // if there are more than 200 action nodes, stop
                if session.action_nodes().len() >= 200 {
                    println!("exceeded_action_nodes");
                    break;
                }

                // grab the reasoning instruction
                let reasoning_instruction = session
                    .clone()
                    .get_reasoning_instruction(
                        tool_agent.clone(),
                        user_message.to_owned(),
                        action_nodes_from,
                        tool_use_reasoning_input.clone(),
                        message_properties.clone(),
                    )
                    .await;

                if reasoning_instruction.is_err() {
                    println!("reasoning_instruction::is_err::{:?}", reasoning_instruction);
                    break;
                }

                let mut reasoning_instruction = reasoning_instruction.expect("is_err to hold");
                println!("{}", reasoning_instruction.to_string());

                // when we have no instrucions we should break
                if reasoning_instruction.instruction().trim().is_empty() {
                    println!("reasoning_instruction::empty_instruction::break");
                    break;
                }

                // add the reasoning to our action nodes
                session.add_action_node(
                    ActionNode::default_with_index(session.action_nodes().len()).set_action_tools(
                        ToolInputPartial::Reasoning(
                            ToolUseAgentReasoningParamsPartial::from_params(
                                Some(reasoning_instruction.clone()),
                                user_message.clone(),
                            ),
                        ),
                    ),
                );

                // keep track of where we are last starting from
                action_nodes_from = session.action_nodes().len();

                if let Some(tool_use_reasoning_input) = tool_use_reasoning_input {
                    // add previous notes from the previous invocation
                    reasoning_instruction.add_previous_notes(tool_use_reasoning_input.notes());
                }

                tool_use_reasoning_input = Some(reasoning_instruction.clone());

                // reset the exchanges in the session
                session.reset_exchanges();

                // update the human message here with the instruction from o1
                session = session
                    .human_message_tool_use(
                        exchange_id.to_owned(),
                        reasoning_instruction.instruction().to_owned(),
                        vec![],
                        vec![],
                        shell.to_owned(),
                        user_context.clone(),
                    )
                    .await;

                // start the agent loop
                let _ = self
                    .agent_loop(
                        session.clone(),
                        user_message.to_owned(),
                        shell.to_owned(),
                        user_context.clone(),
                        running_in_editor,
                        mcts_log_directory.clone(),
                        tool_box.clone(),
                        tool_agent.clone(),
                        root_directory.clone(),
                        exchange_id.clone(),
                        message_properties.clone(),
                        context_crunching_llm.clone(),
                    )
                    .await;

                // do not save the session here since we update the session
                // on the disk in the agent_loop by itself
            }
            Ok(())
        } else {
            let output = self
                .agent_loop(
                    session,
                    user_message,
                    shell.to_owned(),
                    user_context.clone(),
                    running_in_editor,
                    mcts_log_directory,
                    tool_box,
                    tool_agent,
                    root_directory,
                    exchange_id,
                    message_properties,
                    context_crunching_llm,
                )
                .await;
            println!("agent_loop::output::({:?})", &output);
            output
        }
    }

    /// Hot loop for the tool agent to work in
    async fn agent_loop(
        &self,
        mut session: Session,
        original_user_message: String,
        shell: String,
        user_context: UserContext,
        running_in_editor: bool,
        mcts_log_directory: Option<String>,
        tool_box: Arc<ToolBox>,
        tool_agent: ToolUseAgent,
        root_directory: String,
        parent_exchange_id: String,
        mut message_properties: SymbolEventMessageProperties,
        _context_crunching_llm: Option<LLMProperties>,
    ) -> Result<(), SymbolError> {
        let mut previous_failure = false;
        loop {
            println!("tool_use_agentic::looping_again");
            let _ = self
                .save_to_storage(&session, mcts_log_directory.clone())
                .await;
            let tool_exchange_id = self
                .tool_box
                .create_new_exchange(session.session_id().to_owned(), message_properties.clone())
                .await?;

            println!("tool_exchange_id::({:?})", &tool_exchange_id);

            let cancellation_token = tokio_util::sync::CancellationToken::new();

            message_properties = message_properties
                .set_request_id(tool_exchange_id.to_owned())
                .set_cancellation_token(cancellation_token.clone());

            // track the new exchange over here
            self.track_exchange(
                &session.session_id(),
                &tool_exchange_id,
                cancellation_token.clone(),
            )
            .await;

            // this enables context crunching selectively
            let context_crunching = whoami::username() == "skcd".to_owned()
                || whoami::username() == "root".to_owned()
                || std::env::var("SIDECAR_ENABLE_REASONING").map_or(false, |v| !v.is_empty());

            if context_crunching {
                if let Some(input_tokens) = session
                    .action_nodes()
                    .last()
                    .map(|action_node| action_node.get_llm_usage_statistics())
                    .flatten()
                    .map(|llm_stats| {
                        llm_stats.input_tokens().unwrap_or_default()
                            + llm_stats.cached_input_tokens().unwrap_or_default()
                    })
                {
                    // if the input tokens are greater than 60k then do context crunching
                    // over here and lighten the context for the agent
                    // if the input tokens are greater than 60k then do context crunching
                    // over here and lighten the context for the agent
                    // For custom LLMs, we use a higher token threshold
                    let llm = message_properties.llm_properties().llm();
                    let token_threshold =
                        if llm.is_custom() || matches!(llm, &LLMType::ClaudeSonnet3_7) {
                            150_000
                        } else {
                            60_000
                        };
                    if input_tokens >= token_threshold {
                        println!("context_crunching");
                        // the right way to do this would be since the last reasoning node which was present here
                        let last_reasoning_node_index =
                            session.last_reasoning_node_if_any().unwrap_or_default();

                        let last_reasoning_list_list = session.last_reasoning_node_list();
                        // we also need the original human message over here, but what if there are multiple human messages??
                        // no we can just assume that the context crunching will keep the essence of the original human message for now
                        // TODO(skcd): Pick up from here
                        let context_crunching_output = session
                            .context_crunching(
                                tool_agent.clone(),
                                original_user_message.to_owned(),
                                last_reasoning_node_index,
                                last_reasoning_list_list,
                                message_properties.clone(),
                            )
                            .await?;

                        let output = context_crunching_output.output_type();
                        match output {
                            ToolUseAgentOutputType::Success(tool_use_success) => {
                                match tool_use_success.tool_parameters().clone() {
                                    ToolInputPartial::ContextCrunching(context_crunching) => {
                                        // add the context crunching to our action nodes
                                        session.add_action_node(
                                            ActionNode::default_with_index(
                                                session.action_nodes().len(),
                                            )
                                            .set_action_tools(ToolInputPartial::ContextCrunching(
                                                context_crunching.clone(),
                                            )),
                                        );

                                        // reset all the exchanges from before, this is the new starting point
                                        session.reset_exchanges();

                                        // now add a new human message for the context compression
                                        session = session
                                            .human_message_tool_use(
                                                tool_exchange_id.to_owned(),
                                                context_crunching.instruction().to_owned(),
                                                vec![],
                                                vec![],
                                                shell.to_owned(),
                                                user_context.clone(),
                                            )
                                            .await;

                                        let _ = self
                                            .save_to_storage(&session, mcts_log_directory.clone())
                                            .await;

                                        // now we try to start the loop again, with the assumption
                                        // that we won't be exceeding the context window anymore
                                        continue;
                                    }
                                    _ => {}
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            // update the setting for the tool agent
            let tool_agent = if previous_failure {
                tool_agent.clone().set_temperature(0.4)
            } else {
                tool_agent.clone()
            };

            // reset all dynamic properties here, starting with previous_failure
            previous_failure = false;

            // if reasoning is enabled we check how many steps we have taken and trigger
            // the reasoning in between
            let tool_use_output = session
                // the clone here is pretty bad but its the easiest and the sanest
                // way to keep things on the happy path
                .clone()
                .get_tool_to_use(
                    tool_box.clone(),
                    tool_exchange_id.to_owned(),
                    parent_exchange_id.to_owned(),
                    tool_agent,
                    message_properties.clone(),
                )
                .await;

            match tool_use_output {
                Ok(AgentToolUseOutput::Success((tool_input_partial, tool_use_id, new_session))) => {
                    // update our session
                    session = new_session;
                    // store to disk
                    let _ = self
                        .save_to_storage(&session, mcts_log_directory.clone())
                        .await;
                    let tool_type = tool_input_partial.to_tool_type();

                    // invoke the tool and update the session over here
                    session = session
                        .invoke_tool(
                            tool_type.clone(),
                            tool_input_partial,
                            tool_use_id,
                            tool_box.clone(),
                            root_directory.to_owned(),
                            message_properties.clone(),
                        )
                        .await?;

                    let _ = self
                        .save_to_storage(&session, mcts_log_directory.clone())
                        .await;
                    if matches!(tool_type, ToolType::AskFollowupQuestions)
                        || matches!(tool_type, ToolType::AttemptCompletion)
                    {
                        // we break if it is any of these 2 events, since these
                        // require the user to intervene
                        println!("session_service::tool_use_agentic::reached_terminating_tool");
                        break;
                    }
                }
                Ok(AgentToolUseOutput::Cancelled) => {
                    println!("session_service::tool_use_agentic::cancelled");
                    // if it is cancelled then we should break
                    break;
                }
                Ok(AgentToolUseOutput::Failed(failed_to_parse_output)) => {
                    previous_failure = true;
                    let _ = message_properties
                        .ui_sender()
                        .send(UIEventWithID::tool_not_found(
                            session.session_id().to_owned(),
                            tool_exchange_id.to_owned(),
                            failed_to_parse_output.to_owned(),
                        ));
                    // only bail if we are running in the editor environment
                    if running_in_editor {
                        return Err(SymbolError::WrongToolOutput);
                    }
                }
                Ok(AgentToolUseOutput::Errored(e)) => {
                    // if we have an error over here coming from the library then bubble it up
                    // to the user
                    previous_failure = true;
                    let _ = message_properties
                        .ui_sender()
                        .send(UIEventWithID::tool_errored_out(
                            session.session_id().to_owned(),
                            tool_exchange_id.to_owned(),
                            e.to_string(),
                        ));
                    // only bail hard when we are running in the editor
                    if running_in_editor {
                        Err(e)?
                    }
                }
                Err(e) => {
                    eprintln!("{}", &e);
                    let _ = message_properties
                        .ui_sender()
                        .send(UIEventWithID::tool_errored_out(
                            session.session_id().to_owned(),
                            tool_exchange_id.to_owned(),
                            e.to_string(),
                        ));
                    // only bail if we are running in the editor environment
                    if running_in_editor {
                        Err(e)?
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn code_edit_agentic(
        &self,
        session_id: String,
        storage_path: String,
        scratch_pad_agent: ScratchPadAgent,
        exchange_id: String,
        edit_request: String,
        user_context: UserContext,
        project_labels: Vec<String>,
        repo_ref: RepoRef,
        root_directory: String,
        codebase_search: bool,
        aide_rules: Option<String>,
        mut message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        println!("session_service::code_edit::agentic::start");
        let mut session = if let Ok(session) = self.load_from_storage(storage_path.to_owned()).await
        {
            println!(
                "session_service::load_from_storage_ok::session_id({})",
                &session_id
            );
            session
        } else {
            self.create_new_session(
                session_id.to_owned(),
                project_labels.to_vec(),
                repo_ref.clone(),
                storage_path,
                user_context.clone(),
            )
        };

        // truncate hidden messages
        session.truncate_hidden_exchanges();

        // add an exchange that we are going to perform anchored edits
        session = session.agentic_edit(exchange_id, edit_request, user_context, codebase_search);

        session = session.accept_open_exchanges_if_any(message_properties.clone());
        let edit_exchange_id = self
            .tool_box
            .create_new_exchange(session_id.to_owned(), message_properties.clone())
            .await?;

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        self.track_exchange(&session_id, &edit_exchange_id, cancellation_token.clone())
            .await;
        message_properties = message_properties
            .set_request_id(edit_exchange_id)
            .set_cancellation_token(cancellation_token);

        session = session
            .perform_agentic_editing(
                scratch_pad_agent,
                root_directory,
                aide_rules,
                message_properties,
            )
            .await?;

        // save the session to the disk
        self.save_to_storage(&session, None).await?;
        println!("session_service::code_edit::agentic::stop");
        Ok(())
    }

    /// We are going to try and do code edit since we are donig anchored edit
    pub async fn code_edit_anchored(
        &self,
        session_id: String,
        storage_path: String,
        scratch_pad_agent: ScratchPadAgent,
        exchange_id: String,
        edit_request: String,
        user_context: UserContext,
        aide_rules: Option<String>,
        project_labels: Vec<String>,
        repo_ref: RepoRef,
        mut message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        println!("session_service::code_edit::anchored::start");
        let mut session = if let Ok(session) = self.load_from_storage(storage_path.to_owned()).await
        {
            println!(
                "session_service::load_from_storage_ok::session_id({})",
                &session_id
            );
            session
        } else {
            self.create_new_session(
                session_id.to_owned(),
                project_labels.to_vec(),
                repo_ref.clone(),
                storage_path,
                user_context.clone(),
            )
        };

        // truncate hidden messages
        session.truncate_hidden_exchanges();

        let selection_variable = user_context.variables.iter().find(|variable| {
            variable.is_selection()
                && !(variable.start_position.line() == 0 && variable.end_position.line() == 0)
        });
        if selection_variable.is_none() {
            return Ok(());
        }
        let selection_variable = selection_variable.expect("is_none to hold above");
        let selection_range = Range::new(
            selection_variable.start_position,
            selection_variable.end_position,
        );
        println!("session_service::selection_range::({:?})", &selection_range);
        let selection_fs_file_path = selection_variable.fs_file_path.to_owned();
        let file_content = self
            .tool_box
            .file_open(
                selection_fs_file_path.to_owned(),
                message_properties.clone(),
            )
            .await?;
        let file_content_in_range = file_content
            .content_in_range(&selection_range)
            .unwrap_or(selection_variable.content.to_owned());

        session = session.accept_open_exchanges_if_any(message_properties.clone());
        let edit_exchange_id = self
            .tool_box
            .create_new_exchange(session_id.to_owned(), message_properties.clone())
            .await?;

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        self.track_exchange(&session_id, &edit_exchange_id, cancellation_token.clone())
            .await;
        message_properties = message_properties
            .set_request_id(edit_exchange_id)
            .set_cancellation_token(cancellation_token);

        // add an exchange that we are going to perform anchored edits
        session = session.anchored_edit(
            exchange_id.to_owned(),
            edit_request,
            user_context,
            selection_range,
            selection_fs_file_path,
            file_content_in_range,
        );

        // Now we can start editing the selection over here
        session = session
            .perform_anchored_edit(
                exchange_id,
                scratch_pad_agent,
                aide_rules,
                message_properties,
            )
            .await?;

        // save the session to the disk
        self.save_to_storage(&session, None).await?;
        println!("session_service::code_edit::anchored_edit::finished");
        Ok(())
    }

    pub async fn handle_session_undo(
        &self,
        exchange_id: &str,
        storage_path: String,
    ) -> Result<(), SymbolError> {
        let session_maybe = self.load_from_storage(storage_path.to_owned()).await;
        if session_maybe.is_err() {
            return Ok(());
        }
        let mut session = session_maybe.expect("is_err to hold");
        session = session.undo_including_exchange_id(&exchange_id).await?;
        self.save_to_storage(&session, None).await?;
        Ok(())
    }

    pub async fn move_to_checkpoint(
        &self,
        _session_id: &str,
        exchange_id: &str,
        storage_path: String,
    ) -> Result<(), SymbolError> {
        let session_maybe = self.load_from_storage(storage_path.to_owned()).await;
        if session_maybe.is_err() {
            return Ok(());
        }
        let mut session = session_maybe.expect("is_err to hold");

        // Mark exchanges as deleted or not deleted based on the checkpoint
        session = session.move_to_checkpoint(exchange_id).await?;

        self.save_to_storage(&session, None).await?;
        Ok(())
    }

    pub async fn delete_exchanges_until(
        &self,
        exchange_id: &str,
        storage_path: String,
    ) -> Result<(), SymbolError> {
        let session_maybe = self.load_from_storage(storage_path.to_owned()).await;
        if session_maybe.is_err() {
            return Ok(());
        }
        let mut session = session_maybe.expect("is_err to hold");
        session = session.undo_including_exchange_id(exchange_id).await?;
        self.save_to_storage(&session, None).await?;
        Ok(())
    }

    /// Provied feedback to the exchange
    ///
    /// We can react to this later on and send out either another exchange or something else
    /// but for now we are just reacting to it on our side so we know
    pub async fn feedback_for_exchange(
        &self,
        exchange_id: &str,
        step_index: Option<usize>,
        accepted: bool,
        storage_path: String,
        tool_box: Arc<ToolBox>,
        mut message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        let session_maybe = self.load_from_storage(storage_path.to_owned()).await;
        if session_maybe.is_err() {
            return Ok(());
        }
        let mut session = session_maybe.expect("is_err to hold above");
        session = session
            .react_to_feedback(
                exchange_id,
                step_index,
                accepted,
                message_properties.clone(),
            )
            .await?;
        self.save_to_storage(&session, None).await?;
        let session_id = session.session_id().to_owned();
        if accepted {
            println!(
                "session_service::feedback_for_exchange::exchange_id({})::accepted::({})",
                &exchange_id, accepted,
            );
            // if we have accepted it, then we can help the user move forward
            // there are many conditions we can handle over here
            let is_hot_streak_worthy_message = session
                .get_exchange_by_id(&exchange_id)
                .map(|exchange| exchange.is_hot_streak_worthy_message())
                .unwrap_or_default();
            // if we can't reply to the message return quickly over here
            if !is_hot_streak_worthy_message {
                return Ok(());
            }
            let hot_streak_exchange_id = self
                .tool_box
                .create_new_exchange(session_id.to_owned(), message_properties.clone())
                .await?;

            let cancellation_token = tokio_util::sync::CancellationToken::new();
            self.track_exchange(
                &session_id,
                &hot_streak_exchange_id,
                cancellation_token.clone(),
            )
            .await;
            message_properties = message_properties
                .set_request_id(hot_streak_exchange_id)
                .set_cancellation_token(cancellation_token);

            // now ask the session_service to generate the next most important step
            // which the agent should take over here
            session
                .hot_streak_message(exchange_id, tool_box, message_properties)
                .await?;
        } else {
            // if we rejected the agent message, then we can ask for feedback so we can
            // work on it
        }
        Ok(())
    }

    /// Returns if the exchange was really cancelled
    pub async fn set_exchange_as_cancelled(
        &self,
        storage_path: String,
        exchange_id: String,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<bool, SymbolError> {
        let mut session = self.load_from_storage(storage_path).await.map_err(|e| {
            println!(
                "session_service::set_exchange_as_cancelled::exchange_id({})::error({:?})",
                &exchange_id, e
            );
            e
        })?;

        let send_cancellation_signal = session.has_running_code_edits(&exchange_id);
        println!(
            "session_service::exchange_id({})::should_cancel::({})",
            &exchange_id, send_cancellation_signal
        );

        session = session.set_exchange_as_cancelled(&exchange_id, message_properties);
        self.save_to_storage(&session, None).await?;
        Ok(send_cancellation_signal)
    }

    pub async fn get_mcts_data(
        &self,
        _session_id: &str,
        storage_path: String,
    ) -> Result<SearchTreeMinimal, SymbolError> {
        let session = self.load_from_storage(storage_path).await?;

        // Create a SearchTreeMinimal from the action nodes
        let search_tree = SearchTreeMinimal::from_action_nodes(
            session.action_nodes(),
            session.repo_ref().name.to_owned(),
            "".to_owned(), // No need for MCTS log directory
            "".to_owned(), // No need for MCTS log directory
        );

        Ok(search_tree)
    }

    async fn load_from_storage(&self, storage_path: String) -> Result<Session, SymbolError> {
        println!("loading_session_from_path::{}", &storage_path);
        let content = tokio::fs::read_to_string(storage_path.to_owned())
            .await
            .map_err(|e| SymbolError::IOError(e))?;

        // Trim the content to handle any potential trailing whitespace
        let trimmed_content = content.trim();

        let session: Session = serde_json::from_str(trimmed_content).map_err(|e| {
            SymbolError::IOError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Error deserializing session: {}: {}", storage_path, e),
            ))
        })?;

        Ok(session)
    }

    async fn save_to_storage(
        &self,
        session: &Session,
        mcts_log_directory: Option<String>,
    ) -> Result<(), SymbolError> {
        // print the tree over here for the editor agent
        self.print_tree(session.action_nodes());
        if let Some(mcts_log_directory) = mcts_log_directory {
            let search_tree_minimal = SearchTreeMinimal::from_action_nodes(
                session.action_nodes(),
                session.repo_ref().name.to_owned(),
                mcts_log_directory.to_owned(),
                mcts_log_directory.to_owned(),
            );
            search_tree_minimal
                .save_serialised_graph(&mcts_log_directory, session.session_id())
                .await;
        }

        let serialized = serde_json::to_string(session).unwrap();
        Session::atomic_file_operation(session.storage_path(), serialized).await
    }

    fn print_tree(&self, nodes: &[ActionNode]) {
        println!("MCTS Tree");
        self.print_node(0, nodes, "", false);
    }

    fn print_node(&self, node_index: usize, nodes: &[ActionNode], prefix: &str, is_last: bool) {
        let node = match nodes.get(node_index) {
            Some(n) => n,
            None => return,
        };

        // Build state parameters
        let mut state_params = Vec::new();
        if let Some(action) = &node.action() {
            match action {
                ActionToolParameters::Errored(_err) => {
                    // Show errors in bold red
                    state_params.push("Error".bold().red().to_string());
                }
                ActionToolParameters::Tool(tool) => {
                    let tool_type = tool.tool_input_partial().to_tool_type();
                    let tool_str = match tool.tool_input_partial() {
                        ToolInputPartial::CodeEditorParameters(parameters) => {
                            // Unique colors for each EditorCommand
                            match &parameters.command {
                                EditorCommand::Create => {
                                    "str_replace_editor::create".bright_blue().to_string()
                                }
                                EditorCommand::Insert => {
                                    "str_replace_editor::insert".bright_yellow().to_string()
                                }
                                EditorCommand::StrReplace => {
                                    "str_replace_editor::str_replace".green().to_string()
                                }
                                EditorCommand::UndoEdit => {
                                    "str_replace_editor::undo_edit".white().to_string()
                                }
                                EditorCommand::View => {
                                    "str_replace_editor::view".purple().to_string()
                                }
                            }
                        }
                        ToolInputPartial::FindFile(_) => {
                            tool_type.to_string().bright_cyan().to_string()
                        }
                        ToolInputPartial::CodeEditing(_) => {
                            tool_type.to_string().bright_blue().to_string()
                        }
                        ToolInputPartial::ListFiles(_) => {
                            tool_type.to_string().bright_yellow().to_string()
                        }
                        ToolInputPartial::SearchFileContentWithRegex(_) => {
                            tool_type.to_string().bright_cyan().to_string()
                        }
                        ToolInputPartial::OpenFile(_) => {
                            tool_type.to_string().bright_yellow().to_string()
                        }
                        ToolInputPartial::SemanticSearch(_) => {
                            tool_type.to_string().black().to_string()
                        }
                        ToolInputPartial::LSPDiagnostics(_) => {
                            tool_type.to_string().cyan().to_string()
                        }
                        ToolInputPartial::TerminalCommand(_) => {
                            tool_type.to_string().bright_red().to_string()
                        }
                        ToolInputPartial::AskFollowupQuestions(_) => {
                            tool_type.to_string().white().to_string()
                        }
                        ToolInputPartial::AttemptCompletion(_) => {
                            tool_type.to_string().bright_green().to_string()
                        }
                        ToolInputPartial::RepoMapGeneration(_) => {
                            tool_type.to_string().magenta().to_string()
                        }
                        ToolInputPartial::TestRunner(_) => tool_type.to_string().red().to_string(),
                        ToolInputPartial::Reasoning(_) => {
                            tool_type.to_string().bright_blue().to_string()
                        }
                        ToolInputPartial::ContextCrunching(_) => {
                            tool_type.to_string().bright_blue().to_string()
                        }
                        ToolInputPartial::RequestScreenshot(_) => {
                            tool_type.to_string().bright_white().to_string()
                        }
                        ToolInputPartial::McpTool(_) => tool_type.to_string().cyan().to_string(),
                        ToolInputPartial::Thinking(_) => {
                            tool_type.to_string().bright_blue().to_string()
                        }
                    };
                    state_params.push(tool_str);
                }
            }

            if let Some(observation) = &node.observation() {
                if observation.expect_correction() {
                    state_params.push("expect_correction".to_string().red().to_string());
                }
            }
        }

        // Construct state_info
        let state_info = if !state_params.is_empty() {
            let llm_stats = if let Some(stats) = node.get_llm_usage_statistics() {
                format!(
                    " [tokens: in={:?}, out={:?}]",
                    stats.input_tokens().unwrap_or(0) + stats.cached_input_tokens().unwrap_or(0),
                    stats.output_tokens().unwrap_or(0)
                )
            } else {
                "".to_string()
            };
            format!("Node ({}){}", state_params.join(", "), llm_stats)
        } else {
            format!(
                "Node (){}",
                if let Some(stats) = &node.get_llm_usage_statistics() {
                    format!(
                        " [tokens: in={:?}, out={:?}]",
                        stats.input_tokens().unwrap_or(0)
                            + stats.cached_input_tokens().unwrap_or(0),
                        stats.output_tokens().unwrap_or(0)
                    )
                } else {
                    "".to_string()
                }
            )
        };

        // Construct node_str based on reward
        let node_str = if let Some(reward) = node.reward() {
            format!("[{}]", reward.value())
        } else {
            format!("[-]")
        };

        // Reward string
        let reward_str = if let Some(reward) = node.reward() {
            format!("{}", reward.value())
        } else {
            "0".to_string()
        };

        // Decide which branch to draw
        let branch = if is_last { "└── " } else { "├── " };

        // Print the current node
        if node.is_duplicate() {
            println!(
                "{}",
                format!("{}{}{} {} (dup)", prefix, branch, node_str, state_info).bright_red()
            );
        } else {
            println!(
                "{}{}{} {} (re: {})",
                prefix, branch, node_str, state_info, reward_str,
            );
        }

        // Prepare prefix for child nodes
        let new_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });

        // Get children of the current node
        let child_index = node_index + 1;
        if child_index >= nodes.len() {
            return;
        }

        let is_last_child = child_index == nodes.len() - 1;
        self.print_node(child_index, nodes, &new_prefix, is_last_child);
    }
}

#[derive(Debug)]
pub enum TestGenerateCompletion {
    /// The LLM chose to finish (higher confidence)
    LLMChoseToFinish(String),
    /// Hit the maximum iteration limit (lower confidence)
    HitIterationLimit(String),
}
