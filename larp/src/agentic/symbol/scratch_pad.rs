//! Contains the scratch pad agent whose job is to work alongside the developer
//! and help them accomplish a task
//! This way the agent can look at all the events and the requests which are happening
//! and take a decision based on them on what should happen next

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    pin::Pin,
    sync::Arc,
    time::Instant,
};

use futures::{stream, Stream, StreamExt};
use llm_client::{
    clients::types::LLMType,
    provider::{AnthropicAPIKey, LLMProvider, LLMProviderAPIKeys},
};
use tokio::{
    io::AsyncWriteExt,
    sync::{mpsc::UnboundedSender, Mutex},
};

use crate::{
    agentic::{
        symbol::{
            events::{
                edit::SymbolToEditRequest, initial_request::SymbolEditedItem, types::SymbolEvent,
            },
            identifier::LLMProperties,
            ui_event::{InitialSearchSymbolInformation, UIEventWithID},
        },
        tool::{
            helpers::diff_recent_changes::DiffFileContent, output::ToolOutput, r#type::Tool,
            session::chat::SessionChatMessage,
        },
    },
    chunking::{
        languages::TSLanguageParsing,
        text_document::{Position, Range},
    },
};

use super::{
    errors::SymbolError,
    events::{
        agent::{AgentIntentMessage, AgentMessage},
        edit::SymbolToEdit,
        environment_event::{EditorStateChangeRequest, EnvironmentEvent, EnvironmentEventType},
        human::{HumanAgenticRequest, HumanAnchorRequest, HumanMessage},
        input::SymbolInputEvent,
        lsp::{LSPDiagnosticError, LSPSignal},
        message_event::{SymbolEventMessage, SymbolEventMessageProperties},
    },
    identifier::SymbolIdentifier,
    tool_box::ToolBox,
    tool_properties::ToolProperties,
    types::SymbolEventRequest,
};

#[derive(Debug, Clone)]
struct ScratchPadFilesActive {
    _file_content: String,
    _file_path: String,
}

impl ScratchPadFilesActive {
    fn _new(file_content: String, file_path: String) -> Self {
        Self {
            _file_content: file_content,
            _file_path: file_path,
        }
    }

    fn _to_diff_active_file(self) -> DiffFileContent {
        DiffFileContent::new(self._file_path, self._file_content, None)
    }
}

// We should have a way to update our cache of all that has been done
// and what we are upto right now
// the ideal goal would be to rewrite the scratchpad in a good way so we are
// able to work on top of that
// a single LLM call should rewrite the sections which are present and take as input
// the lsp signal
// we also need to tell symbol_event_request agent what all things are possible, like: getting data from elsewhere
// looking at some other file and keeping that in its cache
// also what kind of information it should keep in:
// it can be state driven based on the user ask
// there will be files which the system has to keep in context, which can be dynamic as well
// we have to control it to not go over the 50kish limit ... cause it can grow by a lot
// but screw it, we keep it as it is
// lets keep it free-flow before we figure out the right way to go about doing this
// mega-scratchpad ftw
// Things to do:
// - [imp] how do we keep the cache hot after making updates or discovering new information, we want to keep the prefix hot and consistenet always
// - [not_sure] when recieving a LSP signal we might want to edit or gather more information how do we go about doing that?
// - can we get the user behavior to be about changes done in the past and what effects it has
// - meta programming on the canvas maybe in some ways?
// - can we just start tracking the relevant edits somehow.. just that
// - would go a long way most probably
// - help us prepare for now
// - even better just show the git diff until now
// - even dumber just run git-diff and store it as context anyways?
// - we need access to the root directory for git here

/// Different kind of events which can happen
/// We should move beyond symbol events tbh at this point :')

#[derive(Clone)]
pub struct ScratchPadAgent {
    _storage_fs_path: String,
    tool_box: Arc<ToolBox>,
    // if the scratch-pad agent is right now focussed, then we can't react to other
    // signals and have to pay utmost attention to the current task we are workign on
    focussing: Arc<Mutex<bool>>,
    fixing: Arc<Mutex<bool>>,
    // we store the previous user queries as a vec<string> here so we can show that to
    // the llm when its running inference
    previous_user_queries: Arc<Mutex<Vec<String>>>,
    symbol_event_sender: UnboundedSender<SymbolEventMessage>,
    // This is the cache which we have to send with every request
    _files_context: Arc<Mutex<Vec<ScratchPadFilesActive>>>,
    // This is the extra context which we send everytime with each request
    // this also helps with the prompt cache hits
    extra_context: Arc<Mutex<String>>,
    reaction_sender: UnboundedSender<EnvironmentEventType>,
}

impl ScratchPadAgent {
    pub async fn new(
        scratch_pad_path: String,
        tool_box: Arc<ToolBox>,
        symbol_event_sender: UnboundedSender<SymbolEventMessage>,
        user_provided_context: Option<String>,
    ) -> Self {
        let (reaction_sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let scratch_pad_agent = Self {
            _storage_fs_path: scratch_pad_path,
            tool_box,
            symbol_event_sender,
            focussing: Arc::new(Mutex::new(false)),
            fixing: Arc::new(Mutex::new(false)),
            previous_user_queries: Arc::new(Mutex::new(vec![])),
            _files_context: Arc::new(Mutex::new(vec![])),
            extra_context: Arc::new(Mutex::new(user_provided_context.unwrap_or_default())),
            reaction_sender,
        };
        // let cloned_scratch_pad_agent = scratch_pad_agent.clone();
        let mut reaction_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);

        // we also want a timer event here which can fetch lsp signals ad-hoc and as required
        tokio::spawn(async move {
            while let Some(reaction_event) = reaction_stream.next().await {
                if reaction_event.is_shutdown() {
                    break;
                }
                // we are not going to react the events right now
                // let _ = cloned_scratch_pad_agent
                //     .react_to_event(reaction_event)
                //     .await;
            }
        });
        scratch_pad_agent
    }

