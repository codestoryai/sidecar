use std::{str::FromStr, sync::Arc};

use anyhow::Context;
use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionResponse, LLMType},
    provider::{LLMProvider, LLMProviderAPIKeys},
    tokenizer::tokenizer::{LLMTokenizer, LLMTokenizerInput},
};
use llm_prompts::{
    answer_model::AnswerModel,
    chat::broker::LLMChatModelBroker,
    reranking::{
        broker::ReRankBroker,
        types::{ReRankStrategy, TERMINAL_OUTPUT},
    },
};
use rake::Rake;
use tiktoken_rs::ChatCompletionRequestMessage;
use tokio::sync::mpsc::Sender;
use tracing::{debug, info};

use crate::{
    agent::llm_funcs::llm::FunctionCall,
    agentic::tool::plan::plan::Plan,
    application::application::Application,
    chunking::{editor_parsing::EditorParsing, text_document::Position},
    db::sqlite::SqlDb,
    repo::types::RepoRef,
    user_context::types::{
        UserContext, VariableInformation as UserContextVariableInformation,
        VariableType as UserContextVariableType,
    },
    webserver::{agent::ActiveWindowData, model_selection::LLMClientConfig},
};

use super::{
    llm_funcs::{self},
    prompts,
    search::stop_words,
};

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct CompletionItem {
    pub answer_up_until_now: String,
    pub delta: Option<String>,
    pub logprobs: Option<Vec<Option<f32>>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Answer {
    // This is the answer up-until now
    pub answer_up_until_now: String,
    // This is the delta between the previously sent answer and the current one
    // when streaming we often times want to send the delta so we can show
    // the output in a streaming fashion
    pub delta: Option<String>,
}

impl Answer {
    pub fn from_llm_completion(llm_completion: LLMClientCompletionResponse) -> Self {
        Self {
            answer_up_until_now: llm_completion.answer_up_until_now().to_owned(),
            delta: llm_completion.delta().map(|delta| delta.to_owned()),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub enum VariableType {
    File,
    CodeSymbol,
    Selection,
}

impl VariableType {
    fn from_user_context_variable_type(variable_type: UserContextVariableType) -> Self {
        match variable_type {
            UserContextVariableType::CodeSymbol => Self::CodeSymbol,
            UserContextVariableType::File => Self::File,
            UserContextVariableType::Selection => Self::CodeSymbol,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VariableInformation {
    pub start_position: Position,
    pub end_position: Position,
    pub fs_file_path: String,
    pub name: String,
    pub variable_type: VariableType,
    pub content: String,
    pub language: String,
}

impl VariableInformation {
    pub fn to_prompt(&self) -> String {
        let start_line = self.start_position.line();
        let end_line = self.end_position.line();
        let fs_file_path = self.fs_file_path.as_str();
        let content = self
            .content
            .split("\n")
            .enumerate()
            .into_iter()
            .map(|(idx, content)| format!("{}: {}", idx + 1, content))
            .collect::<Vec<_>>()
            .join("\n");
        let language = self.language.as_str().to_lowercase();
        let prompt = format!(
            r#"Location: {fs_file_path}:{start_line}-{end_line}
```{language}
{content}
```\n"#
        );
        prompt
    }

    pub fn from_internal_variable_information(
        variable_information: UserContextVariableInformation,
    ) -> Self {
        Self {
            start_position: variable_information.start_position,
            end_position: variable_information.end_position,
            fs_file_path: variable_information.fs_file_path,
            name: variable_information.name,
            variable_type: VariableType::from_user_context_variable_type(
                variable_information.variable_type,
            ),
            content: variable_information.content,
            language: variable_information.language,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtendedVariableInformation {
    pub variable_information: VariableInformation,
    pub extended_code_span: Option<CodeSpan>,
    pub content: String,
}

impl ExtendedVariableInformation {
    pub fn new(
        variable_information: VariableInformation,
        extended_code_span: Option<CodeSpan>,
        content: String,
    ) -> Self {
        Self {
            variable_information,
            extended_code_span,
            content,
        }
    }

    pub fn to_prompt(&self) -> String {
        self.variable_information.to_prompt()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConversationMessage {
    message_id: uuid::Uuid,
    /// We also want to store the session id here so we can load it and save it,
    /// this allows us to follow threads and resume the information from that
    /// context
    session_id: uuid::Uuid,
    /// The query which the user has asked
    query: String,
    /// The steps which the agent has taken up until now
    steps_taken: Vec<AgentStep>,
    /// The state of the agent
    agent_state: AgentState,
    /// The file paths we are interested in, can be populated via search or after
    /// asking for more context
    file_paths: Vec<String>,
    /// The span which we found after performing search
    code_spans: Vec<CodeSpan>,
    /// The files which are open in the editor
    open_files: Vec<String>,
    /// The status of this conversation
    conversation_state: ConversationState,
    /// Final answer which is going to get stored here
    answer: Option<Answer>,
    /// Last updated
    last_updated: u64,
    /// Created at
    created_at: u64,
    // What definitions are we interested in?
    definitions_interested_in: Vec<String>,
    /// Whats the context for the answer if any, this gets set at the same time
    /// when we finished generating the answer
    #[serde(skip)]
    generated_answer_context: Option<String>,
    /// The symbols which were provided by the user
    user_variables: Vec<VariableInformation>,
    /// The context provided by the user, this is also carried over from the
    /// previous user conversations
    #[serde(skip)]
    user_context: UserContext,
    #[serde(skip)]
    active_window_data: Option<ActiveWindowData>,
    // This is where we store the user context which is selected by the user,
    // we get a chance to expand on this if there is enough context window remaining
    extended_variable_information: Vec<ExtendedVariableInformation>,
    plan: Option<Plan>,
}

impl ConversationMessage {
    pub fn send_plan_forward(session_id: uuid::Uuid, plan: Plan) -> Self {
        Self {
            message_id: session_id,
            query: "".to_owned(),
            session_id,
            steps_taken: vec![],
            agent_state: AgentState::CodeEdit,
            file_paths: vec![],
            code_spans: vec![],
            open_files: vec![],
            conversation_state: ConversationState::Finished,
            answer: None,
            last_updated: 0,
            created_at: 0,
            definitions_interested_in: vec![],
            generated_answer_context: None,
            user_context: Default::default(),
            user_variables: vec![],
            active_window_data: None,
            extended_variable_information: vec![],
            plan: Some(plan),
        }
    }

    pub fn explain_message(session_id: uuid::Uuid, agent_state: AgentState, query: String) -> Self {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self {
            message_id: uuid::Uuid::new_v4(),
            session_id,
            query,
            steps_taken: vec![],
            agent_state,
            open_files: vec![],
            file_paths: vec![],
            code_spans: vec![],
            conversation_state: ConversationState::Started,
            answer: None,
            created_at: current_time,
            last_updated: current_time,
            generated_answer_context: None,
            definitions_interested_in: vec![],
            user_variables: vec![],
            user_context: Default::default(),
            active_window_data: None,
            extended_variable_information: vec![],
            plan: None,
        }
    }

    pub fn general_question(
        session_id: uuid::Uuid,
        agent_state: AgentState,
        query: String,
    ) -> Self {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self {
            message_id: uuid::Uuid::new_v4(),
            session_id,
            query,
            steps_taken: vec![],
            agent_state,
            file_paths: vec![],
            code_spans: vec![],
            open_files: vec![],
            conversation_state: ConversationState::Started,
            answer: None,
            created_at: current_time,
            last_updated: current_time,
            generated_answer_context: None,
            definitions_interested_in: vec![],
            user_variables: vec![],
            user_context: Default::default(),
            active_window_data: None,
            extended_variable_information: vec![],
            plan: None,
        }
    }

    pub fn search_message(session_id: uuid::Uuid, agent_state: AgentState, query: String) -> Self {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self {
            message_id: uuid::Uuid::new_v4(),
            session_id,
            query,
            steps_taken: vec![],
            agent_state,
            file_paths: vec![],
            code_spans: vec![],
            open_files: vec![],
            conversation_state: ConversationState::Started,
            answer: None,
            created_at: current_time,
            last_updated: current_time,
            generated_answer_context: None,
            definitions_interested_in: vec![],
            user_variables: vec![],
            user_context: Default::default(),
            active_window_data: None,
            extended_variable_information: vec![],
            plan: None,
        }
    }

    pub fn semantic_search(session_id: uuid::Uuid, agent_state: AgentState, query: String) -> Self {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self {
            message_id: uuid::Uuid::new_v4(),
            session_id,
            query,
            steps_taken: vec![],
            agent_state,
            file_paths: vec![],
            code_spans: vec![],
            open_files: vec![],
            conversation_state: ConversationState::Started,
            answer: None,
            created_at: current_time,
            last_updated: current_time,
            generated_answer_context: None,
            definitions_interested_in: vec![],
            user_variables: vec![],
            user_context: Default::default(),
            active_window_data: None,
            extended_variable_information: vec![],
            plan: None,
        }
    }

    pub fn add_definitions_interested_in(&mut self, definitions: Vec<String>) {
        self.definitions_interested_in.extend(definitions);
    }

    pub fn add_agent_step(&mut self, step: AgentStep) {
        self.steps_taken.push(step);
    }

    pub fn add_code_spans(&mut self, code_span: CodeSpan) {
        self.code_spans.push(code_span);
    }

    pub fn add_path(&mut self, relative_path: String) {
        if self.file_paths.contains(&relative_path) {
            return;
        }
        self.file_paths.push(relative_path);
    }

    pub fn get_file_path_alias(&self, file_path: &str) -> Option<usize> {
        self.file_paths.iter().position(|path| path == file_path)
    }

    pub fn add_extended_variable_information(
        &mut self,
        extended_variable_information: ExtendedVariableInformation,
    ) {
        self.extended_variable_information
            .push(extended_variable_information);
    }

    pub fn get_paths(&self) -> &[String] {
        &self.file_paths.as_slice()
    }

    pub fn code_spans(&self) -> &[CodeSpan] {
        &self.code_spans.as_slice()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn answer(&self) -> Option<Answer> {
        self.answer.clone()
    }

    pub fn extend_user_variables(mut self, user_variable: Vec<VariableInformation>) -> Self {
        self.user_variables.extend(user_variable);
        self
    }

    pub fn add_user_variable(&mut self, user_variable: VariableInformation) {
        self.user_variables.push(user_variable);
    }

    pub fn set_answer(&mut self, answer: String) {
        // It's important that we mark the conversation as finished
        self.conversation_state = ConversationState::Finished;
        self.answer = Some(Answer {
            answer_up_until_now: answer,
            delta: None,
        });
    }

    pub fn get_user_context(&self) -> &UserContext {
        &self.user_context
    }

    pub fn set_user_context(&mut self, user_context: UserContext) {
        self.user_context = user_context;
    }

    pub fn set_active_window(&mut self, active_window_data: Option<ActiveWindowData>) {
        self.active_window_data = active_window_data;
    }

    /// Returns the active window data, if available.
    ///
    /// # Returns
    ///
    /// - `Some(&ActiveWindowData)`: If there is active window data available.
    /// - `None`: If there is no active window data available.
    pub fn get_active_window(&self) -> Option<&ActiveWindowData> {
        self.active_window_data.as_ref()
    }

    pub fn set_generated_answer_context(&mut self, answer_context: String) {
        self.generated_answer_context = Some(answer_context);
    }

    pub fn get_generated_answer_context(&self) -> Option<&str> {
        self.generated_answer_context.as_deref()
    }

    pub fn answer_update(session_id: uuid::Uuid, answer_event: AgentAnswerStreamEvent) -> Self {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        match answer_event {
            AgentAnswerStreamEvent::LLMAnswer(llm_completion) => {
                Self {
                    message_id: uuid::Uuid::new_v4(),
                    session_id,
                    query: String::new(),
                    steps_taken: vec![],
                    agent_state: AgentState::Finish,
                    file_paths: vec![],
                    code_spans: vec![],
                    open_files: vec![],
                    conversation_state: ConversationState::StreamingAnswer,
                    answer: Some(Answer::from_llm_completion(llm_completion)),
                    created_at: current_time,
                    last_updated: current_time,
                    // We use this to send it over the wire, this is pretty bad anyways
                    // but its fine to do things this way
                    generated_answer_context: None,
                    definitions_interested_in: vec![],
                    user_variables: vec![],
                    user_context: Default::default(),
                    active_window_data: None,
                    extended_variable_information: vec![],
                    plan: None,
                }
            }
            AgentAnswerStreamEvent::ReRankingFinished => {
                Self {
                    message_id: uuid::Uuid::new_v4(),
                    session_id,
                    query: String::new(),
                    steps_taken: vec![],
                    agent_state: AgentState::Finish,
                    file_paths: vec![],
                    code_spans: vec![],
                    open_files: vec![],
                    conversation_state: ConversationState::ReRankingFinished,
                    answer: None,
                    created_at: current_time,
                    last_updated: current_time,
                    // We use this to send it over the wire, this is pretty bad anyways
                    // but its fine to do things this way
                    generated_answer_context: None,
                    definitions_interested_in: vec![],
                    user_variables: vec![],
                    user_context: Default::default(),
                    active_window_data: None,
                    extended_variable_information: vec![],
                    plan: None,
                }
            }
            AgentAnswerStreamEvent::ReRankingStarted => {
                Self {
                    message_id: uuid::Uuid::new_v4(),
                    session_id,
                    query: String::new(),
                    steps_taken: vec![],
                    agent_state: AgentState::Finish,
                    file_paths: vec![],
                    code_spans: vec![],
                    open_files: vec![],
                    conversation_state: ConversationState::ReRankingStarted,
                    answer: None,
                    created_at: current_time,
                    last_updated: current_time,
                    // We use this to send it over the wire, this is pretty bad anyways
                    // but its fine to do things this way
                    generated_answer_context: None,
                    definitions_interested_in: vec![],
                    user_variables: vec![],
                    user_context: Default::default(),
                    active_window_data: None,
                    extended_variable_information: vec![],
                    plan: None,
                }
            }
        }
    }

    pub async fn save_to_db(&self, db: SqlDb, repo_ref: RepoRef) -> anyhow::Result<()> {
        debug!(%self.session_id, %self.message_id, "saving conversation message to db");
        let mut tx = db.begin().await?;
        let message_id = self.message_id.to_string();
        let query = self.query.to_owned();
        let answer = self
            .answer
            .as_ref()
            .map(|answer| answer.answer_up_until_now.to_owned());
        let created_at = self.created_at as i64;
        let last_updated = self.last_updated as i64;
        let session_id = self.session_id.to_string();
        let steps_taken = serde_json::to_string(&self.steps_taken)?;
        let agent_state = serde_json::to_string(&self.agent_state)?;
        let file_paths = serde_json::to_string(&self.file_paths)?;
        let code_spans = serde_json::to_string::<Vec<CodeSpan>>(&vec![])?;
        let open_files = serde_json::to_string(&self.open_files)?;
        let conversation_state = serde_json::to_string(&self.conversation_state)?;
        let repo_ref_str = repo_ref.to_string();
        let generated_answer_context = self.generated_answer_context.clone();
        let user_context = serde_json::to_string(&self.user_context)?;
        sqlx::query! {
            "DELETE FROM agent_conversation_message WHERE message_id = ?;",
            message_id,
        }
        .execute(&mut *tx)
        .await?;
        sqlx::query! {
            "INSERT INTO agent_conversation_message \
            (message_id, query, answer, created_at, last_updated, session_id, steps_taken, agent_state, file_paths, code_spans, user_selected_code_span, open_files, conversation_state, repo_ref, generated_answer_context, user_context) \
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            message_id,
            query,
            answer,
            created_at,
            last_updated,
            session_id,
            steps_taken,
            agent_state,
            file_paths,
            code_spans,
            "",
            open_files,
            conversation_state,
            repo_ref_str,
            generated_answer_context,
            user_context,
        }.execute(&mut *tx).await?;
        let _ = tx.commit().await?;
        Ok(())
    }

    pub async fn load_from_db(
        db: SqlDb,
        reporef: &RepoRef,
        session_id: uuid::Uuid,
    ) -> anyhow::Result<Vec<ConversationMessage>> {
        let session_id_str = session_id.to_string();
        let repo_ref_str = reporef.to_string();
        // Now here we are going to read the previous conversations which have
        // happened for the session_id and the repository reference
        let rows = sqlx::query! {
            "SELECT message_id, query, answer, created_at, last_updated, session_id, steps_taken, agent_state, file_paths, code_spans, user_selected_code_span, open_files, conversation_state, generated_answer_context, user_context FROM agent_conversation_message \
            WHERE session_id = ? AND repo_ref = ?",
            session_id_str,
            repo_ref_str,
        }
        .fetch_all(db.as_ref())
        .await
        .context("failed to fetch conversation message from the db")?;
        Ok(rows
            .into_iter()
            .map(|record| {
                let message_id = record.message_id;
                let session_id = record.session_id;
                let query = record.query;
                let steps_taken = serde_json::from_str::<Vec<AgentStep>>(&record.steps_taken);
                let agent_state = serde_json::from_str::<AgentState>(&record.agent_state);
                let file_paths = serde_json::from_str::<Vec<String>>(&record.file_paths);
                let open_files = serde_json::from_str::<Vec<String>>(&record.open_files);
                let conversation_state =
                    serde_json::from_str::<ConversationState>(&record.conversation_state);
                let answer = record.answer;
                let last_updated = record.last_updated;
                let created_at = record.created_at;
                let generated_answer_context = record.generated_answer_context;
                let user_context = record
                    .user_context
                    .map(|user_context| serde_json::from_str::<UserContext>(&user_context))
                    .transpose()
                    .unwrap_or_default()
                    .unwrap_or_default();
                ConversationMessage {
                    message_id: uuid::Uuid::from_str(&message_id)
                        .expect("parsing back should work"),
                    session_id: uuid::Uuid::from_str(&session_id)
                        .expect("parsing back should work"),
                    query,
                    steps_taken: steps_taken.unwrap_or_default(),
                    agent_state: agent_state.expect("parsing back should work"),
                    file_paths: file_paths.unwrap_or_default(),
                    code_spans: vec![],
                    open_files: open_files.unwrap_or_default(),
                    conversation_state: conversation_state.expect("parsing back should work"),
                    answer: answer.map(|answer| Answer {
                        answer_up_until_now: answer,
                        delta: None,
                    }),
                    last_updated: last_updated as u64,
                    created_at: created_at as u64,
                    generated_answer_context,
                    // we purposefuly leave this blank
                    // TODO(skcd): Run the migration script for recording this later
                    definitions_interested_in: vec![],
                    // we also leave this blank right now
                    user_variables: vec![],
                    user_context,
                    active_window_data: None,
                    extended_variable_information: vec![],
                    plan: None,
                }
            })
            .collect())
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct CodeSpan {
    pub file_path: String,
    pub alias: usize,
    pub start_line: u64,
    pub end_line: u64,
    #[serde(skip)]
    pub data: String,
    pub score: Option<f32>,
}

impl CodeSpan {
    pub fn new(
        file_path: String,
        alias: usize,
        start_line: u64,
        end_line: u64,
        data: String,
        score: Option<f32>,
    ) -> Self {
        Self {
            file_path,
            alias,
            start_line,
            end_line,
            data,
            score,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.data.trim().is_empty()
    }

    pub fn get_unique_key(&self) -> String {
        format!("{}:{}-{}", self.file_path, self.start_line, self.end_line)
    }

    pub fn from_active_window(active_window_data: &ActiveWindowData, path_alias: usize) -> Self {
        let file_path = active_window_data.file_path.clone();
        let split_content = active_window_data.file_content.lines().collect::<Vec<_>>();
        let alias = path_alias;
        let start_line = 0;
        let end_line = split_content.len() as u64;
        let score = Some(1.0);
        let data = active_window_data.file_content.clone();
        Self {
            file_path,
            alias,
            start_line,
            end_line,
            score,
            data,
        }
    }
}

impl std::fmt::Display for CodeSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}: {}\n{}", self.alias, self.file_path, self.data)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AgentStep {
    Path {
        query: String,
        response: String,
        paths: Vec<String>,
    },
    Code {
        query: String,
        #[serde(skip)]
        response: String,
        code_snippets: Vec<CodeSpan>,
    },
    Proc {
        query: String,
        paths: Vec<String>,
        response: String,
    },
}

impl AgentStep {
    pub fn get_response(&self) -> String {
        match self {
            AgentStep::Path { response, .. } => response.to_owned(),
            AgentStep::Code { response, .. } => response.to_owned(),
            AgentStep::Proc { response, .. } => response.to_owned(),
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentAction {
    Query(String),
    Path {
        query: String,
    },
    Code {
        query: String,
    },
    Proc {
        query: String,
        paths: Vec<usize>,
    },
    #[serde(rename = "none")]
    Answer {
        paths: Vec<usize>,
    },
}

impl AgentAction {
    pub fn from_gpt_response(call: &FunctionCall) -> anyhow::Result<Self> {
        let mut map = serde_json::Map::new();
        map.insert(
            call.name
                .as_ref()
                .expect("function_name to be present in function_call")
                .to_owned(),
            serde_json::from_str(&call.arguments)?,
        );

        Ok(serde_json::from_value(serde_json::Value::Object(map))?)
    }
}

#[derive(Clone)]
pub struct Agent {
    pub application: Application,
    pub reporef: RepoRef,
    /// Session-id is unique to a single execution of the agent, this is different
    /// from the thread-id which you will see across the codebase which is the
    /// interactions happening across a single thread
    pub session_id: uuid::Uuid,
    pub conversation_messages: Vec<ConversationMessage>,
    pub llm_broker: Arc<LLMBroker>,
    pub sql_db: SqlDb,
    pub sender: Sender<ConversationMessage>,
    pub user_context: Option<UserContext>,
    pub project_labels: Vec<String>,
    pub editor_parsing: EditorParsing,
    pub model_config: LLMClientConfig,
    pub llm_tokenizer: Arc<LLMTokenizer>,
    pub chat_broker: Arc<LLMChatModelBroker>,
    pub reranker: Arc<ReRankBroker>,
    pub system_instruction: Option<String>,
}

pub enum AgentAnswerStreamEvent {
    LLMAnswer(LLMClientCompletionResponse),
    // Do I like this? fuck no, but we are using this cause its easier for now
    ReRankingStarted,
    ReRankingFinished,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AgentState {
    // We will end up doing a search
    Search,
    // Plan out what the changes are required for the agent to do
    Plan,
    // Explain to the user what the code is going to do
    Explain,
    // The code editing which needs to be done
    CodeEdit,
    // Fix the linters and everything else over here
    FixSignals,
    // We finish up the work of the agent
    Finish,
    // If we are performing semantic search, we should be over here
    SemanticSearch,
    // Followup question or general question answer
    FollowupChat,
    // Don't use followup but the general state
    ViewPort,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ConversationState {
    Pending,
    Started,
    StreamingAnswer,
    Finished,
    // if we start the reranking, we should signal that here
    ReRankingStarted,
    // if we finish the reranking, we should signal that here
    ReRankingFinished,
}

impl Agent {
    // Listwise is the best we can just assume that for now
    pub fn reranking_strategy(&self) -> ReRankStrategy {
        ReRankStrategy::ListWise
    }

    pub fn slow_llm_model(&self) -> &LLMType {
        &self.model_config.slow_model
    }

    pub fn fast_llm_model(&self) -> &LLMType {
        &self.model_config.fast_model
    }

    pub fn provider_for_llm(&self, llm_type: &LLMType) -> Option<&LLMProviderAPIKeys> {
        // Grabs the provider required for the slow model
        // we first need to get the model configuration for the slow model
        // which will give us the model and the context around it
        let model = self.model_config.models.get(llm_type);
        if let None = model {
            return None;
        }
        let model = model.expect("is_none above to hold");
        let provider = &model.provider;
        // get the related provider if its present
        self.model_config
            .providers
            .iter()
            .find(|p| p.key(provider).is_some())
    }

    pub fn provider_config_for_llm(&self, llm_type: &LLMType) -> Option<&LLMProvider> {
        self.model_config
            .models
            .get(llm_type)
            .map(|model_config| &model_config.provider)
    }

    pub fn provider_for_slow_llm(&self) -> Option<&LLMProviderAPIKeys> {
        // Grabs the provider required for the slow model
        // we first need to get the model configuration for the slow model
        // which will give us the model and the context around it
        let model = self.model_config.models.get(&self.model_config.slow_model);
        if let None = model {
            return None;
        }
        let model = model.expect("is_none above to hold");
        let provider = &model.provider;
        // get the related provider if its present
        self.model_config
            .providers
            .iter()
            .find(|p| p.key(provider).is_some())
    }

    pub fn provider_config_for_slow_model(&self) -> Option<&LLMProvider> {
        self.model_config
            .models
            .get(&self.model_config.slow_model)
            .map(|model_config| &model_config.provider)
    }

    pub fn reporef(&self) -> &RepoRef {
        &self.reporef
    }

    pub fn get_last_conversation_message_agent_state(&self) -> &AgentState {
        &self
            .conversation_messages
            .last()
            .expect("There should be a conversation message")
            .agent_state
    }

    pub fn conversation_messages_len(&self) -> usize {
        self.conversation_messages.len()
    }

    pub fn get_user_variables(&self) -> Option<&[VariableInformation]> {
        self.conversation_messages
            .last()
            .map(|conversation_message| conversation_message.user_variables.as_slice())
    }

    pub fn get_extended_user_selection_information(
        &self,
    ) -> Option<&[ExtendedVariableInformation]> {
        self.conversation_messages
            .last()
            .map(|conversation_message| {
                conversation_message
                    .extended_variable_information
                    .as_slice()
            })
    }

    pub fn get_definition_snippets(&self) -> Option<&[String]> {
        self.conversation_messages
            .last()
            .map(|conversation_message| conversation_message.definitions_interested_in.as_slice())
    }

    pub fn get_query(&self) -> Option<String> {
        self.conversation_messages
            .last()
            .map(|conversation_message| conversation_message.query.to_owned())
    }

    pub fn get_last_conversation_message_immutable(&self) -> &ConversationMessage {
        // If we don't have a conversation message then, we will crash and burn
        // here
        self.conversation_messages
            .last()
            .expect("There should be a conversation message")
    }

    pub fn get_last_conversation_message(&mut self) -> &mut ConversationMessage {
        // If we don't have a conversation message then, we will crash and burn
        // here
        self.conversation_messages
            .last_mut()
            .expect("There should be a conversation message")
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.conversation_messages
            .iter()
            .flat_map(|e| e.file_paths.iter())
            .map(String::as_str)
    }

    pub fn add_path(&mut self, path: &str) {
        self.get_last_conversation_message()
            .add_path(path.to_owned());
    }

    pub async fn get_file_content(&self, path: &str) -> anyhow::Result<Option<String>> {
        // first we try to get the context from the user provided context
        let content = self
            .user_context
            .as_ref()
            .map(|user_context| {
                user_context
                    .file_content_map
                    .iter()
                    .filter(|file_content| file_content.file_path == path)
                    .map(|file_content| file_content.file_content.clone())
                    .next()
            })
            .flatten();
        if let Some(content) = content {
            return Ok(Some(content));
        }

        if path == TERMINAL_OUTPUT {
            // we need to grab the terminal output here from the user context
            if let Some(terminal_output) = self
                .user_context
                .as_ref()
                .and_then(|user_context| user_context.terminal_selection.clone())
            {
                return Ok(Some(terminal_output));
            }
        }

        // this might also be a folder path, in which case we have to look at the
        // folder part of the user context and grab it from there
        let content = self
            .code_spans()
            .iter()
            .find(|code_span| code_span.file_path == path)
            .map(|code_span| code_span.data.to_owned());
        if let Some(content) = content {
            return Ok(Some(content));
        }

        // Now we try to check if the active window data has the file content
        let content = self
            .get_last_conversation_message_immutable()
            .get_active_window()
            .map(|active_window_data| {
                if active_window_data.file_path == path {
                    Some(active_window_data.file_content.clone())
                } else {
                    None
                }
            })
            .flatten();
        if let Some(content) = content {
            return Ok(Some(content));
        }
        debug!(%self.reporef, path, %self.session_id, "executing file search");
        Ok(None)
    }

    pub fn get_path_alias(&mut self, path: &str) -> usize {
        // This has to be stored a variable due to a Rust NLL bug:
        // https://github.com/rust-lang/rust/issues/51826
        let pos = self.paths().position(|p| p == path);
        if let Some(i) = pos {
            i
        } else {
            let i = self.paths().count();
            self.get_last_conversation_message()
                .file_paths
                .push(path.to_owned());
            i
        }
    }

    /// The full history of messages, including intermediate function calls
    fn history(&self) -> anyhow::Result<Vec<llm_funcs::llm::Message>> {
        const ANSWER_MAX_HISTORY_SIZE: usize = 3;
        const FUNCTION_CALL_INSTRUCTION: &str = "CALL A FUNCTION!. Do not answer";

        let history = self
            .conversation_messages
            .iter()
            .rev()
            .take(ANSWER_MAX_HISTORY_SIZE)
            .rev()
            .try_fold(Vec::new(), |mut acc, e| -> anyhow::Result<_> {
                let query = llm_funcs::llm::Message::user(e.query());

                let steps = e.steps_taken.iter().flat_map(|s| {
                    let (name, arguments) = match s {
                        AgentStep::Path { query, .. } => (
                            "path".to_owned(),
                            format!("{{\n \"query\": \"{query}\"\n}}"),
                        ),
                        AgentStep::Code { query, .. } => (
                            "code".to_owned(),
                            format!("{{\n \"query\": \"{query}\"\n}}"),
                        ),
                        AgentStep::Proc { query, paths, .. } => (
                            "proc".to_owned(),
                            format!(
                                "{{\n \"paths\": [{}],\n \"query\": \"{query}\"\n}}",
                                paths
                                    .iter()
                                    .map(|path| self
                                        .paths()
                                        .position(|p| p == path)
                                        .unwrap()
                                        .to_string())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        ),
                    };

                    vec![
                        llm_funcs::llm::Message::function_call(FunctionCall {
                            name: Some(name.to_owned()),
                            arguments,
                        }),
                        llm_funcs::llm::Message::function_return(name.to_owned(), s.get_response()),
                        llm_funcs::llm::Message::user(FUNCTION_CALL_INSTRUCTION),
                    ]
                });

                let answer = match e.answer() {
                    // NB: We intentionally discard the summary as it is redundant.
                    Some(answer) => Some(llm_funcs::llm::Message::function_return(
                        "none".to_owned(),
                        answer.answer_up_until_now,
                    )),
                    None => None,
                };

                acc.extend(
                    std::iter::once(query)
                        .chain(vec![llm_funcs::llm::Message::user(
                            FUNCTION_CALL_INSTRUCTION,
                        )])
                        .chain(steps)
                        .chain(answer.into_iter()),
                );
                Ok(acc)
            })?;
        Ok(history)
    }

    pub fn code_spans(&self) -> Vec<CodeSpan> {
        self.conversation_messages
            .iter()
            .flat_map(|e| e.code_spans.clone())
            .collect()
    }

    pub async fn iterate(
        &mut self,
        action: AgentAction,
        answer_sender: tokio::sync::mpsc::UnboundedSender<AgentAnswerStreamEvent>,
    ) -> anyhow::Result<Option<AgentAction>> {
        // Now we will go about iterating over the action and figure out what the
        // next best action should be
        match action {
            AgentAction::Answer { paths } => {
                // here we can finally answer after we do some merging on the spans
                // and also look at the history to provide more context
                dbg!("sidecar.followup.generating_answer");
                let answer = self.answer(paths.as_slice(), answer_sender).await?;
                info!(%self.session_id, "conversation finished");
                info!(%self.session_id, answer, "answer");
                // We should make it an atomic operation where whenever we update
                // the conversation, we send an update on the stream and also
                // save it to the db
                if let Some(last_conversation) = self.conversation_messages.last() {
                    // save the conversation to the DB
                    let _ = last_conversation
                        .save_to_db(self.sql_db.clone(), self.reporef().clone())
                        .await;
                    // send it over the sender
                    let _ = self.sender.send(last_conversation.clone()).await;
                }
                return Ok(None);
            }
            AgentAction::Code { query } => self.code_search(&query).await?,
            AgentAction::Path { query } => self.path_search(&query).await?,
            AgentAction::Proc { query, paths } => {
                self.process_files(&query, paths.as_slice()).await?
            }
            AgentAction::Query(query) => {
                // just log here for now
                let cloned_query = query.clone();
                // we want to do a search anyways here using the keywords, so we
                // have some kind of context
                // Always make a code search for the user query on the first exchange
                if self.conversation_messages.len() == 1 {
                    // Extract keywords from the query
                    let keywords = {
                        let sw = stop_words();
                        let r = Rake::new(sw.clone());
                        let keywords = r.run(&cloned_query);

                        if keywords.is_empty() {
                            cloned_query.to_owned()
                        } else {
                            keywords
                                .iter()
                                .map(|k| k.keyword.clone())
                                .collect::<Vec<_>>()
                                .join(" ")
                        }
                    };

                    debug!(%self.session_id, %keywords, "extracted keywords from query");

                    let response = self.code_search(&keywords).await;
                    debug!(?response, "code search response");
                }
                query.clone()
            }
        };

        // We also retroactively save the last conversation to the database
        if let Some(last_conversation) = self.conversation_messages.last() {
            // save the conversation to the DB
            let _ = dbg!(
                last_conversation
                    .save_to_db(self.sql_db.clone(), self.reporef().clone())
                    .await
            );
            // send it over the sender
            let _ = self.sender.send(last_conversation.clone()).await;
        }

        let _functions = serde_json::from_value::<Vec<llm_funcs::llm::Function>>(
            prompts::functions(self.paths().next().is_some()), // Only add proc if there are paths in context
        )
        .unwrap();

        let mut history = vec![llm_funcs::llm::Message::system(&prompts::system_search(
            self.paths(),
        ))];
        history.extend(self.history()?);

        let slow_llm = self.slow_llm_model();
        let answer_model = self.chat_broker.get_answer_model(&slow_llm)?;

        let _trimmed_history =
            trim_history(history.clone(), self.llm_tokenizer.clone(), &answer_model)?;

        Ok(None)

        // TODO(skcd): Bring back function calling when we want to
        // let response = self
        //     .get_llm_client()
        //     .stream_function_call(
        //         llm_funcs::llm::OpenAIModel::get_model(self.model.model_name)?,
        //         trimmed_history,
        //         functions,
        //         0.0,
        //         None,
        //     )
        //     .await?;

        // if let Some(response) = response {
        //     AgentAction::from_gpt_response(&response).map(|response| Some(response))
        // } else {
        //     Ok(None)
        // }
    }
}

fn trim_history(
    mut history: Vec<llm_funcs::llm::Message>,
    tokenizer: Arc<LLMTokenizer>,
    model: &AnswerModel,
) -> anyhow::Result<Vec<llm_funcs::llm::Message>> {
    const HIDDEN: &str = "[HIDDEN]";

    let mut tiktoken_msgs: Vec<ChatCompletionRequestMessage> =
        history.iter().map(|m| m.into()).collect::<Vec<_>>();

    let messages = history
        .iter()
        .map(|m| m.try_into())
        .collect::<Vec<_>>()
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;

    while tokenizer.count_tokens(
        &model.llm_type,
        LLMTokenizerInput::Messages(messages.to_vec()),
    )? < model
        .history_tokens_limit
        .try_into()
        .expect("usize to i64 to not fail")
    {
        let _ = history
            .iter_mut()
            .zip(tiktoken_msgs.iter_mut())
            .position(|(m, tm)| match m {
                llm_funcs::llm::Message::PlainText {
                    role,
                    ref mut content,
                } => {
                    if role == &llm_funcs::llm::Role::Assistant && content != HIDDEN {
                        *content = HIDDEN.into();
                        tm.content = Some(HIDDEN.into());
                        true
                    } else {
                        false
                    }
                }
                llm_funcs::llm::Message::FunctionReturn {
                    role: _,
                    name: _,
                    ref mut content,
                } if content != HIDDEN => {
                    *content = HIDDEN.into();
                    tm.content = Some(HIDDEN.into());
                    true
                }
                _ => false,
            })
            .ok_or_else(|| anyhow::anyhow!("could not find message to trim"))?;
    }

    Ok(history)
}

impl Drop for Agent {
    fn drop(&mut self) {
        // Now we will try to save all the conversations to the database
        let db = self.sql_db.clone();
        let conversation_messages = self.conversation_messages.to_vec();
        // This will save all the pending conversations to the database
        let repo_ref = self.reporef().clone();
        tokio::spawn(async move {
            use futures::StreamExt;
            futures::stream::iter(conversation_messages)
                .map(|conversation| (conversation, db.clone(), repo_ref.clone()))
                .map(|(conversation, db, repo_ref)| async move {
                    conversation.save_to_db(db.clone(), repo_ref).await
                })
                .collect::<Vec<_>>()
                .await;
        });
    }
}
