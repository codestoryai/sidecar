//! We can create a new session over here and its composed of exchanges
//! The exchanges can be made by the human or the agent

use std::sync::Arc;

use futures::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::{
    agentic::{
        symbol::{
            errors::SymbolError,
            events::{
                edit::SymbolToEdit,
                human::HumanAgenticRequest,
                message_event::{SymbolEventMessage, SymbolEventMessageProperties},
            },
            identifier::SymbolIdentifier,
            manager::SymbolManager,
            scratch_pad::ScratchPadAgent,
            tool_box::ToolBox,
            tool_properties::ToolProperties,
            types::SymbolEventRequest,
            ui_event::UIEventWithID,
        },
        tool::{
            input::ToolInput,
            plan::{
                generator::{Step, StepSenderEvent},
                service::PlanService,
            },
            r#type::Tool,
        },
    },
    chunking::text_document::Range,
    repo::types::RepoRef,
    user_context::types::UserContext,
};

use super::chat::{SessionChatClientRequest, SessionChatMessage};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum AideAgentMode {
    Edit = 1,
    Plan = 2,
    Chat = 3,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AideEditMode {
    Anchored = 1,
    Agentic = 2,
}

/// The exchange can be in one of the states
///
/// Its either that the edits made were accepted or rejected
/// it could also have been cancelled by the user
/// Default when its created is in running state
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ExchangeState {
    Accepted,
    Rejected,
    Cancelled,
    Running,
    UserMessage,
}

impl Default for ExchangeState {
    fn default() -> Self {
        ExchangeState::Running
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ExchangeType {
    HumanChat(ExchangeTypeHuman),
    AgentChat(ExchangeTypeAgent),
    // what do we store over here for the anchored edit, it can't just be the
    // user query? we probably have to store the snippet we were trying to edit
    // as well
    Edit(ExchangeTypeEdit),
    Plan(ExchangeTypePlan),
}

// TODO(codestory): The user is probably going to add more context over here as they
// keep iterating with their requests over here, we have to do something about it
// or we can keep it simple and just make it so that we store the previous iterations over here
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExchangeTypePlan {
    previous_queries: Vec<String>,
    query: String,
    user_context: UserContext,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExchangeEditInformationAgentic {
    query: String,
    codebase_search: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExchangeEditInformationAnchored {
    query: String,
    fs_file_path: String,
    range: Range,
    selection_context: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ExchangeEditInformation {
    Agentic(ExchangeEditInformationAgentic),
    Anchored(ExchangeEditInformationAnchored),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExchangeTypeEdit {
    information: ExchangeEditInformation,
    user_context: UserContext,
    exchange_type: AideEditMode,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExchangeTypeHuman {
    query: String,
    user_context: UserContext,
    project_labels: Vec<String>,
    repo_ref: RepoRef,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExchangeReplyAgentPlan {
    plan_steps: Vec<Step>,
    // plan discarded over here represents the fact that the plan we CANCELLED
    // it had other meanings but thats what we are going with now 🔫
    plan_discarded: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExchangeReplyAgentChat {
    reply: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExchangeReplyAgentEdit {
    edits_made_diff: String,
    accepted: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ExchangeReplyAgent {
    Plan(ExchangeReplyAgentPlan),
    Chat(ExchangeReplyAgentChat),
    Edit(ExchangeReplyAgentEdit),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExchangeTypeAgent {
    reply: ExchangeReplyAgent,
    /// This points to the exchange id which we are replying to as the agent
    parent_exchange_id: String,
}

impl ExchangeTypeAgent {
    fn chat_reply(reply: String, parent_exchange_id: String) -> Self {
        Self {
            reply: ExchangeReplyAgent::Chat(ExchangeReplyAgentChat { reply }),
            parent_exchange_id,
        }
    }

    fn plan_reply(steps: Vec<Step>, parent_exchange_id: String) -> Self {
        Self {
            reply: ExchangeReplyAgent::Plan(ExchangeReplyAgentPlan {
                plan_steps: steps,
                plan_discarded: false,
            }),
            parent_exchange_id,
        }
    }

    fn edits_reply(edits_made: String, parent_exchange_id: String) -> Self {
        Self {
            reply: ExchangeReplyAgent::Edit(ExchangeReplyAgentEdit {
                edits_made_diff: edits_made,
                accepted: false,
            }),
            parent_exchange_id,
        }
    }
}

impl ExchangeTypeHuman {
    pub fn new(
        query: String,
        user_context: UserContext,
        project_labels: Vec<String>,
        repo_ref: RepoRef,
    ) -> Self {
        Self {
            query,
            user_context,
            project_labels,
            repo_ref,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Exchange {
    exchange_id: String,
    exchange_type: ExchangeType,
    #[serde(default)]
    exchange_state: ExchangeState,
}

impl Exchange {
    pub fn exchange_id(&self) -> &str {
        &self.exchange_id
    }

    fn human_chat(
        exchange_id: String,
        query: String,
        user_context: UserContext,
        project_labels: Vec<String>,
        repo_ref: RepoRef,
    ) -> Self {
        Self {
            exchange_id,
            exchange_type: ExchangeType::HumanChat(ExchangeTypeHuman::new(
                query,
                user_context,
                project_labels,
                repo_ref,
            )),
            exchange_state: ExchangeState::UserMessage,
        }
    }

    fn plan_request(exchange_id: String, query: String, user_context: UserContext) -> Self {
        Self {
            exchange_id,
            exchange_type: ExchangeType::Plan(ExchangeTypePlan {
                previous_queries: vec![],
                query,
                user_context,
            }),
            exchange_state: ExchangeState::UserMessage,
        }
    }

    fn agentic_edit(
        exchange_id: String,
        query: String,
        codebase_search: bool,
        user_context: UserContext,
    ) -> Self {
        Self {
            exchange_id,
            exchange_type: ExchangeType::Edit(ExchangeTypeEdit {
                information: ExchangeEditInformation::Agentic(ExchangeEditInformationAgentic {
                    query,
                    codebase_search,
                }),
                user_context,
                exchange_type: AideEditMode::Agentic,
            }),
            exchange_state: ExchangeState::UserMessage,
        }
    }

    fn anchored_edit(
        exchange_id: String,
        query: String,
        user_context: UserContext,
        range: Range,
        fs_file_path: String,
        selection_context: String,
    ) -> Self {
        Self {
            exchange_id,
            exchange_type: ExchangeType::Edit(ExchangeTypeEdit {
                information: ExchangeEditInformation::Anchored(ExchangeEditInformationAnchored {
                    query,
                    fs_file_path,
                    range,
                    selection_context,
                }),
                user_context,
                exchange_type: AideEditMode::Anchored,
            }),
            exchange_state: ExchangeState::UserMessage,
        }
    }

    fn agent_chat_reply(parent_exchange_id: String, exchange_id: String, message: String) -> Self {
        Self {
            exchange_id,
            exchange_type: ExchangeType::AgentChat(ExchangeTypeAgent::chat_reply(
                message,
                parent_exchange_id,
            )),
            exchange_state: ExchangeState::Running,
        }
    }

    fn agent_plan_reply(parent_exchange_id: String, exchange_id: String, steps: Vec<Step>) -> Self {
        Self {
            exchange_id,
            exchange_type: ExchangeType::AgentChat(ExchangeTypeAgent::plan_reply(
                steps,
                parent_exchange_id,
            )),
            exchange_state: ExchangeState::Running,
        }
    }

    fn agent_edits_reply(
        parent_exchange_id: String,
        exchange_id: String,
        edits_response: String,
    ) -> Self {
        Self {
            exchange_id,
            exchange_type: ExchangeType::AgentChat(ExchangeTypeAgent::edits_reply(
                edits_response,
                parent_exchange_id,
            )),
            exchange_state: ExchangeState::Running,
        }
    }

    fn set_completion_status(mut self, accetped: bool) -> Self {
        if accetped {
            self.exchange_state = ExchangeState::Accepted;
        } else {
            self.exchange_state = ExchangeState::Rejected;
        }
        self
    }

    /// If the exchange has been left open and has not finished yet
    fn is_open(&self) -> bool {
        matches!(self.exchange_state, ExchangeState::Running)
            && matches!(self.exchange_type, ExchangeType::AgentChat(_))
    }

    /// Check if this is agent reply
    fn is_agent_work(&self) -> bool {
        matches!(self.exchange_type, ExchangeType::AgentChat(_))
    }

    fn is_still_running(&self) -> bool {
        matches!(self.exchange_state, ExchangeState::Running)
    }

    /// Assume that we will implement this later but we still have code edits
    /// everywhere
    fn has_code_edits(&self) -> bool {
        true
    }

    fn set_exchange_as_cancelled(&mut self) {
        self.exchange_state = ExchangeState::Cancelled;
    }

    /// Convert the exchange to a session chat message so we can send it over
    /// for inference
    ///
    /// We can have consecutive human messages now on every API so this is no
    /// longer a big worry
    pub async fn to_conversation_message(&self) -> SessionChatMessage {
        match &self.exchange_type {
            ExchangeType::HumanChat(ref chat_message) => {
                // TODO(skcd): Figure out caching etc later on
                let prompt = chat_message.query.to_owned();
                SessionChatMessage::user(prompt)
            }
            ExchangeType::AgentChat(ref chat_message) => {
                // This completely breaks we have to figure out how to covert
                // the various types of exchanges to a string here for passing
                // around as context
                let reply = chat_message.reply.clone();
                match reply {
                    ExchangeReplyAgent::Chat(chat_reply) => {
                        SessionChatMessage::assistant(chat_reply.reply.to_owned())
                    }
                    ExchangeReplyAgent::Edit(edit_reply) => {
                        if edit_reply.accepted {
                            SessionChatMessage::assistant(edit_reply.edits_made_diff.to_owned())
                        } else {
                            let edits_made = edit_reply.edits_made_diff.to_owned();
                            SessionChatMessage::assistant(format!(
                                r#"I made the following edits and the user REJECTED them
{edits_made}"#
                            ))
                        }
                    }
                    ExchangeReplyAgent::Plan(plan_reply) => {
                        dbg!(&plan_reply);
                        if plan_reply.plan_discarded {
                            SessionChatMessage::assistant(
                                "The Plan I came up with was REJECTED by the user".to_owned(),
                            )
                        } else {
                            let plan_steps = plan_reply
                                .plan_steps
                                .into_iter()
                                .map(|step| {
                                    let step_title = step.title.to_owned();
                                    let step_description = step.description();
                                    let files_to_edit = step
                                        .file_to_edit()
                                        .unwrap_or("File to edit not present".to_owned());
                                    format!(
                                        r#"<step>
<files_to_edit>
<file>
{files_to_edit}
</file>
</files_to_edit>
<title>
{step_title}
</title>
<changes>
{step_description}
</changes>
</step>"#
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            SessionChatMessage::assistant(format!(
                                "I came up with the plan below and the user was happy
{plan_steps}"
                            ))
                        }
                    }
                }
            }
            ExchangeType::Plan(ref plan) => {
                let user_query = &plan.query;
                SessionChatMessage::user(format!(
                    r#"I want a plan of edits to help solve this:
{user_query}"#
                ))
            }
            ExchangeType::Edit(ref anchored_edit) => {
                let edit_information = &anchored_edit.information;
                let user_query = match edit_information {
                    ExchangeEditInformation::Agentic(agentic_edit) => {
                        let query = agentic_edit.query.to_owned();
                        format!(
                            r#"I want you to perform edits for my query:
<query>
{query}
</query>"#
                        );
                        query
                    }
                    ExchangeEditInformation::Anchored(anchored_edit) => {
                        let fs_file_path = anchored_edit.fs_file_path.to_owned();
                        let start_line = anchored_edit.range.start_line();
                        let end_line = anchored_edit.range.end_line();
                        let location = format!(r#"{fs_file_path}-{start_line}:{end_line}"#);
                        let query = anchored_edit.query.to_owned();
                        format!(
                            r#"I want to perform edits at {location}
<query>
{query}
</query>"#
                        )
                    }
                };
                SessionChatMessage::user(user_query)
            }
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Session {
    session_id: String,
    project_labels: Vec<String>,
    repo_ref: RepoRef,
    exchanges: Vec<Exchange>,
    storage_path: String,
    global_running_user_context: UserContext,
}

impl Session {
    pub fn new(
        session_id: String,
        project_labels: Vec<String>,
        repo_ref: RepoRef,
        storage_path: String,
        global_running_user_context: UserContext,
    ) -> Self {
        Self {
            session_id,
            project_labels,
            repo_ref,
            exchanges: vec![],
            storage_path,
            global_running_user_context,
        }
    }

    pub fn find_last_edited_file(&self) -> Option<String> {
        let mut last_edited_file: Option<String> = None;

        // Iterate through exchanges in reverse order (newest to oldest)
        for exchange in self.exchanges.iter().rev() {
            // Check if the exchange is an agent reply (contains edits or plans)
            match &exchange.exchange_type {
                ExchangeType::AgentChat(agent_exchange) => {
                    // Check the type of agent reply
                    match &agent_exchange.reply {
                        ExchangeReplyAgent::Plan(plan_reply) => {
                            // Iterate through plan steps in reverse order (last step to first)
                            for step in plan_reply.plan_steps.iter().rev() {
                                // Check if the step contains a file to edit
                                if let Some(file) = step.file_to_edit() {
                                    // Found a file edited in this step. This is the latest so far.
                                    last_edited_file = Some(file.to_owned());
                                    // Break the inner loop, no need to look at earlier steps in this plan
                                    break;
                                }
                            }
                        }
                        ExchangeReplyAgent::Edit(edit_reply) => {
                            // Only consider accepted edits as they result in actual file changes
                            if edit_reply.accepted {
                                // Parse the diff to identify the edited file
                                // *** IMPORTANT: Adapt this to your diff format ***
                                // Example: assuming diff starts with "file: <filepath>"
                                if let Some(file_prefix) =
                                    edit_reply.edits_made_diff.strip_prefix("file: ")
                                {
                                    if let Some(filepath) = file_prefix.split_whitespace().next() {
                                        // Found the edited file from the diff
                                        last_edited_file = Some(filepath.to_owned());
                                        // Break the inner loop, as we've found the last edited file
                                        break;
                                    }
                                }
                            }
                        }
                        _ => {} // Ignore chat replies (they don't contain file edits)
                    }
                }
                _ => {} // Ignore human exchanges (they don't contain edits)
            }
            // If we've found a file in this exchange, it's the last edited one,
            // so break the outer loop as well.
            if last_edited_file.is_some() {
                break;
            }
        }

        last_edited_file
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn storage_path(&self) -> &str {
        &self.storage_path
    }

    pub fn exchanges(&self) -> usize {
        self.exchanges.len()
    }

    fn find_exchange_by_id(&self, exchange_id: &str) -> Option<&Exchange> {
        self.exchanges
            .iter()
            .find(|exchange| &exchange.exchange_id == exchange_id)
    }

    fn find_exchange_by_id_mut(&mut self, exchange_id: &str) -> Option<&mut Exchange> {
        self.exchanges
            .iter_mut()
            .find(|exchange| &exchange.exchange_id == exchange_id)
    }

    /// Finds the exchange we are interested in and mutates the previous queries
    /// and the current query
    pub fn plan_iteration(
        mut self,
        exchange_id: String,
        query: String,
        user_context: UserContext,
    ) -> Session {
        self.global_running_user_context = self
            .global_running_user_context
            .merge_user_context(user_context.clone());
        let exchange_to_change = self
            .exchanges
            .iter_mut()
            .find(|exchange| exchange.exchange_id == exchange_id);
        if let Some(exchange_to_change) = exchange_to_change {
            match &mut exchange_to_change.exchange_type {
                ExchangeType::Plan(plan_exchange) => {
                    let mut previous_queries = plan_exchange.previous_queries.to_vec();
                    previous_queries.push(plan_exchange.query.to_owned());
                    plan_exchange.query = query;
                    plan_exchange.previous_queries = previous_queries;
                    plan_exchange.user_context = user_context;
                }
                _ => {}
            }
        }
        self
    }

    pub fn plan(
        mut self,
        exchange_id: String,
        query: String,
        user_context: UserContext,
    ) -> Session {
        self.global_running_user_context = self
            .global_running_user_context
            .merge_user_context(user_context.clone());
        let exchange = Exchange::plan_request(exchange_id, query, user_context);
        self.exchanges.push(exchange);
        self
    }

    pub fn get_parent_exchange_id(&self, exchange_id: &str) -> Option<Exchange> {
        self.exchanges
            .iter()
            .find(|exchange| &exchange.exchange_id == exchange_id)
            .map(|exchange| match &exchange.exchange_type {
                ExchangeType::AgentChat(ref agent_chat) => {
                    Some(agent_chat.parent_exchange_id.to_owned())
                }
                _ => None,
            })
            .flatten()
            .map(|parent_exchange_id| self.get_exchange_by_id(&parent_exchange_id))
            .flatten()
    }

    pub fn get_exchange_by_id(&self, exchange_id: &str) -> Option<Exchange> {
        self.exchanges
            .iter()
            .find(|exchange| &exchange.exchange_id == exchange_id)
            .cloned()
    }

    pub fn agentic_edit(
        mut self,
        exchange_id: String,
        query: String,
        user_context: UserContext,
        codebase_search: bool,
    ) -> Session {
        self.global_running_user_context = self
            .global_running_user_context
            .merge_user_context(user_context.clone());
        let exchange = Exchange::agentic_edit(exchange_id, query, codebase_search, user_context);
        self.exchanges.push(exchange);
        self
    }

    pub fn anchored_edit(
        mut self,
        exchange_id: String,
        query: String,
        user_context: UserContext,
        range: Range,
        fs_file_path: String,
        file_content_in_selection: String,
    ) -> Session {
        self.global_running_user_context = self
            .global_running_user_context
            .merge_user_context(user_context.clone());
        let exchange = Exchange::anchored_edit(
            exchange_id,
            query,
            user_context,
            range,
            fs_file_path,
            file_content_in_selection,
        );
        self.exchanges.push(exchange);
        self
    }

    pub fn human_message(
        mut self,
        exchange_id: String,
        human_message: String,
        user_context: UserContext,
        project_labels: Vec<String>,
        repo_ref: RepoRef,
    ) -> Session {
        self.global_running_user_context = self
            .global_running_user_context
            .merge_user_context(user_context.clone());
        let exchange = Exchange::human_chat(
            exchange_id,
            human_message,
            user_context,
            project_labels,
            repo_ref,
        );
        self.exchanges.push(exchange);
        self
    }

    pub fn last_exchange(&self) -> Option<&Exchange> {
        self.exchanges.last()
    }

    pub async fn undo_including_exchange_id(
        mut self,
        exchange_id: &str,
    ) -> Result<Self, SymbolError> {
        // keep grabbing the exchanges until we hit the exchange_id we are interested in
        // over  here, that become our new exchange
        let new_exchanges = self
            .exchanges
            .into_iter()
            .take_while(|exchange| &exchange.exchange_id != exchange_id)
            .collect::<Vec<_>>();
        self.exchanges = new_exchanges;
        Ok(self)
    }

    pub async fn react_to_feedback(
        mut self,
        exchange_id: &str,
        step_index: Option<usize>,
        accepted: bool,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Self, SymbolError> {
        // We have to do a couple of things here, since for plans we might have partial
        // acceptance
        // - find the step list of the plan until which we have accepted the changes
        // - if its an anchored edit then mark it completely accepted or rejected
        // Here first we make sure that an exchange of the form exists
        // if it does we mark that exchange as closed and also update its state
        self.exchanges = self
            .exchanges
            .into_iter()
            .map(|exchange| {
                if &exchange.exchange_id == exchange_id {
                    // we have an exchange over here matching our id
                    // now we need to carefully understand if its a plan or if its an edit
                    // if its a plan we should accept it until that step and discard all the steps
                    // post that index
                    match exchange.exchange_type {
                        ExchangeType::AgentChat(agent_exchange) => {
                            let parent_exchange_id = agent_exchange.parent_exchange_id.to_owned();
                            let exchange_reply = match agent_exchange.reply {
                                ExchangeReplyAgent::Plan(mut plan_step) => {
                                    if let Some(step_index) = step_index {
                                        // now here only keep the steps until the index we are interested in
                                        if step_index == 0 {
                                            plan_step.plan_discarded = true;
                                        } else {
                                            plan_step.plan_steps.truncate(step_index + 1);
                                        }
                                    }
                                    ExchangeReplyAgent::Plan(plan_step)
                                }
                                ExchangeReplyAgent::Edit(mut edit_step) => {
                                    edit_step.accepted = accepted;
                                    ExchangeReplyAgent::Edit(edit_step)
                                }
                                ExchangeReplyAgent::Chat(chat_step) => {
                                    ExchangeReplyAgent::Chat(chat_step)
                                }
                            };
                            Exchange {
                                exchange_id: exchange_id.to_owned(),
                                exchange_type: ExchangeType::AgentChat(ExchangeTypeAgent {
                                    reply: exchange_reply,
                                    parent_exchange_id,
                                }),
                                exchange_state: exchange.exchange_state,
                            }
                        }
                        _ => exchange,
                    }
                } else {
                    exchange
                }
            })
            .collect();

        let exchange_to_react = self
            .exchanges
            .iter()
            .find(|exchange| &exchange.exchange_id == exchange_id)
            .map(|exchange| match &exchange.exchange_type {
                ExchangeType::AgentChat(agentic_chat) => match agentic_chat.reply {
                    ExchangeReplyAgent::Chat(_) => None,
                    ExchangeReplyAgent::Edit(_) => Some(AideAgentMode::Edit),
                    ExchangeReplyAgent::Plan(_) => Some(AideAgentMode::Plan),
                },
                _ => None,
            })
            .flatten();

        // give feedback to the editor that our state has changed
        if accepted {
            if matches!(exchange_to_react, Some(AideAgentMode::Plan)) {
                let _ = message_properties
                    .ui_sender()
                    .send(UIEventWithID::plan_as_accepted(
                        self.session_id.to_owned(),
                        exchange_id.to_owned(),
                    ));
            }
            if matches!(exchange_to_react, Some(AideAgentMode::Edit)) {
                let _ = message_properties
                    .ui_sender()
                    .send(UIEventWithID::edits_accepted(
                        self.session_id.to_owned(),
                        exchange_id.to_owned(),
                    ));
            }
        } else {
            if matches!(exchange_to_react, Some(AideAgentMode::Plan)) {
                let _ = message_properties
                    .ui_sender()
                    .send(UIEventWithID::plan_as_cancelled(
                        self.session_id.to_owned(),
                        exchange_id.to_owned(),
                    ));
            }
            if matches!(exchange_to_react, Some(AideAgentMode::Edit)) {
                let _ = message_properties.ui_sender().send(
                    UIEventWithID::edits_cancelled_in_exchange(
                        self.session_id.to_owned(),
                        exchange_id.to_owned(),
                    ),
                );
            }
        }

        // now close the exchange
        println!("session::react_to_feedback::finished_exchange");
        let _ = message_properties
            .ui_sender()
            .send(UIEventWithID::finished_exchange(
                self.session_id.to_owned(),
                message_properties.request_id_str().to_owned(),
            ));
        Ok(self)
    }

    /// This reacts to the last message and generates the reply for the user to handle
    ///
    /// we should have a way to sync this up with a queue based system so we react to events
    /// one after another
    pub async fn reply_to_last_exchange(
        self,
        exchange_reply: AideAgentMode,
        tool_box: Arc<ToolBox>,
        parent_exchange_id: String,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Self, SymbolError> {
        let last_exchange = self.last_exchange();
        if last_exchange.is_none() {
            return Ok(self);
        }

        // plan and edit todos are intentional. Should never be hit, but double check @skcd
        match exchange_reply {
            AideAgentMode::Chat => {
                self.chat_reply(tool_box, parent_exchange_id, message_properties)
                    .await
            }
            AideAgentMode::Plan => {
                todo!("plan branch")
            }
            AideAgentMode::Edit => {
                todo!("edit branch not supported")
            }
        }
    }

    /// This sends back a reply to the user message, using the context we have from before
    async fn chat_reply(
        self,
        tool_box: Arc<ToolBox>,
        parent_exchange_id: String,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Self, SymbolError> {
        println!("session::chat_reply");
        // over here we want to convert all the previous exchanges to a context prompt
        // and then generate the appropriate things required
        let last_exchange = self.last_exchange();
        if last_exchange.is_none() {
            return Ok(self);
        }
        let last_exchange = last_exchange.expect("is_none to hold").clone();

        // Now that we have a new response exchange id we want to start streaming the reply back
        // to the user
        let last_exchange_type = last_exchange.exchange_type;
        match last_exchange_type {
            ExchangeType::HumanChat(_) => {
                self.human_chat_message_reply(tool_box, parent_exchange_id, message_properties)
                    .await
            }
            ExchangeType::AgentChat(_agent_message) => {
                todo!("figure out what to do over here")
            }
            ExchangeType::Edit(_edit) => {
                todo!("figure out what to do over here")
            }
            ExchangeType::Plan(_plan) => {
                todo!("figure out what to do over here")
            }
        }
    }

    /// Create the stream which will reply to the user
    async fn human_chat_message_reply(
        mut self,
        tool_box: Arc<ToolBox>,
        parent_exchange_id: String,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Session, SymbolError> {
        println!("session::human_chat_message_reply");
        // take everything until the exchange id of the message we are supposed to
        // reply to
        let mut converted_messages = vec![];
        for previous_message in self.exchanges.iter() {
            converted_messages.push(previous_message.to_conversation_message().await);
        }

        let exchange_id = message_properties.request_id_str().to_owned();
        let access_token = message_properties.access_token().to_owned();

        let tool_input = SessionChatClientRequest::new(
            tool_box
                .recently_edited_files(Default::default(), message_properties.clone())
                .await?,
            self.global_running_user_context.clone(),
            converted_messages,
            self.repo_ref.clone(),
            self.project_labels.to_vec(),
            self.session_id.to_owned(),
            exchange_id.to_owned(),
            message_properties.ui_sender(),
            message_properties.cancellation_token(),
            access_token,
        );
        let chat_output = tool_box
            .tools()
            .invoke(ToolInput::ContextDrivenChatReply(tool_input))
            .await
            .map_err(|e| SymbolError::ToolError(e))?
            .context_drive_chat_reply()
            .ok_or(SymbolError::WrongToolOutput)?
            .reply();
        self.exchanges.push(Exchange::agent_chat_reply(
            parent_exchange_id,
            exchange_id.to_owned(),
            chat_output,
        ));
        let ui_sender = message_properties.ui_sender();
        // finsihed the exchange here since we have replied already
        let _ = ui_sender.send(UIEventWithID::finished_exchange(
            self.session_id.to_owned(),
            exchange_id,
        ));
        Ok(self)
    }

    /// We want to make sure that any open exchanges are accepted as we make
    /// progress towards are current exchange
    pub fn accept_open_exchanges_if_any(
        mut self,
        message_properties: SymbolEventMessageProperties,
    ) -> Self {
        let exchanges_to_close = self
            .exchanges
            .iter()
            .filter_map(|exchange| {
                if exchange.is_open() {
                    Some(exchange.exchange_id.to_owned())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        exchanges_to_close.into_iter().for_each(|exchange_id| {
            // mark the exchange as accepted
            let _ = message_properties
                .ui_sender()
                .send(UIEventWithID::edits_accepted(
                    self.session_id.to_owned(),
                    exchange_id.to_owned(),
                ));
            // mark the exchange as closed
            let _ = message_properties
                .ui_sender()
                .send(UIEventWithID::finished_exchange(
                    self.session_id.to_owned(),
                    exchange_id,
                ));
        });

        // now update all our exchanges to accepted
        self.exchanges = self
            .exchanges
            .into_iter()
            .map(|exchange| {
                if exchange.is_open() {
                    exchange.set_completion_status(true)
                } else {
                    exchange
                }
            })
            .collect();

        self
    }

    /// We have to map the plan revert exchange-id over here to be similar to
    /// the previous plan exchange-id, doing this will allow us to make sure
    /// that we are able to keep track of the edits properly
    pub async fn perform_plan_revert(
        self,
        plan_service: PlanService,
        previous_plan_exchange_id: String,
        step_index: usize,
        tool_box: Arc<ToolBox>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Self, SymbolError> {
        let original_plan = plan_service
            .load_plan_from_id(
                &plan_service.generate_unique_plan_id(&self.session_id, &previous_plan_exchange_id),
            )
            .await
            .map_err(|e| SymbolError::IOError(e))?;

        let exchange_id = message_properties.request_id_str().to_owned();

        if step_index == 0 {
            let ui_sender = message_properties.ui_sender();
            // revert the changes back by talking to the editor
            let _ = tool_box
                .undo_changes_made_during_session(
                    self.session_id.to_owned(),
                    previous_plan_exchange_id.to_owned(),
                    Some(step_index),
                    message_properties.clone(),
                )
                .await;

            let reply =
                "I have reverted the full plan, let me know how I can be of help?".to_owned();
            let _ = ui_sender.send(UIEventWithID::chat_event(
                self.session_id.to_owned(),
                exchange_id.to_owned(),
                reply.to_owned(),
                Some(reply.to_owned()),
            ));

            // update our exchanges and add what we did
            // TODO(skcd): Not sure what to do over here
            // self.exchanges.push(Exchange::agent_reply(
            //     message_properties.request_id_str().to_owned(),
            //     reply,
            //     AideAgentMode::Plan,
            // ));

            // now close the exchange
            let _ = ui_sender.send(UIEventWithID::finished_exchange(
                self.session_id.to_owned(),
                exchange_id,
            ));
        } else {
            let updated_plan = original_plan.drop_plan_steps(step_index);
            let ui_sender = message_properties.ui_sender();
            // send all the updated plan steps to the exchange
            updated_plan
                .steps()
                .into_iter()
                .enumerate()
                .filter(|(idx, _)| *idx < step_index)
                .for_each(|(idx, plan_step)| {
                    let _ = ui_sender.send(UIEventWithID::plan_complete_added(
                        self.session_id.to_owned(),
                        previous_plan_exchange_id.to_owned(),
                        idx,
                        plan_step.files_to_edit().to_vec(),
                        plan_step.title().to_owned(),
                        plan_step.description().to_owned(),
                    ));
                });

            // revert the changes back by talking to the editor
            let _ = tool_box
                .undo_changes_made_during_session(
                    self.session_id.to_owned(),
                    previous_plan_exchange_id.to_owned(),
                    Some(step_index),
                    message_properties.clone(),
                )
                .await;

            // now send a message to the exchange telling that we have reverted
            // the changes
            let _ = ui_sender.send(UIEventWithID::chat_event(
                self.session_id.to_owned(),
                exchange_id.to_owned(),
                "I have reverted the changes made by the plan".to_owned(),
                Some("I have reverted the changes made by the plan".to_owned()),
            ));

            // update our exchanges and add what we did
            // TODO(skcd): Not sure what to do over here
            // self.exchanges.push(Exchange::agent_reply(
            //     message_properties.request_id_str().to_owned(),
            //     "I have reverted the changes made by the plan".to_owned(),
            //     AideAgentMode::Plan,
            // ));

            // now close the exchange
            let _ = ui_sender.send(UIEventWithID::finished_exchange(
                self.session_id.to_owned(),
                exchange_id,
            ));
        }

        Ok(self)
    }

    pub async fn perform_plan_generation(
        mut self,
        plan_service: PlanService,
        plan_id: String,
        parent_exchange_id: String,
        exchange_in_focus: Option<Exchange>,
        plan_storage_path: String,
        tool_box: Arc<ToolBox>,
        symbol_manager: Arc<SymbolManager>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Self, SymbolError> {
        // one of the bugs here is that the last exchange is now of the agent
        // replying to the user, so the exchange type is different completely
        // so can we pass it top down instead of getting the exchange here implicitly
        if let Some(Exchange {
            exchange_id: _,
            exchange_type:
                ExchangeType::Plan(ExchangeTypePlan {
                    // when doing plan generation we are looking at the previous
                    // queries
                    previous_queries,
                    query,
                    user_context: _,
                }),
            exchange_state: _,
        }) = exchange_in_focus
        {
            // take everything until the exchange id of the message we are supposed to
            // reply to
            let mut converted_messages = vec![];
            for previous_message in self.exchanges.iter() {
                converted_messages.push(previous_message.to_conversation_message().await);
            }
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            let mut stream_receiver =
                tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);

            // we should set our exchange over here
            self.exchanges.push(Exchange::agent_plan_reply(
                parent_exchange_id.to_owned(),
                message_properties.request_id_str().to_owned(),
                vec![],
            ));
            self.save_to_storage().await?;
            let mut agent_reply_exchange = self
                .exchanges
                .iter_mut()
                .find(|exchange| &exchange.exchange_id == message_properties.request_id_str());

            let exchange_id = message_properties.request_id_str().to_owned();
            let session_id = self.session_id.to_owned();

            // send a message over here that inference has started on the plan
            let _ = message_properties
                .ui_sender()
                .send(UIEventWithID::inference_started(
                    message_properties.root_request_id().to_owned(),
                    message_properties.request_id_str().to_owned(),
                ));

            // send a message over here that the plan is regenerating
            let _ = message_properties
                .ui_sender()
                .send(UIEventWithID::plan_regeneration(
                    message_properties.root_request_id().to_owned(),
                    message_properties.request_id_str().to_owned(),
                ));

            let ui_sender = message_properties.ui_sender();
            let _ = ui_sender.send(UIEventWithID::start_plan_generation(
                session_id.to_owned(),
                exchange_id.to_owned(),
            ));

            let cloned_message_properties = message_properties.clone();
            let cloned_plan_service = plan_service.clone();
            let global_running_context = self.global_running_user_context.clone();
            let _plan = tokio::spawn(async move {
                cloned_plan_service
                    .create_plan(
                        plan_id,
                        query.to_owned(),
                        previous_queries.to_vec(),
                        // always send the global running context over here
                        global_running_context,
                        converted_messages,
                        false,
                        plan_storage_path,
                        Some(sender),
                        cloned_message_properties.clone(),
                    )
                    .await
            });

            // Create a channel for edits
            let (edits_sender, mut edits_receiver) = tokio::sync::mpsc::channel::<Option<Step>>(1);

            // Clone necessary variables for the edit task
            let symbol_manager_clone = symbol_manager.clone();
            let tool_box_clone = tool_box.clone();
            let message_properties_clone = message_properties.clone();

            // Spawn the edit task
            let edit_task = tokio::spawn(async move {
                let mut steps_up_until_now = 0;
                while let Some(step) = edits_receiver.recv().await {
                    let previous_steps_up_until_now = steps_up_until_now;
                    steps_up_until_now += 1;
                    if step.is_none() {
                        break;
                    }
                    let step = step.expect("is_none to hold");
                    println!("session::perform_plan_generation::new_step_found");
                    let instruction = step.description();
                    if let Some(file_to_edit) = step.file_to_edit() {
                        let file_open_response = tool_box_clone
                            .file_open(file_to_edit.to_owned(), message_properties_clone.clone())
                            .await?;
                        let hub_sender = symbol_manager_clone.hub_sender();
                        let (edit_done_sender, edit_done_receiver) =
                            tokio::sync::oneshot::channel();
                        let _ = hub_sender.send(SymbolEventMessage::new(
                            SymbolEventRequest::simple_edit_request(
                                SymbolIdentifier::with_file_path(&file_to_edit, &file_to_edit),
                                SymbolToEdit::new(
                                    file_to_edit.to_owned(),
                                    file_open_response.full_range(),
                                    file_to_edit.to_owned(),
                                    vec![instruction.to_owned()],
                                    false,
                                    false,
                                    true,
                                    instruction.to_owned(),
                                    None,
                                    false,
                                    None,
                                    true,
                                    None,
                                    vec![],
                                    Some(previous_steps_up_until_now.to_string()),
                                ),
                                ToolProperties::new(),
                            ),
                            message_properties_clone.request_id().clone(),
                            message_properties_clone.ui_sender().clone(),
                            edit_done_sender,
                            message_properties_clone.cancellation_token(),
                            message_properties_clone.editor_url(),
                            message_properties_clone.access_token().to_owned(),
                        ));
                        println!("session::perform_plan_generation::edit_event::hub_sender::send");
                        let _ = edit_done_receiver.await;
                        println!("session::perform_plan_generation::edits_done::hub_sender::happy");
                    }
                }
                Ok::<(), SymbolError>(())
            });

            let mut generated_steps = vec![];

            while let Some(step_message) = stream_receiver.next().await {
                match step_message {
                    StepSenderEvent::NewStep(step) => {
                        {
                            if let Some(ref mut agent_reply_exchange) = agent_reply_exchange {
                                match &mut agent_reply_exchange.exchange_type {
                                    ExchangeType::AgentChat(ref mut agent_chat) => {
                                        match &mut agent_chat.reply {
                                            ExchangeReplyAgent::Plan(ref mut plan_reply) => {
                                                plan_reply.plan_steps.push(step.clone());
                                            }
                                            _ => {}
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        generated_steps.push(step.clone());
                        let _ = edits_sender.send(Some(step)).await;
                    }
                    StepSenderEvent::NewStepTitle(title_found) => {
                        let _ =
                            message_properties
                                .ui_sender()
                                .send(UIEventWithID::plan_title_added(
                                    self.session_id.to_owned(),
                                    exchange_id.clone(),
                                    title_found.step_index(),
                                    title_found.files_to_edit().to_vec(),
                                    title_found.title().to_owned(),
                                ));
                    }
                    StepSenderEvent::NewStepDescription(description_update) => {
                        let _ = message_properties.ui_sender().send(
                            UIEventWithID::plan_description_updated(
                                self.session_id.to_owned(),
                                exchange_id.clone(),
                                description_update.index(),
                                description_update.delta(),
                                description_update.description_up_until_now().to_owned(),
                                description_update.files_to_edit().to_vec(),
                            ),
                        );
                    }
                    StepSenderEvent::Done => {
                        let _ = edits_sender.send(None).await;
                        break;
                    }
                }
            }

            // Close the edits sender and await the edit task
            // println!("session::perform_plan_generation::edits_sender::closed");
            // edits_sender.closed().await;

            println!("session::perform_plan_generation::edit_task::closed");
            let _ = edit_task.await;

            println!("session::perform_plan_generation::stream_receiver::closed");
            stream_receiver.close();

            // there is a race condition with cancel_running_exchange's invocation of
            // set_exchange_as_cancelled, which also saves to storage.
            self.save_to_storage().await?;

            // send a message over here that the request is in review now
            // since we generated something for the plan
            if !message_properties.cancellation_token().is_cancelled() {
                println!("session::perform_plan_generation::cancellation_token::not_cancelled");
                let _ = message_properties
                    .ui_sender()
                    .send(UIEventWithID::request_review(
                        message_properties.root_request_id().to_owned(),
                        message_properties.request_id_str().to_owned(),
                    ));
                let _ = message_properties
                    .ui_sender()
                    .send(UIEventWithID::plan_as_finished(
                        message_properties.root_request_id().to_owned(),
                        message_properties.request_id_str().to_owned(),
                    ));
            }
        }
        Ok(self)
    }

    /// we are going to perform the agentic editing
    pub async fn perform_agentic_editing(
        self,
        scratch_pad_agent: ScratchPadAgent,
        root_directory: String,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Self, SymbolError> {
        let last_exchange = self.last_exchange();
        if let Some(Exchange {
            exchange_id: _,
            exchange_type:
                ExchangeType::Edit(ExchangeTypeEdit {
                    information:
                        ExchangeEditInformation::Agentic(ExchangeEditInformationAgentic {
                            query,
                            codebase_search,
                        }),
                    user_context: _,
                    ..
                }),
            exchange_state: _,
        }) = last_exchange
        {
            let edits_performed = scratch_pad_agent
                .human_message_agentic(
                    HumanAgenticRequest::new(
                        query.to_owned(),
                        root_directory,
                        *codebase_search,
                        self.global_running_user_context.clone(),
                        false,
                    ),
                    message_properties.clone(),
                )
                .await;
            match edits_performed {
                Ok(_) => println!("session::perform_agentic_editing::finished_editing"),
                Err(_) => println!("Failed to edit"),
            };

            println!("session::finished_agentic_editing_exchange");
        }
        Ok(self)
    }

    /// We perform the anchored edit over here
    pub async fn perform_anchored_edit(
        mut self,
        parent_exchange_id: String,
        scratch_pad_agent: ScratchPadAgent,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<Self, SymbolError> {
        let last_exchange = self.last_exchange().cloned();
        if let Some(Exchange {
            exchange_id: _,
            exchange_type:
                ExchangeType::Edit(ExchangeTypeEdit {
                    information:
                        ExchangeEditInformation::Anchored(ExchangeEditInformationAnchored {
                            query,
                            fs_file_path,
                            range,
                            selection_context: _,
                        }),
                    ..
                }),
            exchange_state: _,
        }) = last_exchange
        {
            let mut converted_messages = vec![];
            for previous_message in self.exchanges.iter() {
                converted_messages.push(previous_message.to_conversation_message().await);
            }
            // send a message over that the inference will start in a bit
            let _ = message_properties
                .ui_sender()
                .send(UIEventWithID::inference_started(
                    message_properties.root_request_id().to_owned(),
                    message_properties.request_id_str().to_owned(),
                ));

            // we want to set our state over here that we have started working on it
            // We want to get the changes which have been performed here for the anchored
            // edit especially on the location we are interested in and not anywhere else
            // self.exchanges.push(Exchange::agent_reply(
            //     message_properties.request_id_str().to_owned(),
            //     "thinking".to_owned(),
            //     AideAgentMode::Edit,
            // ));
            // send a message that we are starting with the edits over here
            // we want to make a note of the exchange that we are working on it
            // once we have the eidts, then we also have to make sure that we track
            // that we are working on some exchange
            let _ = message_properties
                .ui_sender()
                .send(UIEventWithID::edits_started_in_exchange(
                    message_properties.root_request_id().to_owned(),
                    message_properties.request_id_str().to_owned(),
                    vec![fs_file_path.to_owned()],
                ));
            let edits_performed = scratch_pad_agent
                .anchor_editing_on_range(
                    range.clone(),
                    fs_file_path.to_owned(),
                    query.to_owned(),
                    converted_messages,
                    self.global_running_user_context
                        .clone()
                        .to_xml(Default::default())
                        .await
                        .map_err(|e| SymbolError::UserContextError(e))?,
                    message_properties.clone(),
                )
                .await;

            match edits_performed {
                Ok(edits_performed) => {
                    self.exchanges.push(Exchange::agent_edits_reply(
                        parent_exchange_id,
                        message_properties.request_id_str().to_owned(),
                        edits_performed,
                    ));
                }
                Err(_) => self.exchanges.push(Exchange::agent_edits_reply(
                    parent_exchange_id,
                    message_properties.request_id_str().to_owned(),
                    "Failed to edit selection properly".to_owned(),
                )),
            }

            // send a message over here static that we can ask the user for review
            let _ = message_properties
                .ui_sender()
                .send(UIEventWithID::request_review(
                    message_properties.root_request_id().to_owned(),
                    message_properties.request_id_str().to_owned(),
                ));
        }
        Ok(self)
    }

    pub fn has_running_code_edits(&self, exchange_id: &str) -> bool {
        let found_exchange = self.find_exchange_by_id(exchange_id);
        println!(
            "session::has_running_code_edits::exchange_id({})::found_exchange::({:?})",
            exchange_id, found_exchange
        );
        match found_exchange {
            Some(exchange) => {
                exchange.is_agent_work() && exchange.is_still_running() && exchange.has_code_edits()
            }
            None => false,
        }
    }

    pub fn set_exchange_as_cancelled(
        mut self,
        exchange_id: &str,
        message_properties: SymbolEventMessageProperties,
    ) -> Self {
        if self.has_running_code_edits(exchange_id) {
            let found_exchange = self.find_exchange_by_id_mut(exchange_id);
            if let Some(exchange) = found_exchange {
                exchange.set_exchange_as_cancelled();
                match &mut exchange.exchange_type {
                    ExchangeType::AgentChat(ref mut agent_chat) => match &mut agent_chat.reply {
                        ExchangeReplyAgent::Edit(_) => {
                            let _ = message_properties.ui_sender().send(
                                UIEventWithID::edits_cancelled_in_exchange(
                                    message_properties.root_request_id().to_owned(),
                                    message_properties.request_id_str().to_owned(),
                                ),
                            );
                        }
                        ExchangeReplyAgent::Plan(ref mut plan) => {
                            plan.plan_discarded = true;
                            let _ = message_properties.ui_sender().send(
                                UIEventWithID::plan_as_cancelled(
                                    message_properties.root_request_id().to_owned(),
                                    message_properties.request_id_str().to_owned(),
                                ),
                            );
                        }
                        _ => {}
                    },
                    _ => {}
                };
            }
        }
        self
    }

    async fn save_to_storage(&self) -> Result<(), SymbolError> {
        let serialized = serde_json::to_string(self).unwrap();
        let mut file = tokio::fs::File::create(self.storage_path())
            .await
            .map_err(|e| SymbolError::IOError(e))?;
        file.write_all(serialized.as_bytes())
            .await
            .map_err(|e| SymbolError::IOError(e))?;
        Ok(())
    }
}
