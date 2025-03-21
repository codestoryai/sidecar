use either::Either;
use futures::pin_mut;
use futures::stream;
use futures::FutureExt;
use futures::StreamExt;
use llm_client::broker::LLMBroker;
use llm_client::clients::types::LLMClientCompletionRequest;
use llm_client::clients::types::LLMClientCompletionResponse;
use llm_client::clients::types::LLMClientCompletionStringRequest;
use llm_client::clients::types::LLMClientMessage;
use llm_client::clients::types::LLMType;
use llm_prompts::chat::broker::LLMChatModelBroker;
use llm_prompts::in_line_edit::broker::InLineEditPromptBroker;
use llm_prompts::in_line_edit::types::InLineDocNode;
use llm_prompts::in_line_edit::types::InLineDocRequest;
use llm_prompts::in_line_edit::types::InLineEditRequest;
use llm_prompts::in_line_edit::types::InLineFixRequest;
use llm_prompts::in_line_edit::types::InLinePromptResponse;
use regex::Regex;
use std::sync::Arc;
use tokio::sync::mpsc::{Sender, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::info;

use crate::chunking::text_document::Range;
use crate::chunking::types::FunctionInformation;
use crate::chunking::types::FunctionNodeType;
use crate::in_line_agent::context_parsing::generate_context_for_range;
use crate::in_line_agent::context_parsing::ContextParserInLineEdit;
use crate::in_line_agent::context_parsing::EditExpandedSelectionRange;
use crate::user_context::types::UserContext;
use crate::{
    application::application::Application,
    chunking::{editor_parsing::EditorParsing, text_document::DocumentSymbol},
    db::sqlite::SqlDb,
    repo::types::RepoRef,
    webserver::in_line_agent::ProcessInEditorRequest,
};

use super::context_parsing::generate_selection_context_for_fix;
use super::context_parsing::ContextWindowTracker;
use super::context_parsing::SelectionContext;
use super::context_parsing::SelectionWithOutlines;
use super::prompts;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InLineAgentSelectionData {
    has_content: bool,
    first_line_index: i64,
    last_line_index: i64,
    lines: Vec<String>,
}

impl InLineAgentSelectionData {
    pub fn new(
        has_content: bool,
        first_line_index: i64,
        last_line_index: i64,
        lines: Vec<String>,
    ) -> Self {
        Self {
            has_content,
            first_line_index,
            last_line_index,
            lines,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContextSelection {
    above: InLineAgentSelectionData,
    range: InLineAgentSelectionData,
    below: InLineAgentSelectionData,
}

impl ContextSelection {
    pub fn new(
        above: InLineAgentSelectionData,
        range: InLineAgentSelectionData,
        below: InLineAgentSelectionData,
    ) -> Self {
        Self {
            above,
            range,
            below,
        }
    }

    pub fn from_selection_context(selection_context: SelectionContext) -> Self {
        Self {
            above: selection_context.above.to_agent_selection_data(),
            range: selection_context.range.to_agent_selection_data(),
            below: selection_context.below.to_agent_selection_data(),
        }
    }

    pub fn generate_placeholder_for_range(range: &Range) -> Self {
        let mut lines = vec![];
        for _ in range.start_line()..=range.end_line() {
            lines.push(String::new());
        }
        Self {
            above: InLineAgentSelectionData::new(false, 0, 0, vec![]),
            range: InLineAgentSelectionData::new(
                true,
                range.start_line().try_into().unwrap(),
                range.end_line().try_into().unwrap(),
                lines,
            ),
            below: InLineAgentSelectionData::new(false, 0, 0, vec![]),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InLineAgentAnswer {
    pub answer_up_until_now: String,
    pub delta: Option<String>,
    pub state: MessageState,
    // We also send the document symbol in question along the wire
    pub document_symbol: Option<DocumentSymbol>,
    pub context_selection: Option<ContextSelection>,
    pub model: LLMType,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum InLineAgentAction {
    // Add code to an already existing codebase
    Code,
    // Add documentation comment for this symbol
    Doc,
    // Refactors the selected code based on requirements provided by the user
    Edit,
    // Generate unit tests for the selected code
    Tests,
    // Propose a fix for the problems in the selected code
    Fix,
    // Explain how the selected code snippet works
    Explain,
    // Intent of this command is unclear or is not related to the information technologies
    Unknown,
    // decide the next action the agent should take, this is the first state always
    DecideAction { query: String },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MessageState {
    Pending,
    Started,
    StreamingAnswer,
    Finished,
    Errored,
}

impl Default for MessageState {
    fn default() -> Self {
        MessageState::StreamingAnswer
    }
}

impl InLineAgentAction {
    pub fn from_gpt_response(response: &str) -> anyhow::Result<Self> {
        if response.contains("code") {
            Ok(Self::Code)
        } else if response.contains("doc") {
            Ok(Self::Doc)
        } else if response.contains("edit") {
            Ok(Self::Edit)
        } else if response.contains("tests") {
            Ok(Self::Tests)
        } else if response.contains("fix") {
            Ok(Self::Fix)
        } else if response.contains("explain") {
            Ok(Self::Explain)
        } else if response.contains("unknown") {
            Ok(Self::Unknown)
        } else {
            Ok(Self::Unknown)
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InLineAgentMessage {
    message_id: uuid::Uuid,
    session_id: uuid::Uuid,
    query: String,
    steps_taken: Vec<InLineAgentAction>,
    message_state: MessageState,
    answer: Option<InLineAgentAnswer>,
    last_updated: u64,
    created_at: u64,
}

impl InLineAgentMessage {
    pub fn decide_action(
        session_id: uuid::Uuid,
        query: String,
        agent_state: InLineAgentAction,
    ) -> Self {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self {
            message_id: uuid::Uuid::new_v4(),
            session_id,
            query,
            steps_taken: vec![agent_state],
            message_state: MessageState::Started,
            answer: None,
            last_updated: current_time,
            created_at: current_time,
        }
    }

    pub fn answer_update(session_id: uuid::Uuid, answer_update: InLineAgentAnswer) -> Self {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self {
            message_id: uuid::Uuid::new_v4(),
            session_id,
            query: String::new(),
            steps_taken: vec![],
            message_state: MessageState::StreamingAnswer,
            answer: Some(answer_update),
            last_updated: current_time,
            created_at: current_time,
        }
    }

    pub fn start_message(session_id: uuid::Uuid, query: String) -> Self {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self {
            message_id: uuid::Uuid::new_v4(),
            session_id,
            query,
            steps_taken: vec![],
            message_state: MessageState::Pending,
            answer: None,
            last_updated: current_time,
            created_at: current_time,
        }
    }

    pub fn add_agent_action(&mut self, agent_action: InLineAgentAction) {
        self.steps_taken.push(agent_action);
    }
}

/// We have an inline agent which takes care of questions which are asked in-line
/// this agent behaves a bit different than the general agent which we provide
/// as a chat, so there are different states and other things which this agent
/// takes care of
#[derive(Clone)]
pub struct InLineAgent {
    application: Application,
    repo_ref: RepoRef,
    _session_id: uuid::Uuid,
    inline_agent_messages: Vec<InLineAgentMessage>,
    llm_broker: Arc<LLMBroker>,
    llm_prompt_formatter: Arc<InLineEditPromptBroker>,
    chat_broker: Arc<LLMChatModelBroker>,
    _sql_db: SqlDb,
    editor_parsing: EditorParsing,
    // TODO(skcd): Break this out and don't use cross crate dependency like this
    editor_request: ProcessInEditorRequest,
    sender: Sender<InLineAgentMessage>,
}

impl InLineAgent {
    pub fn new(
        application: Application,
        repo_ref: RepoRef,
        sql_db: SqlDb,
        llm_broker: Arc<LLMBroker>,
        llm_prompt_formatter: Arc<InLineEditPromptBroker>,
        editor_parsing: EditorParsing,
        editor_request: ProcessInEditorRequest,
        messages: Vec<InLineAgentMessage>,
        sender: Sender<InLineAgentMessage>,
        chat_broker: Arc<LLMChatModelBroker>,
    ) -> Self {
        Self {
            application,
            repo_ref,
            _session_id: uuid::Uuid::new_v4(),
            inline_agent_messages: messages,
            llm_broker,
            llm_prompt_formatter,
            _sql_db: sql_db,
            sender,
            editor_request,
            editor_parsing,
            chat_broker,
        }
    }

    fn get_llm_broker(&self) -> Arc<LLMBroker> {
        self.llm_broker.clone()
    }

    fn last_agent_message(&self) -> Option<&InLineAgentMessage> {
        self.inline_agent_messages.last()
    }

    fn get_last_agent_message(&mut self) -> &mut InLineAgentMessage {
        self.inline_agent_messages
            .last_mut()
            .expect("There should always be a agent message")
    }

    pub async fn iterate(
        &mut self,
        action: InLineAgentAction,
        answer_sender: UnboundedSender<InLineAgentAnswer>,
    ) -> anyhow::Result<Option<InLineAgentAction>> {
        let llm = self.editor_request.slow_model();
        match action {
            InLineAgentAction::DecideAction { query } => {
                // If we are using OSS models we take a different route (especially
                // for smaller models since they can't follow the commands properly)
                info!(
                    event_name = "inline_edit_agent",
                    is_openai = llm.is_openai(),
                    is_custom = llm.is_custom(),
                    slow_model = ?llm,
                );
                if !llm.is_openai() {
                    let last_exchange = self.get_last_agent_message();
                    // We add that we took a action to decide what we should do next
                    last_exchange.add_agent_action(InLineAgentAction::DecideAction {
                        query: query.to_owned(),
                    });
                    if let Some(last_exchange) = self.last_agent_message() {
                        self.sender.send(last_exchange.clone()).await?;
                    }
                    if query.starts_with("/fix") {
                        return Ok(Some(InLineAgentAction::Fix));
                    } else if query.starts_with("/doc") {
                        return Ok(Some(InLineAgentAction::Doc));
                    } else {
                        info!(
                            event_name = "inline_agent_decide_action",
                            query = ?query,
                        );
                        return Ok(Some(InLineAgentAction::Code));
                    }
                }
                info!(
                    event_name = "inline_agent_decide_action_gpt",
                    query = ?query,
                );
                let next_action = self.decide_action(&query, &llm).await?;

                info!(event_name = "inline_agent_last_message_gpt",);
                // Send it to the answer sender so we can show it on the frontend
                if let Some(last_exchange) = self.last_agent_message() {
                    self.sender.send(last_exchange.clone()).await?;
                }
                return Ok(Some(next_action));
            }
            InLineAgentAction::Doc => {
                // If we are going to document something, then we go into
                // this flow here
                // First we update our state that we are now going to generate documentation
                let last_exchange;
                {
                    let last_exchange_ref = self.get_last_agent_message();
                    last_exchange_ref.add_agent_action(InLineAgentAction::Doc);
                    last_exchange = last_exchange_ref.clone();
                }
                // and send it over the sender too
                {
                    self.sender.send(last_exchange.clone()).await?;
                }
                // and then we start generating the documentation
                self.generate_documentation(answer_sender, &llm).await?;
                return Ok(None);
            }
            // For both the edit and the code we use the same functionality right
            // now, we will give them separate commands later on
            InLineAgentAction::Edit | InLineAgentAction::Code => {
                // First we update our state here
                let last_exchange;
                {
                    let last_exchange_ref = self.get_last_agent_message();
                    last_exchange_ref.add_agent_action(InLineAgentAction::Edit);
                    last_exchange = last_exchange_ref.clone();
                }
                // send it over the wire
                {
                    self.sender.send(last_exchange.clone()).await?;
                }
                // and then we start generating the edit and send it over
                self.process_edit(answer_sender, &llm).await?;
                return Ok(None);
            }
            InLineAgentAction::Fix => {
                let last_exchange;
                {
                    let last_exchange_ref = self.get_last_agent_message();
                    last_exchange_ref.add_agent_action(InLineAgentAction::Fix);
                    last_exchange = last_exchange_ref.clone();
                }
                // send it over the wire
                {
                    self.sender.send(last_exchange.clone()).await?;
                }
                // and then we start generating the fix and send it over
                self.process_fix(answer_sender, &llm).await?;
                return Ok(None);
            }
            _ => {
                self.apologise_message().await?;
                return Ok(None);
            }
        }
    }

    async fn decide_action(
        &mut self,
        query: &str,
        model: &LLMType,
    ) -> anyhow::Result<InLineAgentAction> {
        // Here we decide what we should do next based on the query
        dbg!("sidecar.inline_completion.decide_action");
        let system_prompt = prompts::decide_function_to_use(query);
        let request = LLMClientCompletionRequest::from_messages(
            vec![LLMClientMessage::system(system_prompt)],
            model.clone(),
        );
        let provider = self
            .editor_request
            .provider_for_fast_model()
            .ok_or(anyhow::anyhow!(
                "No provider found for fast model: {:?}",
                model
            ))?;
        let provider_config =
            self.editor_request
                .provider_config_for_slow_model()
                .ok_or(anyhow::anyhow!(
                    "No provider config found for fast model: {:?}",
                    model
                ))?;
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let response = self
            .get_llm_broker()
            .stream_completion(
                provider.clone(),
                request,
                provider_config.clone(),
                vec![("event_type".to_owned(), "decide_action".to_owned())]
                    .into_iter()
                    .collect(),
                sender,
            )
            .await?;
        let last_exchange = self.get_last_agent_message();
        // We add that we took a action to decide what we should do next
        last_exchange.add_agent_action(InLineAgentAction::DecideAction {
            query: query.to_owned(),
        });
        InLineAgentAction::from_gpt_response(response.answer_up_until_now())
    }

    async fn generate_documentation(
        &mut self,
        answer_sender: UnboundedSender<InLineAgentAnswer>,
        model: &LLMType,
    ) -> anyhow::Result<()> {
        // Now we get to the documentation generation loop, here we want to
        // first figure out what the context of the document is which we want
        // to generate the documentation for
        let source_str = self.editor_request.text_document_web.text.to_owned();
        let language = self.editor_request.text_document_web.language.to_owned();
        let relative_path = self
            .editor_request
            .text_document_web
            .relative_path
            .to_owned();
        let fs_file_path = self
            .editor_request
            .text_document_web
            .fs_file_path
            .to_owned();
        let start_position = self
            .editor_request
            .snippet_information
            .start_position
            .clone();
        let end_position = self.editor_request.snippet_information.end_position.clone();
        let request = self.editor_request.query.to_owned();
        let document_nodes = self.editor_parsing.get_documentation_node_for_range(
            &source_str,
            &language,
            &relative_path,
            &fs_file_path,
            &start_position,
            &end_position,
            &self.repo_ref,
        );
        let last_exchange = self.get_last_agent_message();
        if document_nodes.is_empty() {
            last_exchange.message_state = MessageState::Errored;
            answer_sender.send(InLineAgentAnswer {
                answer_up_until_now: "could not find documentation node".to_owned(),
                delta: Some("could not find documentation node".to_owned()),
                state: MessageState::Errored,
                document_symbol: None,
                context_selection: None,
                model: model.clone(),
            })?;
        } else {
            last_exchange.message_state = MessageState::StreamingAnswer;
            let messages_list = self.messages_for_documentation_generation(
                document_nodes,
                &language,
                &fs_file_path,
                &request,
            );
            let slow_model = self.editor_request.slow_model();
            let self_ = &*self;
            let provider =
                self_
                    .editor_request
                    .provider_for_slow_model()
                    .ok_or(anyhow::anyhow!(
                        "No provider found for fast model: {:?}",
                        model
                    ))?;
            let provider_config_slow = self_
                .editor_request
                .provider_config_for_slow_model()
                .ok_or(anyhow::anyhow!(
                    "No provider config found for fast model: {:?}",
                    model
                ))?;
            let answer_model = self.chat_broker.get_answer_model(&slow_model)?;
            let answer_tokens: usize = answer_model
                .answer_tokens
                .try_into()
                .expect("i64 positive to usize should work");
            dbg!("sidecar.inline_completion.generate_documentation");
            stream::iter(messages_list)
                .map(|messages| (messages, answer_sender.clone(), slow_model.clone()))
                .for_each(
                    |((doc_generation_request, document_symbol), answer_sender, slow_model)| async move {
                        let prompt = self_
                            .llm_prompt_formatter
                            .get_doc_prompt(&slow_model, doc_generation_request);
                        if let Err(_) = prompt {
                            return;
                        }
                        let context_selection = ContextSelection::generate_placeholder_for_range(&document_symbol.range());
                        let prompt = prompt.expect("if let Err above to hold");
                        // send the request to the llm client
                        let (sender, receiver) =
                            tokio::sync::mpsc::unbounded_channel::<LLMClientCompletionResponse>();
                        let receiver_stream =
                            UnboundedReceiverStream::new(receiver).map(Either::Left);
                        let llm_broker = self_.get_llm_broker().clone();
                        let answer_stream = {
                            match prompt {
                                InLinePromptResponse::Chat(chat) => {
                                    let request = LLMClientCompletionRequest::from_messages(
                                        chat,
                                        slow_model.clone(),
                                    ).set_temperature(0.2)
                                    .set_max_tokens(answer_tokens);
                                    llm_broker
                                        .stream_answer(
                                            provider.clone(),
                                            provider_config_slow.clone(),
                                            either::Left(request),
                                            vec![("event_type".to_owned(), "documentation".to_owned())].into_iter().collect(),
                                            sender,
                                        )
                                        .into_stream()
                                }
                                InLinePromptResponse::Completion(prompt) => {
                                    let request = LLMClientCompletionStringRequest::new(
                                        slow_model.clone(),
                                        prompt,
                                        0.2,
                                        None,
                                    ).set_max_tokens(answer_tokens);
                                    llm_broker
                                        .stream_answer(
                                            provider.clone(),
                                            provider_config_slow.clone(),
                                            either::Right(request),
                                            vec![("event_type".to_owned(), "documentation".to_owned())].into_iter().collect(),
                                            sender,
                                        )
                                        .into_stream()
                                }
                            }
                        }
                        .map(Either::Right);

                        let merged_stream = stream::select(receiver_stream, answer_stream);
                        // this worked out somehow?
                        pin_mut!(merged_stream);

                        // Play with the streams here so we can send incremental updates ASAP
                        while let Some(item) = merged_stream.next().await {
                            match item {
                                Either::Left(receiver_element) => {
                                    let _ = answer_sender.send(InLineAgentAnswer {
                                        answer_up_until_now: receiver_element
                                            .answer_up_until_now()
                                            .to_owned(),
                                        delta: receiver_element
                                            .delta()
                                            .map(|delta| delta.to_owned()),
                                        state: Default::default(),
                                        document_symbol: Some(document_symbol.clone()),
                                        context_selection: Some(context_selection.clone()),
                                        model: self_.editor_request.slow_model(),
                                    });
                                }
                                Either::Right(_) => {}
                            }
                        }
                    },
                )
                .await;
        }
        // here we can have a case where we didn't detect any documentation node
        // if that's the case we should just reply with not found
        Ok(())
    }

    async fn apologise_message(&mut self) -> anyhow::Result<()> {
        let last_exchange = self.get_last_agent_message();
        last_exchange.add_agent_action(InLineAgentAction::Unknown);
        Ok(())
    }

    async fn process_fix(
        &mut self,
        answer_sender: UnboundedSender<InLineAgentAnswer>,
        model: &LLMType,
    ) -> anyhow::Result<()> {
        let fixing_range_maybe = self.application.language_parsing.get_fix_range(
            self.editor_request.source_code(),
            self.editor_request.language(),
            &self.editor_request.snippet_information.to_range(),
            15,
        );
        let fixing_range =
            fixing_range_maybe.unwrap_or(self.editor_request.snippet_information.to_range());

        let split_lines = Regex::new(r"\r\n|\r|\n").unwrap();
        let source_lines: Vec<String> = split_lines
            .split(&self.editor_request.source_code())
            .map(|s| s.to_owned())
            .collect();
        let character_limit = 8000;
        let mut token_tracker = ContextWindowTracker::new(character_limit);
        // Now we try to generate the snippet information
        let selection_context = generate_selection_context_for_fix(
            <i64>::try_from(self.editor_request.line_count()).unwrap(),
            &fixing_range,
            &self.editor_request.snippet_information.to_range(),
            self.editor_request.language(),
            source_lines,
            self.editor_request.fs_file_path().to_owned(),
            &mut token_tracker,
        );
        let last_exchange = self.get_last_agent_message();
        last_exchange.message_state = MessageState::StreamingAnswer;
        let document_symbol = {
            let response_range = fixing_range;
            DocumentSymbol::for_edit(
                response_range.start_position(),
                response_range.end_position(),
            )
        };

        // Now we invoke the inline edit broker to get the response for the fixes

        let related_prompts = self.fix_diagnostics_prompt();
        let fix_request = self.inline_fix_request(
            self.editor_request.language(),
            &selection_context,
            related_prompts,
        );
        let answer_model = self.chat_broker.get_answer_model(&model)?;
        let answer_tokens: usize = answer_model
            .answer_tokens
            .try_into()
            .expect("i64 positive to usize should work");

        // Now we try to get the request we have to send from the inline edit broker
        let prompt = self
            .llm_prompt_formatter
            .get_fix_prompt(&model, fix_request)?;

        // send the request to the llm client
        let (sender, receiver) =
            tokio::sync::mpsc::unbounded_channel::<LLMClientCompletionResponse>();
        let receiver_stream = UnboundedReceiverStream::new(receiver).map(Either::Left);
        let llm_broker = self.get_llm_broker().clone();
        let provider = self
            .editor_request
            .provider_for_slow_model()
            .ok_or(anyhow::anyhow!(
                "No provider found for fast model: {:?}",
                model
            ))?;
        let provider_config =
            self.editor_request
                .provider_config_for_slow_model()
                .ok_or(anyhow::anyhow!(
                    "No provider config found for fast model: {:?}",
                    model
                ))?;
        let answer_stream = {
            match prompt {
                InLinePromptResponse::Chat(chat) => {
                    let request = LLMClientCompletionRequest::from_messages(chat, model.clone())
                        .set_max_tokens(answer_tokens);
                    llm_broker
                        .stream_answer(
                            provider.clone(),
                            provider_config.clone(),
                            either::Left(request),
                            vec![("event_type".to_owned(), "fix".to_owned())]
                                .into_iter()
                                .collect(),
                            sender,
                        )
                        .into_stream()
                }
                InLinePromptResponse::Completion(prompt) => {
                    let request =
                        LLMClientCompletionStringRequest::new(model.clone(), prompt, 0.0, None)
                            .set_max_tokens(answer_tokens);
                    llm_broker
                        .stream_answer(
                            provider.clone(),
                            provider_config.clone(),
                            either::Right(request),
                            vec![("event_type".to_owned(), "fix".to_owned())]
                                .into_iter()
                                .collect(),
                            sender,
                        )
                        .into_stream()
                }
            }
        }
        .map(Either::Right);

        let merged_stream = stream::select(receiver_stream, answer_stream);
        // this worked out somehow?
        pin_mut!(merged_stream);

        let context_selection = selection_context.to_context_selection();

        // Play with the streams here so we can send incremental updates ASAP
        while let Some(item) = merged_stream.next().await {
            match item {
                Either::Left(receiver_element) => {
                    let _ = answer_sender.send(InLineAgentAnswer {
                        answer_up_until_now: receiver_element.answer_up_until_now().to_owned(),
                        delta: receiver_element.delta().map(|delta| delta.to_owned()),
                        state: Default::default(),
                        document_symbol: Some(document_symbol.clone()),
                        context_selection: Some(context_selection.clone()),
                        model: self.editor_request.slow_model(),
                    });
                }
                Either::Right(_) => {}
            }
        }

        Ok(())
    }

    async fn process_edit(
        &mut self,
        answer_sender: UnboundedSender<InLineAgentAnswer>,
        _model: &LLMType,
    ) -> anyhow::Result<()> {
        dbg!("sidecar.inline_completion.process_edit");
        // Here we will try to process the edits
        // This is the current request selection range
        let selection_range = Range::new(
            self.editor_request.start_position(),
            self.editor_request.end_position(),
        );
        // Now we want to get the chunks properly
        // First we get the function blocks along with the ranges we know about
        // we get the function nodes here
        let function_nodes = self.editor_parsing.function_information_nodes(
            &self.editor_request.source_code_bytes(),
            &self.editor_request.language(),
        );
        dbg!("sidecar.function_nodes.len", &function_nodes.len());
        // Now we need to get the nodes which are just function blocks
        let mut function_blocks: Vec<_> = function_nodes
            .iter()
            .filter_map(|function_node| {
                if function_node.r#type() == &FunctionNodeType::Function {
                    Some(function_node)
                } else {
                    None
                }
            })
            .collect();
        // Now we sort the function blocks based on how close they are to the start index
        // of the code selection
        // we sort the nodes in increasing order
        function_blocks.sort_by(|a, b| a.range().start_byte().cmp(&b.range().start_byte()));

        // Next we need to get the function bodies
        let mut function_bodies: Vec<_> = function_nodes
            .iter()
            .filter_map(|function_node| {
                if function_node.r#type() == &FunctionNodeType::Body {
                    Some(function_node)
                } else {
                    None
                }
            })
            .collect();
        // Here we are sorting it in increasing order of start byte
        function_bodies.sort_by(|a, b| a.range().start_byte().cmp(&b.range().start_byte()));

        let expanded_selection = if self.editor_request.exact_selection() {
            selection_range.clone()
        } else {
            FunctionInformation::get_expanded_selection_range(
                function_blocks.as_slice(),
                &selection_range,
            )
        };

        let edit_expansion = EditExpandedSelectionRange::new(
            Range::guard_large_expansion(selection_range.clone(), expanded_selection.clone(), 30),
            expanded_selection.clone(),
            FunctionInformation::fold_function_blocks(
                function_bodies
                    .to_vec()
                    .into_iter()
                    .map(|x| x.clone())
                    .collect(),
            ),
        );

        // these are the missing variables I have to fill in,
        // lines count and the source lines
        let split_lines = Regex::new(r"\r\n|\r|\n").unwrap();
        let source_lines: Vec<String> = split_lines
            .split(&self.editor_request.source_code())
            .map(|s| s.to_owned())
            .collect();
        // generate the prompts for it and then send it over to the LLM
        let response = generate_context_for_range(
            self.editor_request.source_code_bytes().to_vec(),
            self.editor_request.line_count(),
            &selection_range,
            &expanded_selection,
            &edit_expansion.range_expanded_to_functions,
            &self.editor_request.language(),
            // TODO(skcd): Make this more variable going forward
            4000,
            source_lines,
            edit_expansion.function_bodies,
            self.editor_request.fs_file_path().to_owned(),
        );

        let selection_context = response.to_context_selection();

        // We create a fake document symbol which we will use to replace the
        // range which is present in the context of the selection
        let document_symbol = {
            let response_range = response.selection_context.get_selection_range();
            DocumentSymbol::for_edit(
                response_range.start_position(),
                response_range.end_position(),
            )
        };

        // which model are we going to use
        let slow_model = self.editor_request.slow_model();

        // inline edit prompt
        let inline_edit_request = self
            .inline_edit_request(
                self.editor_request.language(),
                response,
                &self.editor_request.query,
                &self.editor_request.user_context,
            )
            .await;
        // Now we try to get the request we have to send from the inline edit broker
        let prompt = self
            .llm_prompt_formatter
            .get_prompt(&slow_model, inline_edit_request)?;

        // Now that we have the user-messages we can send it over the wire
        let last_exchange = self.get_last_agent_message();
        last_exchange.message_state = MessageState::StreamingAnswer;

        // send the request to the llm client
        let (sender, receiver) =
            tokio::sync::mpsc::unbounded_channel::<LLMClientCompletionResponse>();
        let receiver_stream = UnboundedReceiverStream::new(receiver).map(Either::Left);
        let llm_broker = self.get_llm_broker().clone();
        let provider = self
            .editor_request
            .provider_for_slow_model()
            .ok_or(anyhow::anyhow!(
                "No provider found for fast model: {:?}",
                slow_model
            ))?;
        let provider_config =
            self.editor_request
                .provider_config_for_slow_model()
                .ok_or(anyhow::anyhow!(
                    "No provider config found for fast model: {:?}",
                    slow_model
                ))?;
        let answer_model = self.chat_broker.get_answer_model(&slow_model)?;
        let answer_tokens: usize = answer_model
            .answer_tokens
            .try_into()
            .expect("i64 positive to usize should work");

        let answer_stream = {
            match prompt {
                InLinePromptResponse::Chat(chat) => {
                    let request =
                        LLMClientCompletionRequest::from_messages(chat, slow_model.clone())
                            .set_temperature(0.2)
                            .set_max_tokens(answer_tokens);
                    llm_broker
                        .stream_answer(
                            provider.clone(),
                            provider_config.clone(),
                            either::Left(request),
                            vec![("event_type".to_owned(), "edit".to_owned())]
                                .into_iter()
                                .collect(),
                            sender,
                        )
                        .into_stream()
                }
                InLinePromptResponse::Completion(prompt) => {
                    let request = LLMClientCompletionStringRequest::new(
                        slow_model.clone(),
                        prompt,
                        0.2,
                        None,
                    )
                    .set_max_tokens(answer_tokens);
                    llm_broker
                        .stream_answer(
                            provider.clone(),
                            provider_config.clone(),
                            either::Right(request),
                            vec![("event_type".to_owned(), "edit".to_owned())]
                                .into_iter()
                                .collect(),
                            sender,
                        )
                        .into_stream()
                }
            }
        }
        .map(Either::Right);

        let merged_stream = stream::select(receiver_stream, answer_stream);
        // this worked out somehow?
        pin_mut!(merged_stream);

        // Play with the streams here so we can send incremental updates ASAP
        while let Some(item) = merged_stream.next().await {
            match item {
                Either::Left(receiver_element) => {
                    let _ = answer_sender.send(InLineAgentAnswer {
                        answer_up_until_now: receiver_element.answer_up_until_now().to_owned(),
                        delta: receiver_element.delta().map(|delta| delta.to_owned()),
                        state: Default::default(),
                        document_symbol: Some(document_symbol.clone()),
                        context_selection: Some(selection_context.clone()),
                        model: self.editor_request.slow_model(),
                    });
                }
                Either::Right(_) => {}
            }
        }

        Ok(())
    }

    pub fn messages_for_documentation_generation(
        &mut self,
        document_symbols: Vec<DocumentSymbol>,
        language: &str,
        file_path: &str,
        _query: &str,
    ) -> Vec<(InLineDocRequest, DocumentSymbol)> {
        document_symbols
            .into_iter()
            .map(|document_symbol| {
                let inline_doc_request = InLineDocRequest::new(
                    self.document_symbol_prompt(&document_symbol, language, file_path),
                    self.inline_doc_node(&document_symbol),
                    language.to_owned(),
                    file_path.to_owned(),
                );
                (inline_doc_request, document_symbol)
            })
            .collect::<Vec<_>>()
    }

    fn document_symbol_prompt(
        &self,
        document_symbol: &DocumentSymbol,
        language: &str,
        file_path: &str,
    ) -> String {
        let code = &document_symbol.code;
        let prompt_string = format!(
            r#"I have the following code in the selection:
```{language}
// FILEPATH: {file_path}
// BEGIN: ed8c6549bwf9
{code}
// END: ed8c6549bwf9
```"#
        );
        prompt_string
    }

    fn inline_doc_node(&self, document_symbol: &DocumentSymbol) -> InLineDocNode {
        match document_symbol.name.as_ref() {
            Some(name) => InLineDocNode::Node(name.to_owned()),
            None => InLineDocNode::Selection,
        }
    }

    fn fix_diagnostics_prompt(&self) -> Vec<String> {
        if let Some(diagnostics_information) = &self.editor_request.diagnostics_information {
            let first_message = &diagnostics_information.first_message;
            let related_information = diagnostics_information
                .diagnostic_information
                .iter()
                .map(|diagnostic| {
                    let prompt_parts = diagnostic.prompt_parts.to_vec();
                    let code_blocks: Vec<String> = diagnostic
                        .related_information
                        .iter()
                        .map(|related_information| {
                            let new_range = self
                                .application
                                .language_parsing
                                .get_parent_range_for_selection(
                                    &related_information.text,
                                    &related_information.language,
                                    &related_information.range,
                                );
                            let source_code = related_information.text
                                [new_range.start_byte()..new_range.end_byte()]
                                .to_owned();
                            wrap_in_code_block("", &source_code)
                        })
                        .collect();
                    if diagnostic.related_information.is_empty() {
                        prompt_parts.join("\n")
                    } else {
                        let mut answer = vec![prompt_parts.join("\n")];
                        answer.push("This diagnostic has some related code:".to_owned());
                        answer.extend(code_blocks.into_iter());
                        answer.join("\n")
                    }
                })
                .collect::<Vec<_>>();
            {
                vec![format!(
                    "{}\n{}",
                    first_message,
                    related_information.join("\n")
                )]
            }
        } else {
            vec![]
        }
    }

    fn inline_fix_request(
        &self,
        language: &str,
        selection: &SelectionContext,
        diagnostics: Vec<String>,
    ) -> InLineFixRequest {
        let mut above_context = None;
        let mut below_context = None;
        if selection.above.has_context() {
            let mut above_prompts = vec![];
            above_prompts.extend(selection.above.generate_prompt(true));
            above_context = Some(above_prompts.join("\n"));
        }

        if selection.below.has_context() {
            let mut below_prompts = vec![];
            below_prompts.extend(selection.below.generate_prompt(true));
            below_context = Some(below_prompts.join("\n"));
        }

        let in_range = selection.range.generate_prompt(true).join("\n");

        InLineFixRequest::new(
            above_context,
            below_context,
            in_range,
            diagnostics,
            language.to_owned(),
            selection.range.fs_file_path().to_owned(),
        )
    }

    /// Generate the inline edit request
    async fn inline_edit_request(
        &self,
        language: &str,
        selection_with_outline: SelectionWithOutlines,
        user_query: &str,
        user_context: &UserContext,
    ) -> InLineEditRequest {
        let mut above_context = None;
        let mut below_context = None;
        let has_surrounding_context = selection_with_outline.selection_context.above.has_context()
            || selection_with_outline.selection_context.below.has_context()
            || !selection_with_outline.outline_above.is_empty()
            || !selection_with_outline.outline_below.is_empty();

        let prompt_with_outline =
            |outline: String, fs_file_path: &str, start_marker: &str, end_marker: &str| -> String {
                return vec![
                    format!("```{language}"),
                    start_marker.to_owned(),
                    format!("// FILEPATH: {fs_file_path}"),
                    outline,
                    end_marker.to_owned(),
                    "```".to_owned(),
                ]
                .join("\n");
            };

        let prompt_with_content = |context: &ContextParserInLineEdit| -> String {
            let prompt_parts = context.generate_prompt(has_surrounding_context);
            let mut answer = vec![];
            answer.extend(prompt_parts.into_iter());
            answer.join("\n").trim().to_owned()
        };

        if !selection_with_outline.outline_above.is_empty() {
            above_context = Some(prompt_with_outline(
                selection_with_outline.outline_above.to_owned(),
                self.editor_request.fs_file_path(),
                selection_with_outline
                    .selection_context
                    .above
                    .start_marker(),
                selection_with_outline.selection_context.above.end_marker(),
            ));
        }

        if selection_with_outline.selection_context.above.has_context() {
            above_context = Some(prompt_with_content(
                &selection_with_outline.selection_context.above,
            ));
        }

        if selection_with_outline.selection_context.below.has_context() {
            below_context = Some(prompt_with_content(
                &selection_with_outline.selection_context.below,
            ));
        }

        if !selection_with_outline.outline_below.is_empty() {
            below_context = Some(prompt_with_outline(
                selection_with_outline.outline_below.trim().to_owned(),
                self.editor_request.fs_file_path(),
                selection_with_outline
                    .selection_context
                    .below
                    .start_marker(),
                selection_with_outline.selection_context.below.end_marker(),
            ));
        }

        let mut selection_prompt = vec![];
        if selection_with_outline.selection_context.range.has_context() {
            selection_prompt.extend(
                selection_with_outline
                    .selection_context
                    .range
                    .generate_prompt(has_surrounding_context)
                    .into_iter(),
            );
        } else {
            let fs_file_path = self.editor_request.fs_file_path();
            selection_prompt.push(format!("```{language}"));
            selection_prompt.push("// BEGIN".to_owned());
            selection_prompt.push(format!("// FILEPATH: {fs_file_path}"));
            selection_prompt.push("// END".to_owned());
            selection_prompt.push("```".to_owned());
        }
        let in_range_context = Some(selection_prompt.join("\n"));
        InLineEditRequest::new(
            above_context,
            below_context,
            in_range_context,
            user_query.to_owned(),
            selection_with_outline.fs_file_path(),
            // TODO(skcd): Check the implementation of this later on
            vec![user_context
                .clone()
                .to_xml(Default::default())
                .await
                .expect("to work")],
            language.to_owned(),
        )
    }
}

fn wrap_in_code_block(t: &str, e: &str) -> String {
    let re = regex::Regex::new(r"^\s*(```+)").unwrap();
    let captures = re.captures_iter(e);

    let max_length = captures.map(|cap| cap[1].len() + 1).max().unwrap_or(3);

    let i = "`".repeat(max_length);

    format!("{}{}\n{}\n{}", i, t, e.trim(), i)
}
