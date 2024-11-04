//! Creates the service which handles saving the session and extending it

use std::{collections::HashMap, sync::Arc};

use tokio::{io::AsyncWriteExt, sync::Mutex};
use tokio_util::sync::CancellationToken;

use crate::{
    agentic::{
        symbol::{
            errors::SymbolError, events::message_event::SymbolEventMessageProperties,
            manager::SymbolManager, scratch_pad::ScratchPadAgent, tool_box::ToolBox,
            ui_event::UIEventWithID,
        },
        tool::plan::service::PlanService,
    },
    chunking::text_document::Range,
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
                message_properties,
            )
            .await?;

        // save the session to the disk
        self.save_to_storage(&session).await?;
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
        self.save_to_storage(&session).await?;
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
                plan_storage_path,
                self.tool_box.clone(),
                self.symbol_manager.clone(),
                message_properties,
            )
            .await?;
        // save the session to the disk
        self.save_to_storage(&session).await?;

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

        // add an exchange that we are going to genrate a plan over here
        session = session.plan(exchange_id.to_owned(), query, user_context);
        self.save_to_storage(&session).await?;

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
                plan_storage_path,
                self.tool_box.clone(),
                self.symbol_manager.clone(),
                message_properties,
            )
            .await?;
        // save the session to the disk
        self.save_to_storage(&session).await?;

        println!("session_service::plan_generation::stop");
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
            .perform_agentic_editing(scratch_pad_agent, root_directory, message_properties)
            .await?;

        // save the session to the disk
        self.save_to_storage(&session).await?;
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
            .perform_anchored_edit(exchange_id, scratch_pad_agent, message_properties)
            .await?;

        // save the session to the disk
        self.save_to_storage(&session).await?;
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
        self.save_to_storage(&session).await?;
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
        tool_box: Arc<ToolBox>,
        accepted: bool,
        storage_path: String,
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

        // this is a hack
        if accepted {
            let new_exchange = tool_box
                .create_new_exchange(session.session_id().to_owned(), message_properties.clone())
                .await?;
            message_properties = message_properties.set_request_id(new_exchange.to_owned());

            let last_exchange = match session.last_exchange() {
                Some(x) => x,
                None => return Ok(()),
            };

            let session_chat_message = last_exchange.to_conversation_message().await;
            let _last_message = session_chat_message.message();

            let Some(last_file) = session.find_last_edited_file() else {
                return Ok(());
            };

            let diags = tool_box
                .get_lsp_diagnostics_for_files(
                    vec![last_file.clone()],
                    message_properties.clone(),
                    true,
                )
                .await;

            let Ok(diags) = diags else { return Ok(()) };

            dbg!(&diags);

            let _ = message_properties
                .ui_sender()
                .send(UIEventWithID::chat_event(
                    session.session_id().to_owned(),
                    new_exchange,
                    "".to_owned(),
                    Some(format!("last edited file: {}", &last_file).to_owned()),
                ));
        }
        self.save_to_storage(&session).await?;
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
        self.save_to_storage(&session).await?;
        Ok(send_cancellation_signal)
    }

    async fn load_from_storage(&self, storage_path: String) -> Result<Session, SymbolError> {
        let content = tokio::fs::read_to_string(storage_path.to_owned())
            .await
            .map_err(|e| SymbolError::IOError(e))?;

        let session: Session = serde_json::from_str(&content).expect(&format!(
            "converting to session from json is okay: {storage_path}"
        ));
        Ok(session)
    }

    async fn save_to_storage(&self, session: &Session) -> Result<(), SymbolError> {
        let serialized = serde_json::to_string(session).unwrap();
        let mut file = tokio::fs::File::create(session.storage_path())
            .await
            .map_err(|e| SymbolError::IOError(e))?;
        file.write_all(serialized.as_bytes())
            .await
            .map_err(|e| SymbolError::IOError(e))?;
        Ok(())
    }
}