    /// Starts the scratch pad agent and returns the environment sender
    /// which can be used to talk to these agents
    pub async fn start_scratch_pad(
        scratch_pad_file_path: PathBuf,
        tool_box: Arc<ToolBox>,
        symbol_event_sender: UnboundedSender<SymbolEventMessage>,
        _message_properties: SymbolEventMessageProperties,
        user_provided_context: Option<String>,
    ) -> (Self, UnboundedSender<EnvironmentEvent>) {
        let mut scratch_pad_file = tokio::fs::File::create(scratch_pad_file_path.clone())
            .await
            .expect("scratch_pad path created");
        let _ = scratch_pad_file
            .write_all("<scratchpad>\n</scratchpad>".as_bytes())
            .await;
        let _ = scratch_pad_file
            .flush()
            .await
            .expect("initiating scratch pad failed");

        let scratch_pad_path = scratch_pad_file_path
            .into_os_string()
            .into_string()
            .expect("os_string to into_string to work");
        let scratch_pad_agent = ScratchPadAgent::new(
            scratch_pad_path,
            tool_box,
            symbol_event_sender,
            user_provided_context.clone(),
        )
        .await;
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let cloned_scratch_pad_agent = scratch_pad_agent.clone();
        let _scratch_pad_handle = tokio::spawn(async move {
            // spawning the scratch pad agent
            cloned_scratch_pad_agent
                .process_envrionment(Box::pin(
                    tokio_stream::wrappers::UnboundedReceiverStream::new(receiver),
                ))
                .await;
        });
        (scratch_pad_agent, sender)
    }
}

impl ScratchPadAgent {
    /// We try to contain all the events which are coming in from the symbol
    /// which is being edited by the user, the real interface here will look like this
    pub async fn process_envrionment(
        self,
        stream: Pin<Box<dyn Stream<Item = EnvironmentEvent> + Send + Sync>>,
    ) {
        let cloned_self = self.clone();

        let filtered_stream = stream
            .filter_map(move |event| {
                let cloned_self = cloned_self.clone();
                async move {
                    if event.is_lsp_event() {
                        if cloned_self.is_fixing().await {
                            println!("scratchpad::discarding_lsp::busy_fixing");
                            None
                        } else if cloned_self.is_focussing().await {
                            println!("scratchpad::discarding_lsp::busy_focussing");
                            None
                        } else {
                            Some(event)
                        }
                    } else {
                        Some(event)
                    }
                }
            })
            .boxed();

        let mut stream = filtered_stream;

        println!("scratch_pad_agent::start_processing_environment");
        while let Some(event) = stream.next().await {
            let message_properties = event.message_properties();
            let event = event.event_type();
            match event {
                EnvironmentEventType::LSP(lsp_signal) => {
                    // we just want to react to the lsp signal over here, so we do just that
                    // if we are fixing or if we are focussing
                    if self.is_fixing().await {
                        println!("scratchpad::environment_event::discarding_lsp::busy_fixing");
                        continue;
                    }
                    if self.is_focussing().await {
                        println!("scratchpad::environment_event::discarding_lsp::busy_focussing");
                        continue;
                    }
                    let _ = self
                        .reaction_sender
                        .send(EnvironmentEventType::LSP(lsp_signal));
                }
                EnvironmentEventType::Human(message) => {
                    let _ = self.handle_human_message(message, message_properties).await;
                    // whenever the human sends a request over here, encode it and try
                    // to understand how to handle it, some might require search, some
                    // might be more automagic
                }
                EnvironmentEventType::Symbol(_symbol_event) => {
                    // we know a symbol is going to be edited, what should we do about it?
                }
                EnvironmentEventType::EditorStateChange(_) => {
                    // not sure what to do about this right now, this event is used so the
                    // scratchpad can react to it, so for now do not do anything
                    // we might have to split the events later down the line
                }
                EnvironmentEventType::Agent(agent_message) => {
                    let _ = self
                        .handle_agent_message(agent_message, message_properties)
                        .await;
                }
                EnvironmentEventType::ShutDown => {
                    println!("scratch_pad_agent::shut_down");
                    let _ = self.reaction_sender.send(EnvironmentEventType::ShutDown);
                    break;
                }
            }
        }
    }

    async fn _react_to_event(
        &self,
        event: EnvironmentEventType,
        message_properties: SymbolEventMessageProperties,
    ) {
        match event {
            EnvironmentEventType::Human(human_event) => {
                let _ = self
                    ._react_to_human_event(human_event, message_properties)
                    .await;
            }
            EnvironmentEventType::EditorStateChange(editor_state_change) => {
                self._react_to_edits(editor_state_change, message_properties)
                    .await;
            }
            EnvironmentEventType::LSP(lsp_signal) => {
                self._react_to_lsp_signal(lsp_signal, message_properties)
                    .await;
            }
            _ => {}
        }
    }

    async fn handle_agent_message(
        &self,
        agent_message: AgentMessage,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        match agent_message {
            AgentMessage::ReferenceCheck(reference_check) => {
                let cloned_self = self.clone();
                let _ = tokio::spawn(async move {
                    cloned_self
                        .agent_reference_check(reference_check, message_properties)
                        .await
                });
                Ok(())
            }
        }
    }

    async fn handle_human_message(
        &self,
        human_message: HumanMessage,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        match human_message {
            HumanMessage::Anchor(anchor_request) => {
                self.human_message_anchor(anchor_request, message_properties)
                    .await
            }
            HumanMessage::Agentic(agentic_request) => {
                self.human_message_agentic(agentic_request, message_properties)
                    .await
            }
            HumanMessage::Followup(_followup_request) => Ok(()),
        }
    }

    async fn _react_to_human_event(
        &self,
        human_event: HumanMessage,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        match human_event {
            HumanMessage::Anchor(anchor_request) => {
                let _ = self
                    ._handle_user_anchor_request(anchor_request, message_properties)
                    .await;
            }
            HumanMessage::Agentic(_) => {}
            HumanMessage::Followup(_followup_request) => {}
        }
        Ok(())
    }

    async fn agent_reference_check(
        &self,
        agent_message: AgentIntentMessage,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        let start = Instant::now();
        let user_query = agent_message.get_user_intent().to_owned();
        let anchored_symbols = agent_message.anchor_symbols();

        let references = stream::iter(anchored_symbols.into_iter())
            .flat_map(|anchored_symbol| {
                let symbol_names = anchored_symbol.sub_symbol_names().to_vec();
                let symbol_identifier = anchored_symbol.identifier().to_owned();
                let toolbox = self.tool_box.clone();
                let message_properties = message_properties.clone();
                let request_id = message_properties.request_id_str().to_owned();
                let range = anchored_symbol.possible_range().clone();
                stream::iter(symbol_names.into_iter().filter_map(move |symbol_name| {
                    symbol_identifier.fs_file_path().map(|path| {
                        (
                            anchored_symbol.clone(),
                            path,
                            symbol_name,
                            toolbox.clone(),
                            message_properties.clone(),
                            request_id.clone(),
                            range.clone(),
                        )
                    })
                }))
            })
            .map(
                |(
                    original_symbol,
                    path,
                    symbol_name,
                    toolbox,
                    message_properties,
                    request_id,
                    range,
                )| async move {
                    println!("getting references for {}-{}", &path, &symbol_name);
                    let refs = toolbox
                        .get_symbol_references(
                            path,
                            symbol_name.to_owned(),
                            range,
                            message_properties.clone(),
                            request_id,
                        )
                        .await;

                    match refs {
                        Ok(references) => {
                            toolbox
                                .anchored_references_for_locations(
                                    references.as_slice(),
                                    original_symbol,
                                    message_properties,
                                )
                                .await
                        }
                        Err(e) => {
                            println!("{:?}", e);
                            vec![]
                        }
                    }
                },
            )
            .buffer_unordered(100)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();

        println!("total references: {}", references.len());
        println!("collect references time elapsed: {:?}", start.elapsed());

        // send UI event with grouped references
        let grouped: HashMap<String, usize> =
            references
                .clone()
                .into_iter()
                .fold(HashMap::new(), |mut acc, anchored_reference| {
                    let reference_len = anchored_reference.reference_locations().len();
                    acc.entry(
                        anchored_reference
                            .fs_file_path_for_outline_node()
                            .to_string(),
                    )
                    .and_modify(|count| *count += reference_len)
                    .or_insert(1);
                    acc
                });

        let _ = message_properties
            .ui_sender()
            .send(UIEventWithID::found_reference(
                message_properties.request_id_str().to_owned(),
                grouped,
            ));

        let _ = self
            .tool_box
            .reference_filtering(&user_query, references, message_properties.clone())
            .await;
        Ok(())
    }

    pub async fn human_message_agentic(
        &self,
        human_agentic_request: HumanAgenticRequest,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        let cache;
        {
            cache = self.extra_context.lock().await.clone();
        }
        let previous_user_queries;
        {
            previous_user_queries = self.previous_user_queries.lock().await.to_vec();
        }
        let deep_reasoning = human_agentic_request.deep_reasoning();
        println!(
            "scratch_pad::human_message_agentic::deep_reasoning({})",
            deep_reasoning
        );
        let user_context = human_agentic_request.user_context().clone();
        let user_query = human_agentic_request.user_query().to_owned();
        let root_directory = human_agentic_request.root_directory().to_owned();
        let codebase_search = human_agentic_request.codebase_search();
        let edit_request_id = message_properties.request_id_str().to_owned();
        let aide_rules = human_agentic_request.aide_rules().clone();
        let ui_sender = message_properties.ui_sender();
        let start_instant = std::time::Instant::now();
        let mut input_event = SymbolInputEvent::new(
            user_context,
            message_properties.llm_properties().llm().clone(),
            message_properties.llm_properties().provider().clone(),
            message_properties.llm_properties().api_key().clone(),
            user_query.to_owned(),
            edit_request_id.to_owned(),
            edit_request_id,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            true,
            Some(root_directory),
            None,
            codebase_search, // big search
            ui_sender,
        );
        let ui_sender = input_event.ui_sender();
        let request_id = input_event.request_id().to_owned();
        let is_full_edit = input_event.full_symbol_edit();
        let is_big_search = input_event.big_search();
        let _swe_bench_id = input_event.swe_bench_instance_id();
        let _swe_bench_git_dname = input_event.get_swe_bench_git_dname();
        let swe_bench_test_endpoint = input_event.get_swe_bench_test_endpoint();
        let swe_bench_code_editing_model = input_event.get_swe_bench_code_editing();
        let swe_bench_gemini_properties = input_event.get_swe_bench_gemini_llm_properties();
        let swe_bench_long_context_editing = input_event.get_swe_bench_long_context_editing();
        let full_symbol_edit = input_event.full_symbol_edit();
        let fast_code_symbol_llm = input_event.get_fast_code_symbol_llm();
        let tool_properties = ToolProperties::new()
            .set_swe_bench_endpoint(swe_bench_test_endpoint.clone())
            .set_swe_bench_code_editing_llm(swe_bench_code_editing_model)
            .set_swe_bench_reranking_llm(swe_bench_gemini_properties)
            .set_long_context_editing_llm(swe_bench_long_context_editing)
            .set_full_symbol_request(full_symbol_edit)
            .set_fast_code_symbol_search(fast_code_symbol_llm);
        // if we have deep reasoning then we should use o1 over here
        // make this happen
        if deep_reasoning {
            println!("scratch_pad_agent::planning_with_deep_reasoning");
            let planned_out_reasoning = self
                .tool_box
                .reasoning(
                    user_query,
                    human_agentic_request.user_context().file_paths(),
                    aide_rules.clone(),
                    &self._storage_fs_path,
                    message_properties.clone(),
                )
                .await;
            println!(
                "scratch_pad_agent::planning_with_deep_reasoning::reasoning::is_ok({})",
                planned_out_reasoning.is_ok()
            );
            if let Ok(planned_out_reasoning) = planned_out_reasoning {
                input_event = input_event.set_user_query(planned_out_reasoning);
            }
        }
        let user_query = input_event.user_query().to_owned();
        let recent_edits = self
            .tool_box
            .recently_edited_files(Default::default(), message_properties.clone())
            .await?;
        let lsp_diagnostics = self
            .tool_box
            .get_lsp_diagnostics_prompt_format(
                human_agentic_request.user_context().file_paths(),
                message_properties.clone(),
            )
            .await?;
        let tool_input = input_event
            .tool_use_on_initial_invocation(
                recent_edits.l2_changes().to_owned(),
                lsp_diagnostics,
                message_properties.clone(),
            )
            .await;
        let tool_output = if let Some(tool_input) = tool_input {
            {
                if tool_input.is_repo_map_search() {
                    let _ = ui_sender.send(UIEventWithID::start_long_context_search(
                        request_id.to_owned(),
                    ));
                    let result = self
                        .tool_box
                        .tools()
                        .invoke(tool_input)
                        .await
                        .map_err(|e| SymbolError::ToolError(e));
                    let _ = ui_sender.send(UIEventWithID::finish_long_context_search(
                        request_id.to_owned(),
                    ));
                    result?
                } else {
                    self.tool_box
                        .tools()
                        .invoke(tool_input)
                        .await
                        .map_err(|e| SymbolError::ToolError(e))?
                }
            }
        } else {
            println!("no tool found for the agentic editing start, this is super fucked");
            return Ok(());
        };

        if let ToolOutput::ImportantSymbols(important_symbols)
        | ToolOutput::RepoMapSearch(important_symbols)
        | ToolOutput::BigSearch(important_symbols) = tool_output
        {
            // The fix symbol name here helps us get the top-level symbol name
            // if the LLM decides to have fun and spit out a.b.c instead of a or b or c individually
            // as it can with python where it will tell class.method_name instead of just class or just
            // method_name
            let llm_properties = LLMProperties::new(
                LLMType::ClaudeSonnet,
                LLMProvider::Anthropic,
                LLMProviderAPIKeys::Anthropic(AnthropicAPIKey::new("".to_owned())),
            );

            let ts_parsing = Arc::new(TSLanguageParsing::init());

            // should pass self.editorparsing <tsconfigs>
            let important_symbols = important_symbols.fix_symbol_names(ts_parsing.clone());

            // Debug printing
            println!("Important symbols: {:?}", &important_symbols);

            println!("symbol_manager::planning_before_editing");
            let important_symbols = {
                if is_full_edit && !is_big_search {
                    important_symbols
                } else {
                    let important_symbols = self
                        .tool_box
                        .planning_before_code_editing(
                            &important_symbols,
                            &user_query,
                            llm_properties.clone(),
                            is_big_search,
                            message_properties.clone(),
                        )
                        .await?
                        .fix_symbol_names(ts_parsing);
                    important_symbols
                }
            };

            println!("symbol_manager::plan_finished_before_editing");

            let updated_tool_properties = important_symbols.ordered_symbols_to_plan();
            let tool_properties = tool_properties
                .clone()
                .set_plan_for_input(Some(updated_tool_properties));
            let tool_properties_ref = &tool_properties;
            println!(
                "symbol_manager::tool_box::important_symbols::search({})",
                important_symbols
                    .ordered_symbols()
                    .into_iter()
                    .map(|code_symbol| code_symbol.code_symbol())
                    .collect::<Vec<_>>()
                    .join(",")
            );

            // Lets first start another round of COT over here to figure out
            // how to go about making the changes, I know this is a bit orthodox
            // and goes against our plans of making the agents better, but
            // this feels useful to have, since the previous iteration
            // does not even see the code and what changes need to be made
            let mut symbols = self
                .tool_box
                .important_symbols_per_file(&important_symbols, message_properties.clone())
                .await
                .map_err(|e| e.into())?;

            // send a UI event to the frontend over here
            let mut initial_symbol_search_information = vec![];
            for (symbol, _) in symbols.iter() {
                initial_symbol_search_information.push(InitialSearchSymbolInformation::new(
                    symbol.symbol_name().to_owned(),
                    // TODO(codestory): umm.. how can we have a file path for a symbol
                    // which does not exist if is_new is true
                    Some(symbol.fs_file_path().to_owned()),
                    symbol.is_new(),
                    symbol.steps().await.join("\n"),
                    symbol
                        .get_snippet()
                        .await
                        .map(|snippet| snippet.range().clone()),
                ));
            }
            let _ = ui_sender.send(UIEventWithID::initial_search_symbol_event(
                request_id.to_owned(),
                initial_symbol_search_information,
            ));
            // TODO(skcd): Another check over here is that we can search for the exact variable
            // and then ask for the edit
            println!(
                "symbol_manager::initial_request::symbols::({})",
                symbols
                    .iter()
                    .map(|(symbol, _)| symbol.symbol_name().to_owned())
                    .collect::<Vec<_>>()
                    .join(",")
            );

            let symbols_edited_list = important_symbols
                .ordered_symbols()
                .into_iter()
                .map(|symbol| {
                    SymbolEditedItem::new(
                        symbol.code_symbol().to_owned(),
                        symbol.file_path().to_owned(),
                        symbol.is_new(),
                        symbol.steps().to_vec().join("\n"),
                    )
                })
                .collect::<Vec<_>>();
            // TODO(skcd): the symbol here might belong to a class or it might be a global function
            // we want to grab the largest node containing the symbol here instead of using
            // the symbol directly since our algorithm would not work otherwise
            // we would also need to de-duplicate the symbols which we have to process right over here
            // otherwise it might lead to errors
            if symbols.is_empty() {
                println!("symbol_manager::grab_symbols_using_search");
                symbols = self
                    .tool_box
                    .grab_symbol_using_search(important_symbols, message_properties.clone())
                    .await
                    .map_err(|e| e.into())?;
            }

            println!("symbol_manager::symbols_len::({})", symbols.len());

            // This is where we are creating all the symbols
            let _ = stream::iter(
                // we are loosing context about the changes which we want to make
                // to the symbol over here
                symbols.into_iter().map(|symbol| {
                    (
                        symbol,
                        user_query.to_owned(),
                        symbols_edited_list.to_vec(),
                        cache.to_owned(),
                        previous_user_queries.to_vec(),
                        message_properties.clone(),
                    )
                }),
            )
            .map(
                |(
                    (symbol_request, steps),
                    user_query,
                    _symbols_edited_list,
                    cache,
                    previous_user_queries,
                    message_properties,
                )| async move {
                    let symbol_identifier = symbol_request.to_symbol_identifier_with_file_path();
                    {
                        // TODO(codestory+caching): We should be sending the edit request directly
                        // we are not providing any data over here
                        let symbol_event = SymbolEvent::Edit(SymbolToEditRequest::new(
                            vec![SymbolToEdit::new(
                                symbol_identifier.symbol_name().to_owned(),
                                Range::new(Position::new(0, 0, 0), Position::new(100000, 0, 0)),
                                symbol_identifier.fs_file_path().unwrap_or_default(),
                                steps,
                                false,
                                false,
                                true,
                                user_query.to_owned(),
                                None,
                                false,
                                Some(cache),
                                true, // we want to have code correctness
                                None,
                                previous_user_queries,
                                None,
                            )],
                            symbol_identifier.clone(),
                            vec![],
                        ));
                        let symbol_event_request = SymbolEventRequest::new(
                            symbol_identifier.clone(),
                            symbol_event,
                            tool_properties_ref.clone(),
                        );
                        let (sender, receiver) = tokio::sync::oneshot::channel();
                        println!(
                            "symbol_manager::initial_request::sending_request({})",
                            symbol_identifier.symbol_name()
                        );
                        let symbol_event = SymbolEventMessage::message_with_properties(
                            symbol_event_request,
                            message_properties.clone(),
                            sender,
                        );
                        let _ = self.symbol_event_sender.send(symbol_event);
                        let _ = receiver.await;
                    }
                },
            )
            // TODO(codestory): We should play with the parallelism over here
            .buffered(1)
            .collect::<Vec<_>>()
            .await;
        }
        println!("scratch_pad_agent::agentic_editing::finish");
        println!(
            "scratch_pad::agentic_editing::time_taken({}ms)",
            start_instant.elapsed().as_millis()
        );
        let _ = ui_sender.send(UIEventWithID::finish_edit_request(request_id));
        Ok(())
    }

    pub async fn anchor_editing_on_range(
        &self,
        range: Range,
        fs_file_path: String,
        query: String,
        converted_messages: Vec<SessionChatMessage>,
        user_context_str: String,
        aide_rules: Option<String>,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<String, SymbolError> {
        println!("scratch_pad_agent::anchor_editing_on_range::start");
        // We want to send the content of the files which we have seen before as they
        // are, this makes sure that we always get the latest git-diff, in case of reverts
        // which happen by the human outside of the scope of the files we are interested in
        let recent_edits = self
            .tool_box
            .recently_edited_files(
                vec![fs_file_path.to_owned()].into_iter().collect(),
                message_properties.clone(),
            )
            .await?;
        println!("scratch_pad_agent::human_message_anchor::recent_edits::done");
        let symbol_to_edit_request = SymbolToEditRequest::new(
            vec![SymbolToEdit::new(
                fs_file_path.to_owned(),
                range.clone(),
                fs_file_path.to_owned(),
                vec![query.to_owned()],
                false,
                false,
                true,
                query.to_owned(),
                None,
                false,
                Some(user_context_str),
                true,
                Some(recent_edits.clone()),
                vec![],
                None,
            )
            .set_previous_messages(converted_messages)
            .set_aide_rules(aide_rules)],
            SymbolIdentifier::with_file_path(&fs_file_path, &fs_file_path),
            vec![],
        );
        let symbol_event_sender = self.symbol_event_sender.clone();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let symbol_event_request = SymbolEventRequest::new(
            symbol_to_edit_request.symbol_identifier().clone(),
            SymbolEvent::Edit(symbol_to_edit_request), // defines event type
            ToolProperties::new(),
        );
        let event = SymbolEventMessage::message_with_properties(
            symbol_event_request,
            message_properties,
            sender,
        );
        let _ = symbol_event_sender.send(event);
        let response = receiver.await.map_err(|e| SymbolError::RecvError(e))?;
        Ok(response.to_string())
    }

    async fn human_message_anchor(
        &self,
        anchor_request: HumanAnchorRequest,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<(), SymbolError> {
        let start_instant = std::time::Instant::now();
        println!("scratch_pad_agent::human_message_anchor::start");
        let anchored_symbols = anchor_request.anchored_symbols();

        // We want to send the content of the files which we have seen before as they
        // are, this makes sure that we always get the latest git-diff, in case of reverts
        // which happen by the human outside of the scope of the files we are interested in
        let recent_edits = self
            .tool_box
            .recently_edited_files_with_content(
                anchored_symbols
                    .iter()
                    .filter_map(|anchor_symbol| anchor_symbol.fs_file_path())
                    .collect(),
                vec![],
                message_properties.clone(),
            )
            .await?;
        println!("scratch_pad_agent::human_message_anchor::recent_edits::done");
        // keep track of the user request in our state
        let previous_user_queries;
        {
            let mut user_queries = self.previous_user_queries.lock().await;
            previous_user_queries = user_queries.to_vec();
            user_queries.push(anchor_request.to_string());
        }

        let symbols_to_edit_request = self
            .tool_box
            .symbol_to_edit_request(
                anchored_symbols,
                anchor_request.user_query(),
                anchor_request.anchor_request_context(),
                recent_edits,
                previous_user_queries,
                message_properties.clone(),
            )
            .await?;

        let cloned_anchored_request = anchor_request.clone();
        // we are going to react to the user message
        let _ = self
            .reaction_sender
            .send(EnvironmentEventType::Human(HumanMessage::Anchor(
                cloned_anchored_request,
            )));

        // we start making the edits
        {
            let mut focussed = self.focussing.lock().await;
            *focussed = true;
        }
        let edits_done = stream::iter(symbols_to_edit_request.into_iter().map(|data| {
            (
                data,
                message_properties.clone(),
                self.symbol_event_sender.clone(),
            )
        }))
        .map(
            |(symbol_to_edit_request, message_properties, symbol_event_sender)| async move {
                let (sender, receiver) = tokio::sync::oneshot::channel();
                let symbol_event_request = SymbolEventRequest::new(
                    symbol_to_edit_request.symbol_identifier().clone(),
                    SymbolEvent::Edit(symbol_to_edit_request), // defines event type
                    ToolProperties::new(),
                );
                let event = SymbolEventMessage::message_with_properties(
                    symbol_event_request,
                    message_properties,
                    sender,
                );
                let _ = symbol_event_sender.send(event);
                receiver.await
            },
        )
        // run 100 edit requests in parallel to prevent race conditions
        .buffer_unordered(100)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s.ok())
        .collect::<Vec<_>>();

        let cloned_user_query = anchor_request.user_query().to_owned();
        // the editor state has changed, so we need to react to that now
        let _ = self
            .reaction_sender
            .send(EnvironmentEventType::EditorStateChange(
                EditorStateChangeRequest::new(edits_done, cloned_user_query),
            ));
        // we are not focussed anymore, we can go about receiving events as usual
        {
            let mut focussed = self.focussing.lock().await;
            *focussed = false;
        }
        println!(
            "scratch_pad_agent::human_message_anchor::end::time_taken({}ms)",
            start_instant.elapsed().as_millis()
        );
        // send end of iteration event over here to the frontend
        let _ = message_properties
            .ui_sender()
            .send(UIEventWithID::code_iteration_finished(
                message_properties.request_id_str().to_owned(),
            ));
        Ok(())
    }

    async fn _handle_user_anchor_request(
        &self,
        anchor_request: HumanAnchorRequest,
        message_properties: SymbolEventMessageProperties,
    ) {
        println!("scratch_pad::handle_user_anchor_request");
        // we are busy with the edits going on, so we can discard lsp signals for a while
        // figure out what to do over here
        let file_paths = anchor_request
            .anchored_symbols()
            .into_iter()
            .filter_map(|anchor_symbol| anchor_symbol.fs_file_path())
            .collect::<Vec<_>>();
        let mut already_seen_files: HashSet<String> = Default::default();
        let mut user_context_files = vec![];
        for fs_file_path in file_paths.into_iter() {
            if already_seen_files.contains(&fs_file_path) {
                continue;
            }
            already_seen_files.insert(fs_file_path.to_owned());
            let file_contents = self
                .tool_box
                .file_open(fs_file_path, message_properties.clone())
                .await;
            if let Ok(file_contents) = file_contents {
                user_context_files.push({
                    let file_path = file_contents.fs_file_path();
                    let language = file_contents.language();
                    let content = file_contents.contents_ref();
                    ScratchPadFilesActive::_new(
                        format!(
                            r#"<file>
<fs_file_path>
{file_path}
</fs_file_path>
<content>
```{language}
{content}
```
</content>
</file>"#
                        ),
                        file_path.to_owned(),
                    )
                });
            }
        }
        // update our cache over here
        {
            let mut files_context = self._files_context.lock().await;
            *files_context = user_context_files.to_vec();
        }
        let file_paths_interested = user_context_files
            .iter()
            .map(|context_file| context_file._file_path.to_owned())
            .collect::<Vec<_>>();
        let user_context_files = user_context_files
            .into_iter()
            .map(|context_file| context_file._file_content)
            .collect::<Vec<_>>();
        println!("scratch_pad_agent::tool_box::agent_human_request");
        let _ = self
            .tool_box
            .scratch_pad_agent_human_request(
                self._storage_fs_path.to_owned(),
                anchor_request.user_query().to_owned(),
                user_context_files,
                file_paths_interested,
                anchor_request
                    .anchored_symbols()
                    .into_iter()
                    .map(|anchor_symbol| {
                        let content = anchor_symbol.content();
                        let fs_file_path = anchor_symbol.fs_file_path().unwrap_or_default();
                        let line_range_header = format!(
                            "{}-{}:{}",
                            fs_file_path,
                            anchor_symbol.possible_range().start_line(),
                            anchor_symbol.possible_range().end_line()
                        );
                        format!(
                            r#"Location: {line_range_header}
```
{content}
```"#
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                message_properties.clone(),
            )
            .await;
    }

    /// We want to react to the various edits which have happened and the request they were linked to
    /// and come up with next steps and try to understand what we can do to help the developer
    async fn _react_to_edits(
        &self,
        editor_state_change: EditorStateChangeRequest,
        message_properties: SymbolEventMessageProperties,
    ) {
        println!("scratch_pad::react_to_edits");
        // figure out what to do over here
        let user_context_files;
        {
            let files_context = self._files_context.lock().await;
            user_context_files = (*files_context).to_vec();
        }
        let file_paths_in_focus = user_context_files
            .iter()
            .map(|context_file| context_file._file_path.to_owned())
            .collect::<Vec<String>>();
        let user_context_files = user_context_files
            .into_iter()
            .map(|context_file| context_file._file_content)
            .collect::<Vec<_>>();
        let user_query = editor_state_change.user_query().to_owned();
        let edits_made = editor_state_change.consume_edits_made();
        let extra_context;
        {
            extra_context = (*self.extra_context.lock().await).to_owned();
        }
        {
            let mut extra_context = self.extra_context.lock().await;
            *extra_context = (*extra_context).to_owned()
                + "\n"
                + &edits_made
                    .iter()
                    .map(|edit| edit.clone().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
        }
        let _ = self
            .tool_box
            .scratch_pad_edits_made(
                &self._storage_fs_path,
                &user_query,
                &extra_context,
                file_paths_in_focus,
                edits_made
                    .into_iter()
                    .map(|edit| edit.to_string())
                    .collect::<Vec<_>>(),
                user_context_files,
                message_properties.clone(),
            )
            .await;

        // Now we want to grab the diagnostics which come in naturally
        // or via the files we are observing, there are race conditions here which
        // we want to tackle for sure
        // check for diagnostic_symbols
        // let cloned_self = self.clone();
        // let _ = tokio::spawn(async move {
        //     // sleep for 2 seconds before getting the signals
        //     let _ = tokio::time::sleep(Duration::from_secs(2)).await;
        //     cloned_self.grab_diagnostics().await;
        // });
    }

    /// We get to react to the lsp signal over here
    async fn _react_to_lsp_signal(
        &self,
        lsp_signal: LSPSignal,
        message_properties: SymbolEventMessageProperties,
    ) {
        let focussed;
        {
            focussed = *(self.focussing.lock().await);
        }
        if focussed {
            return;
        }
        match lsp_signal {
            LSPSignal::Diagnostics(diagnostics) => {
                self._react_to_diagnostics(diagnostics, message_properties)
                    .await;
            }
        }
    }

    async fn _react_to_diagnostics(
        &self,
        diagnostics: Vec<LSPDiagnosticError>,
        message_properties: SymbolEventMessageProperties,
    ) {
        // we are busy fixing 🧘‍♂️
        {
            let mut fixing = self.fixing.lock().await;
            *fixing = true;
        }
        let file_paths_focussed;
        {
            file_paths_focussed = self
                ._files_context
                .lock()
                .await
                .iter()
                .map(|file_content| file_content._file_path.to_owned())
                .collect::<HashSet<String>>();
        }
        let diagnostic_messages = diagnostics
            .into_iter()
            .filter(|diagnostic| file_paths_focussed.contains(diagnostic.fs_file_path()))
            .map(|diagnostic| {
                let diagnostic_file_path = diagnostic.fs_file_path();
                let diagnostic_message = diagnostic.diagnostic_message();
                let diagnostic_snippet = diagnostic.snippet();
                format!(
                    r#"<fs_file_path>
{diagnostic_file_path}
</fs_file_path>
<message>
{diagnostic_message}
</message>
<snippet_with_error>
{diagnostic_snippet}
</snippet_with_error>"#
                )
            })
            .collect::<Vec<_>>();
        if diagnostic_messages.is_empty() {
            return;
        }
        println!("scratch_pad::reacting_to_diagnostics");
        let files_context;
        {
            files_context = (*self._files_context.lock().await).to_vec();
        }
        let extra_context;
        {
            extra_context = (*self.extra_context.lock().await).to_owned();
        }
        let interested_file_paths = files_context
            .iter()
            .map(|file_context| file_context._file_path.to_owned())
            .collect::<Vec<_>>();
        let _ = self
            .tool_box
            .scratch_pad_diagnostics(
                &self._storage_fs_path,
                diagnostic_messages,
                interested_file_paths,
                files_context
                    .into_iter()
                    .map(|files_context| files_context._file_content)
                    .collect::<Vec<_>>(),
                extra_context,
                message_properties.clone(),
            )
            .await;

        // we try to make code edits to fix the diagnostics
        let _ = self._code_edit_for_diagnostics(message_properties).await;

        // we are done fixing so start skipping
        {
            let mut fixing = self.fixing.lock().await;
            *fixing = false;
        }
    }

    // Now that we have reacted to the update on the scratch-pad we can start
    // thinking about making code edits for this
    async fn _code_edit_for_diagnostics(&self, message_properties: SymbolEventMessageProperties) {
        // we want to give the scratch-pad as input to the agent and the files
        // which are visible as the context where it can make the edits
        // we can be a bit smarter and make the eidts over the file one after
        // the other
        // what about the cache hits over here? thats one of the major issues
        // on how we want to tack it
        // fuck the cache hit just raw dog the edits in parallel on the files
        // which we are tracking using the scratch-pad and the files
        let scratch_pad_content = self
            .tool_box
            .file_open(self._storage_fs_path.to_owned(), message_properties.clone())
            .await;
        if let Err(e) = scratch_pad_content.as_ref() {
            println!("scratch_pad_agnet::scratch_pad_reading::error");
            eprintln!("{:?}", e);
        }
        let scratch_pad_content = scratch_pad_content.expect("if let Err to hold");
        let active_file_paths;
        {
            active_file_paths = self
                ._files_context
                .lock()
                .await
                .iter()
                .map(|file_context| file_context._file_path.to_owned())
                .collect::<Vec<_>>();
        }
        // we should optimse for cache hit over here somehow
        let mut files_context = vec![];
        for active_file in active_file_paths.to_vec().into_iter() {
            let file_contents = self
                .tool_box
                .file_open(active_file, message_properties.clone())
                .await;
            if let Ok(file_contents) = file_contents {
                let fs_file_path = file_contents.fs_file_path();
                let language_id = file_contents.language();
                let contents = file_contents.contents_ref();
                files_context.push(format!(
                    r#"# FILEPATH: {fs_file_path}
```{language_id}
{contents}
```"#
                ));
            }
        }
        let scratch_pad_contents_ref = scratch_pad_content.contents_ref();
        let mut edits_made = vec![];
        for active_file in active_file_paths.iter() {
            let symbol_identifier = SymbolIdentifier::with_file_path(&active_file, &active_file);
            let user_instruction = format!(
                r#"I am sharing with you the scratchpad where I am keeping track of all the things I am working on. I want you to make edits which help move the tasks forward.
It's important to remember that some edits might require additional steps before we can go about doing them, so feel free to ignore them.
Only make the edits to be the best of your ability in {active_file}

My scratchpad looks like this:
{scratch_pad_contents_ref}

Please help me out by making the necessary code edits"#
            );
            let symbol_event_request = SymbolEventRequest::simple_edit_request(
                symbol_identifier,
                SymbolToEdit::new(
                    active_file.to_owned(),
                    Range::new(Position::new(0, 0, 0), Position::new(10000, 0, 0)),
                    active_file.to_owned(),
                    vec![user_instruction.to_owned()],
                    false,
                    false,
                    true,
                    user_instruction,
                    None,
                    false,
                    Some(files_context.to_vec().join("\n")),
                    true,
                    None,
                    vec![],
                    None,
                ),
                ToolProperties::new(),
            );
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let symbol_event_message = SymbolEventMessage::message_with_properties(
                symbol_event_request,
                message_properties.clone(),
                sender,
            );
            let _ = self.symbol_event_sender.send(symbol_event_message);
            // we are going to react to this automagically since the environment
            // will give us feedback about this (copium)
            let output = receiver.await;
            edits_made.push(output);
        }

        let edits_made = edits_made
            .into_iter()
            .filter_map(|edit| edit.ok())
            .collect::<Vec<_>>();

        // Now we can send these edits to the scratchpad to have a look
        let _ = self
            .reaction_sender
            .send(EnvironmentEventType::EditorStateChange(
                EditorStateChangeRequest::new(
                    edits_made,
                    "I fixed some diagnostic erorrs".to_owned(),
                ),
            ));
    }

    async fn _grab_diagnostics(&self, message_properties: SymbolEventMessageProperties) {
        let files_focussed;
        {
            files_focussed = self
                ._files_context
                .lock()
                .await
                .iter()
                .map(|file| file._file_path.to_owned())
                .collect::<Vec<_>>();
        }
        let diagnostics = self
            .tool_box
            .get_lsp_diagnostics_for_files(files_focussed, message_properties.clone(), false) // with enrichment
            .await
            .unwrap_or_default();
        let _ = self
            .reaction_sender
            .send(EnvironmentEventType::LSP(LSPSignal::Diagnostics(
                diagnostics,
            )));
    }

    async fn is_fixing(&self) -> bool {
        let fixing;
        {
            fixing = *(self.fixing.lock().await);
        }
        fixing
    }

    async fn is_focussing(&self) -> bool {
        let focussing;
        {
            focussing = *(self.focussing.lock().await);
        }
        focussing
    }
}
