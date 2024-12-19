//! We are going to log the UI events, this is mostly for
//! debugging and having better visibility to what ever is happening
//! in the symbols

use std::collections::HashMap;

use crate::{
    agentic::tool::{
        code_symbol::models::anthropic::StepListItem, input::ToolInputPartial, r#type::ToolType,
        ref_filter::ref_filter::Location, search::iterative::IterativeSearchEvent,
        session::tool_use_agent::ToolParameters,
    },
    chunking::text_document::Range,
    user_context::types::VariableInformation,
};

use super::{
    identifier::SymbolIdentifier,
    types::{SymbolEventRequest, SymbolLocation},
};

#[derive(Debug, serde::Serialize)]
pub struct UIEventWithID {
    request_id: String,
    exchange_id: String,
    event: UIEvent,
}

impl UIEventWithID {
    pub fn code_iteration_finished(request_id: String) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::FrameworkEvent(FrameworkEvent::CodeIterationFinished(request_id)),
        }
    }

    pub fn start_long_context_search(request_id: String) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::FrameworkEvent(FrameworkEvent::LongContextSearchStart(request_id)),
        }
    }

    pub fn finish_long_context_search(request_id: String) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::FrameworkEvent(FrameworkEvent::LongContextSearchFinished(request_id)),
        }
    }

    pub fn finish_edit_request(request_id: String) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::EditRequestFinished(request_id),
        }
    }

    /// Repo map search start
    pub fn repo_map_gen_start(request_id: String) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::FrameworkEvent(FrameworkEvent::RepoMapGenerationStart(request_id)),
        }
    }

    /// Repo map generation end
    pub fn repo_map_gen_end(request_id: String) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::FrameworkEvent(FrameworkEvent::RepoMapGenerationFinished(request_id)),
        }
    }

    pub fn from_symbol_event(request_id: String, input: SymbolEventRequest) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::SymbolEvent(input),
        }
    }

    pub fn symbol_location(request_id: String, symbol_location: SymbolLocation) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::SymbolLoctationUpdate(symbol_location),
        }
    }

    pub fn sub_symbol_step(
        request_id: String,
        sub_symbol_request: SymbolEventSubStepRequest,
    ) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::SymbolEventSubStep(sub_symbol_request),
        }
    }

    pub fn probe_answer_event(
        request_id: String,
        symbol_identifier: SymbolIdentifier,
        probe_answer: String,
    ) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::SymbolEventSubStep(SymbolEventSubStepRequest::new(
                symbol_identifier,
                SymbolEventSubStep::Probe(SymbolEventProbeRequest::ProbeAnswer(probe_answer)),
            )),
        }
    }

    pub fn probing_started_event(request_id: String) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::RequestEvent(RequestEvents::ProbingStart),
        }
    }

    pub fn probing_finished_event(request_id: String, response: String) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::RequestEvent(RequestEvents::ProbeFinished(
                RequestEventProbeFinished::new(response),
            )),
        }
    }

    pub fn range_selection_for_edit(
        request_id: String,
        symbol_identifier: SymbolIdentifier,
        range: Range,
        fs_file_path: String,
    ) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::SymbolEventSubStep(
                SymbolEventSubStepRequest::range_selection_for_edit(
                    symbol_identifier,
                    fs_file_path,
                    range,
                ),
            ),
        }
    }

    pub fn edited_code(
        request_id: String,
        symbol_identifier: SymbolIdentifier,
        range: Range,
        fs_file_path: String,
        edited_code: String,
    ) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::SymbolEventSubStep(SymbolEventSubStepRequest::edited_code(
                symbol_identifier,
                range,
                fs_file_path,
                edited_code,
            )),
        }
    }

    pub fn code_correctness_action(
        request_id: String,
        symbol_identifier: SymbolIdentifier,
        range: Range,
        fs_file_path: String,
        tool_use_thinking: String,
    ) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id,
            event: UIEvent::SymbolEventSubStep(SymbolEventSubStepRequest::code_correctness_action(
                symbol_identifier,
                range,
                fs_file_path,
                tool_use_thinking,
            )),
        }
    }

    /// Sends the initial search event to the editor
    pub fn initial_search_symbol_event(
        request_id: String,
        symbols: Vec<InitialSearchSymbolInformation>,
    ) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::FrameworkEvent(FrameworkEvent::InitialSearchSymbols(
                InitialSearchSymbolEvent::new(request_id, symbols),
            )),
        }
    }

    /// sends a open file request
    pub fn open_file_event(request_id: String, exchange_id: String, fs_file_path: String) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: exchange_id,
            event: UIEvent::FrameworkEvent(FrameworkEvent::OpenFile(OpenFileRequest {
                fs_file_path,
                request_id,
            })),
        }
    }

    // start the edit streaming
    pub fn start_edit_streaming(
        request_id: String,
        symbol_identifier: SymbolIdentifier,
        edit_request_id: String,
        range: Range,
        fs_file_path: String,
        session_id: String,
        exchange_id: String,
        plan_step_id: Option<String>,
    ) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::SymbolEventSubStep(
                SymbolEventSubStepRequest::edited_code_stream_start(
                    symbol_identifier,
                    edit_request_id,
                    range,
                    fs_file_path,
                    session_id,
                    exchange_id,
                    plan_step_id,
                ),
            ),
        }
    }

    // end the edit streaming
    pub fn end_edit_streaming(
        request_id: String,
        symbol_identifier: SymbolIdentifier,
        edit_request_id: String,
        range: Range,
        fs_file_path: String,
        session_id: String,
        exchange_id: String,
        plan_step_id: Option<String>,
    ) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::SymbolEventSubStep(SymbolEventSubStepRequest::edited_code_stream_end(
                symbol_identifier,
                edit_request_id,
                range,
                fs_file_path,
                session_id,
                exchange_id,
                plan_step_id,
            )),
        }
    }

    // send delta from the edit stream
    pub fn delta_edit_streaming(
        request_id: String,
        symbol_identifier: SymbolIdentifier,
        delta: String,
        edit_request_id: String,
        range: Range,
        fs_file_path: String,
        session_id: String,
        exchange_id: String,
        plan_step_id: Option<String>,
    ) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::SymbolEventSubStep(
                SymbolEventSubStepRequest::edited_code_stream_delta(
                    symbol_identifier,
                    edit_request_id,
                    range,
                    fs_file_path,
                    delta,
                    session_id,
                    exchange_id,
                    plan_step_id,
                ),
            ),
        }
    }

    pub fn send_thinking_for_edit(
        request_id: String,
        symbol_identifier: SymbolIdentifier,
        thinking: String,
        delta: Option<String>,
        edit_request_id: String,
        exchange_id: String,
    ) -> Self {
        Self {
            request_id: request_id,
            exchange_id: exchange_id,
            event: UIEvent::SymbolEventSubStep(SymbolEventSubStepRequest::thinking_for_edit(
                symbol_identifier,
                thinking,
                delta,
                edit_request_id,
            )),
        }
    }

    pub fn found_reference(request_id: String, references: FoundReference) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::FrameworkEvent(FrameworkEvent::ReferenceFound(references)),
        }
    }

    pub fn relevant_reference(
        request_id: String,
        fs_file_path: &str,
        symbol_name: &str,
        thinking: &str,
    ) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::FrameworkEvent(FrameworkEvent::RelevantReference(
                RelevantReference::new(&fs_file_path, &symbol_name, &thinking),
            )),
        }
    }

    pub fn grouped_by_reason_references(request_id: String, references: GroupedReferences) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::FrameworkEvent(FrameworkEvent::GroupedReferences(references)),
        }
    }

    pub fn search_iteration_event(request_id: String, event: IterativeSearchEvent) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: request_id.to_owned(),
            event: UIEvent::FrameworkEvent(FrameworkEvent::SearchIteration(event)),
        }
    }

    pub fn agentic_top_level_thinking(
        request_id: String,
        exchange_id: String,
        thinking: &str,
    ) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id,
            event: UIEvent::FrameworkEvent(FrameworkEvent::AgenticTopLevelThinking(
                thinking.to_owned(),
            )),
        }
    }

    pub fn agentic_symbol_level_thinking(
        request_id: String,
        exchange_id: String,
        event: StepListItem,
    ) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id,
            event: UIEvent::FrameworkEvent(FrameworkEvent::AgenticSymbolLevelThinking(event)),
        }
    }

    /// Sends over a chat event to the frontend
    pub fn chat_event(
        request_id: String,
        exchange_id: String,
        answer_up_until_now: String,
        delta: Option<String>,
    ) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: exchange_id.to_owned(),
            event: UIEvent::ChatEvent(ChatMessageEvent::new(
                answer_up_until_now,
                delta,
                exchange_id,
            )),
        }
    }

    /// Sends over the variables we are using for this intent
    pub fn send_variables(
        request_id: String,
        exchange_id: String,
        variables: Vec<VariableInformation>,
    ) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: exchange_id.to_owned(),
            event: UIEvent::FrameworkEvent(FrameworkEvent::ReferencesUsed(
                FrameworkReferencesUsed::new(exchange_id, variables),
            )),
        }
    }

    /// Finished exchange
    pub fn finished_exchange(request_id: String, exchange_id: String) -> Self {
        Self {
            request_id: request_id.to_owned(),
            exchange_id: exchange_id.to_owned(),
            event: UIEvent::ExchangeEvent(ExchangeMessageEvent::FinishedExchange(
                FinishedExchangeEvent::new(exchange_id, request_id),
            )),
        }
    }

    pub fn plan_description_updated(
        session_id: String,
        exchange_id: String,
        index: usize,
        delta: Option<String>,
        description_up_until_now: String,
        files_to_edit: Vec<String>,
    ) -> Self {
        Self {
            request_id: session_id.to_owned(),
            exchange_id: exchange_id.to_owned(),
            event: UIEvent::PlanEvent(PlanMessageEvent::PlanStepDescriptionUpdate(
                PlanStepDescriptionUpdateEvent {
                    session_id,
                    exchange_id,
                    files_to_edit,
                    delta,
                    description_up_until_now,
                    index,
                },
            )),
        }
    }

    pub fn plan_title_added(
        session_id: String,
        exchange_id: String,
        index: usize,
        files_to_edit: Vec<String>,
        title: String,
    ) -> Self {
        Self {
            request_id: session_id.to_owned(),
            exchange_id: exchange_id.to_owned(),
            event: UIEvent::PlanEvent(PlanMessageEvent::PlanStepTitleAdded(PlanStepTitleEvent {
                session_id,
                exchange_id,
                files_to_edit,
                title,
                index,
            })),
        }
    }

    pub fn plan_complete_added(
        session_id: String,
        exchange_id: String,
        index: usize,
        files_to_edit: Vec<String>,
        title: String,
        description: String,
    ) -> Self {
        Self {
            request_id: session_id.to_owned(),
            exchange_id: exchange_id.to_owned(),
            event: UIEvent::PlanEvent(PlanMessageEvent::PlanStepCompleteAdded(PlanStepAddEvent {
                session_id,
                exchange_id,
                files_to_edit,
                title,
                description,
                index,
            })),
        }
    }

    pub fn inference_started(session_id: String, exchange_id: String) -> Self {
        Self {
            request_id: session_id,
            exchange_id,
            event: UIEvent::ExchangeEvent(ExchangeMessageEvent::ExecutionState(
                ExecutionExchangeStateEvent::Inference,
            )),
        }
    }

    pub fn request_review(session_id: String, exchange_id: String) -> Self {
        Self {
            request_id: session_id,
            exchange_id,
            event: UIEvent::ExchangeEvent(ExchangeMessageEvent::ExecutionState(
                ExecutionExchangeStateEvent::InReview,
            )),
        }
    }

    pub fn request_cancelled(session_id: String, exchange_id: String) -> Self {
        Self {
            request_id: session_id,
            exchange_id,
            event: UIEvent::ExchangeEvent(ExchangeMessageEvent::ExecutionState(
                ExecutionExchangeStateEvent::Cancelled,
            )),
        }
    }

    pub fn edits_started_in_exchange(
        session_id: String,
        exchange_id: String,
        files: Vec<String>,
    ) -> Self {
        Self {
            request_id: session_id,
            exchange_id,
            event: UIEvent::ExchangeEvent(ExchangeMessageEvent::EditsExchangeState(
                EditsExchangeStateEvent {
                    edits_state: EditsStateEvent::Loading,
                    files,
                },
            )),
        }
    }

    pub fn edits_cancelled_in_exchange(session_id: String, exchange_id: String) -> Self {
        Self {
            request_id: session_id,
            exchange_id,
            event: UIEvent::ExchangeEvent(ExchangeMessageEvent::EditsExchangeState(
                EditsExchangeStateEvent {
                    edits_state: EditsStateEvent::Cancelled,
                    files: vec![],
                },
            )),
        }
    }

    pub fn edits_marked_complete(session_id: String, exchange_id: String) -> Self {
        Self {
            request_id: session_id,
            exchange_id,
            event: UIEvent::ExchangeEvent(ExchangeMessageEvent::EditsExchangeState(
                EditsExchangeStateEvent {
                    edits_state: EditsStateEvent::MarkedComplete,
                    files: vec![],
                },
            )),
        }
    }

    pub fn edits_accepted(session_id: String, exchange_id: String) -> Self {
        Self {
            request_id: session_id,
            exchange_id,
            event: UIEvent::ExchangeEvent(ExchangeMessageEvent::EditsExchangeState(
                EditsExchangeStateEvent {
                    edits_state: EditsStateEvent::Accepted,
                    files: vec![],
                },
            )),
        }
    }

    pub fn start_plan_generation(session_id: String, exchange_id: String) -> Self {
        Self {
            request_id: session_id,
            exchange_id,
            event: UIEvent::ExchangeEvent(ExchangeMessageEvent::PlansExchangeState(
                EditsExchangeStateEvent {
                    edits_state: EditsStateEvent::Loading,
                    files: vec![],
                },
            )),
        }
    }

    pub fn plan_as_finished(session_id: String, exchange_id: String) -> Self {
        Self {
            request_id: session_id,
            exchange_id,
            event: UIEvent::ExchangeEvent(ExchangeMessageEvent::PlansExchangeState(
                EditsExchangeStateEvent {
                    edits_state: EditsStateEvent::MarkedComplete,
                    files: vec![],
                },
            )),
        }
    }

    pub fn plan_as_accepted(session_id: String, exchange_id: String) -> Self {
        Self {
            request_id: session_id,
            exchange_id,
            event: UIEvent::ExchangeEvent(ExchangeMessageEvent::PlansExchangeState(
                EditsExchangeStateEvent {
                    edits_state: EditsStateEvent::Accepted,
                    files: vec![],
                },
            )),
        }
    }

    pub fn plan_as_cancelled(session_id: String, exchange_id: String) -> Self {
        Self {
            request_id: session_id,
            exchange_id,
            event: UIEvent::ExchangeEvent(ExchangeMessageEvent::PlansExchangeState(
                EditsExchangeStateEvent {
                    edits_state: EditsStateEvent::Cancelled,
                    files: vec![],
                },
            )),
        }
    }

    pub fn plan_regeneration(session_id: String, exchange_id: String) -> Self {
        Self {
            request_id: session_id.to_owned(),
            exchange_id: exchange_id.to_owned(),
            event: UIEvent::ExchangeEvent(ExchangeMessageEvent::RegeneratePlan(
                RegeneratePlanExchangeEvent::new(exchange_id, session_id),
            )),
        }
    }

    pub fn terminal_command(session_id: String, exchange_id: String, command: String) -> Self {
        Self {
            request_id: session_id.to_owned(),
            exchange_id: exchange_id.to_owned(),
            event: UIEvent::ExchangeEvent(ExchangeMessageEvent::TerminalCommand(
                TerminalCommandEvent::new(session_id, exchange_id, command),
            )),
        }
    }

    pub fn tool_use_detected(
        session_id: String,
        exchange_id: String,
        tool_use_partial_input: ToolInputPartial,
        thinking: String,
    ) -> Self {
        Self {
            request_id: session_id.to_owned(),
            exchange_id: exchange_id.to_owned(),
            event: UIEvent::FrameworkEvent(FrameworkEvent::ToolUseDetected(ToolUseDetectedEvent {
                tool_use_partial_input,
                thinking,
            })),
        }
    }

    /// Sends over the tool thinking to the external world
    pub fn tool_thinking(session_id: String, exchange_id: String, tool_thinking: String) -> Self {
        Self {
            request_id: session_id.to_owned(),
            exchange_id,
            event: UIEvent::FrameworkEvent(FrameworkEvent::ToolThinking(ToolThinkingEvent {
                thinking: tool_thinking,
            })),
        }
    }

    pub fn tool_not_found(session_id: String, exchange_id: String, full_output: String) -> Self {
        Self {
            request_id: session_id.to_owned(),
            exchange_id,
            event: UIEvent::FrameworkEvent(FrameworkEvent::ToolNotFound(ToolNotFoundEvent {
                full_output,
            })),
        }
    }

    pub fn tool_found(session_id: String, exchange_id: String, tool_type: ToolType) -> Self {
        Self {
            request_id: session_id.to_owned(),
            exchange_id,
            event: UIEvent::FrameworkEvent(FrameworkEvent::ToolTypeFound(ToolTypeFoundEvent {
                tool_type,
            })),
        }
    }

    pub fn tool_parameter_found(
        session_id: String,
        exchange_id: String,
        tool_parameter_input: ToolParameters,
    ) -> Self {
        Self {
            request_id: session_id.to_owned(),
            exchange_id,
            event: UIEvent::FrameworkEvent(FrameworkEvent::ToolParameterFound(
                ToolParameterFoundEvent {
                    tool_parameter_input,
                },
            )),
        }
    }

    pub fn tool_output_delta_response(
        session_id: String,
        exchange_id: String,
        delta: String,
        answer_up_until_now: String,
    ) -> Self {
        Self {
            request_id: session_id.to_owned(),
            exchange_id,
            event: UIEvent::FrameworkEvent(FrameworkEvent::ToolOutput(
                ToolOutputEvent::ToolOutputResponse(ToolOutputResponseEvent {
                    answer_up_until_now,
                    delta,
                }),
            )),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub enum UIEvent {
    SymbolEvent(SymbolEventRequest),
    SymbolLoctationUpdate(SymbolLocation),
    SymbolEventSubStep(SymbolEventSubStepRequest),
    RequestEvent(RequestEvents),
    EditRequestFinished(String),
    FrameworkEvent(FrameworkEvent),
    ChatEvent(ChatMessageEvent),
    ExchangeEvent(ExchangeMessageEvent),
    PlanEvent(PlanMessageEvent),
}

impl From<SymbolEventRequest> for UIEvent {
    fn from(req: SymbolEventRequest) -> Self {
        UIEvent::SymbolEvent(req)
    }
}

#[derive(Debug, serde::Serialize)]
pub enum SymbolEventProbeRequest {
    SubSymbolSelection,
    ProbeDeeperSymbol,
    /// The final answer for the probe is sent via this event
    ProbeAnswer(String),
}

#[derive(Debug, serde::Serialize)]
pub struct SymbolEventGoToDefinitionRequest {
    fs_file_path: String,
    range: Range,
    thinking: String,
}

impl SymbolEventGoToDefinitionRequest {
    fn new(fs_file_path: String, range: Range, thinking: String) -> Self {
        Self {
            fs_file_path,
            range,
            thinking,
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct RangeSelectionForEditRequest {
    range: Range,
    fs_file_path: String,
    // user_id: LSPQuickFixInvocationRequest,
}

impl RangeSelectionForEditRequest {
    pub fn new(range: Range, fs_file_path: String) -> Self {
        Self {
            range,
            fs_file_path,
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct InsertCodeForEditRequest {
    range: Range,
    fs_file_path: String,
}

#[derive(Debug, serde::Serialize)]
pub struct EditedCodeForEditRequest {
    range: Range,
    fs_file_path: String,
    new_code: String,
}

impl EditedCodeForEditRequest {
    pub fn new(range: Range, fs_file_path: String, new_code: String) -> Self {
        Self {
            range,
            fs_file_path,
            new_code,
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct CodeCorrectionToolSelection {
    range: Range,
    fs_file_path: String,
    tool_use_thinking: String,
}

impl CodeCorrectionToolSelection {
    pub fn new(range: Range, fs_file_path: String, tool_use_thinking: String) -> Self {
        Self {
            range,
            fs_file_path,
            tool_use_thinking,
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub enum EditedCodeStreamingEvent {
    Start,
    Delta(String),
    End,
}

#[derive(Debug, serde::Serialize)]
pub struct EditedCodeStreamingRequest {
    edit_request_id: String,
    // This is the id of the session the edit is part of
    session_id: String,
    range: Range,
    fs_file_path: String,
    updated_code: Option<String>,
    event: EditedCodeStreamingEvent,
    apply_directly: bool,
    // The exchange id this edit is part of
    exchange_id: String,
    plan_step_id: Option<String>,
}

impl EditedCodeStreamingRequest {
    pub fn start_edit(
        edit_request_id: String,
        session_id: String,
        range: Range,
        fs_file_path: String,
        exchange_id: String,
        plan_step_id: Option<String>,
    ) -> Self {
        Self {
            edit_request_id,
            session_id,
            range,
            fs_file_path,
            updated_code: None,
            event: EditedCodeStreamingEvent::Start,
            apply_directly: false,
            exchange_id,
            plan_step_id,
        }
    }

    pub fn delta(
        edit_request_id: String,
        session_id: String,
        range: Range,
        fs_file_path: String,
        delta: String,
        exchange_id: String,
        plan_step_id: Option<String>,
    ) -> Self {
        Self {
            edit_request_id,
            session_id,
            range,
            fs_file_path,
            updated_code: None,
            event: EditedCodeStreamingEvent::Delta(delta),
            apply_directly: false,
            exchange_id,
            plan_step_id,
        }
    }

    pub fn end(
        edit_request_id: String,
        session_id: String,
        range: Range,
        fs_file_path: String,
        exchange_id: String,
        plan_step_id: Option<String>,
    ) -> Self {
        Self {
            edit_request_id,
            session_id,
            range,
            fs_file_path,
            updated_code: None,
            event: EditedCodeStreamingEvent::End,
            apply_directly: false,
            exchange_id,
            plan_step_id,
        }
    }

    pub fn set_apply_directly(mut self) -> Self {
        self.apply_directly = true;
        self
    }
}

/// We have range selection and then the edited code, we should also show the
/// events which the AI is using for the tool correction and whats it is planning
/// on doing for that
#[derive(Debug, serde::Serialize)]
pub enum SymbolEventEditRequest {
    RangeSelectionForEdit(RangeSelectionForEditRequest),
    /// We might be inserting code at a line which is a new symbol by itself
    InsertCode(InsertCodeForEditRequest),
    EditCode(EditedCodeForEditRequest),
    CodeCorrectionTool(CodeCorrectionToolSelection),
    EditCodeStreaming(EditedCodeStreamingRequest),
    ThinkingForEdit(ThinkingForEditRequest),
}

#[derive(Debug, serde::Serialize)]
pub struct ThinkingForEditRequest {
    edit_request_id: String,
    thinking: String,
    delta: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub enum SymbolEventSubStep {
    Probe(SymbolEventProbeRequest),
    GoToDefinition(SymbolEventGoToDefinitionRequest),
    Edit(SymbolEventEditRequest),
}

#[derive(Debug, serde::Serialize)]
pub struct SymbolEventSubStepRequest {
    symbol_identifier: SymbolIdentifier,
    event: SymbolEventSubStep,
}

impl SymbolEventSubStepRequest {
    pub fn new(symbol_identifier: SymbolIdentifier, event: SymbolEventSubStep) -> Self {
        Self {
            symbol_identifier,
            event,
        }
    }

    pub fn probe_answer(symbol_identifier: SymbolIdentifier, answer: String) -> Self {
        Self {
            symbol_identifier,
            event: SymbolEventSubStep::Probe(SymbolEventProbeRequest::ProbeAnswer(answer)),
        }
    }

    pub fn go_to_definition_request(
        symbol_identifier: SymbolIdentifier,
        fs_file_path: String,
        range: Range,
        thinking: String,
    ) -> Self {
        Self {
            symbol_identifier,
            event: SymbolEventSubStep::GoToDefinition(SymbolEventGoToDefinitionRequest::new(
                fs_file_path,
                range,
                thinking,
            )),
        }
    }

    pub fn range_selection_for_edit(
        symbol_identifier: SymbolIdentifier,
        fs_file_path: String,
        range: Range,
    ) -> Self {
        Self {
            symbol_identifier,
            event: SymbolEventSubStep::Edit(SymbolEventEditRequest::RangeSelectionForEdit(
                RangeSelectionForEditRequest::new(range, fs_file_path),
            )),
        }
    }

    pub fn edited_code(
        symbol_identifier: SymbolIdentifier,
        range: Range,
        fs_file_path: String,
        edited_code: String,
    ) -> Self {
        Self {
            symbol_identifier,
            event: SymbolEventSubStep::Edit(SymbolEventEditRequest::EditCode(
                EditedCodeForEditRequest::new(range, fs_file_path, edited_code),
            )),
        }
    }

    pub fn edited_code_stream_start(
        symbol_identifier: SymbolIdentifier,
        edit_request_id: String,
        range: Range,
        fs_file_path: String,
        session_id: String,
        exchange_id: String,
        plan_step_id: Option<String>,
    ) -> Self {
        Self {
            symbol_identifier,
            event: SymbolEventSubStep::Edit(SymbolEventEditRequest::EditCodeStreaming(
                EditedCodeStreamingRequest {
                    edit_request_id,
                    session_id,
                    range,
                    fs_file_path,
                    event: EditedCodeStreamingEvent::Start,
                    updated_code: None,
                    apply_directly: false,
                    exchange_id,
                    plan_step_id,
                },
            )),
        }
    }

    pub fn edited_code_stream_end(
        symbol_identifier: SymbolIdentifier,
        edit_request_id: String,
        range: Range,
        fs_file_path: String,
        session_id: String,
        exchange_id: String,
        plan_step_id: Option<String>,
    ) -> Self {
        Self {
            symbol_identifier,
            event: SymbolEventSubStep::Edit(SymbolEventEditRequest::EditCodeStreaming(
                EditedCodeStreamingRequest {
                    edit_request_id,
                    session_id,
                    range,
                    fs_file_path,
                    updated_code: None,
                    event: EditedCodeStreamingEvent::End,
                    apply_directly: false,
                    exchange_id,
                    plan_step_id,
                },
            )),
        }
    }

    pub fn thinking_for_edit(
        symbol_identifier: SymbolIdentifier,
        thinking: String,
        delta: Option<String>,
        edit_request_id: String,
    ) -> Self {
        Self {
            symbol_identifier,
            event: SymbolEventSubStep::Edit(SymbolEventEditRequest::ThinkingForEdit(
                ThinkingForEditRequest {
                    edit_request_id,
                    thinking,
                    delta,
                },
            )),
        }
    }

    pub fn edited_code_stream_delta(
        symbol_identifier: SymbolIdentifier,
        edit_request_id: String,
        range: Range,
        fs_file_path: String,
        delta: String,
        session_id: String,
        exchange_id: String,
        plan_step_id: Option<String>,
    ) -> Self {
        Self {
            symbol_identifier,
            event: SymbolEventSubStep::Edit(SymbolEventEditRequest::EditCodeStreaming(
                EditedCodeStreamingRequest {
                    edit_request_id,
                    session_id,
                    range,
                    fs_file_path,
                    event: EditedCodeStreamingEvent::Delta(delta),
                    updated_code: None,
                    apply_directly: false,
                    exchange_id,
                    plan_step_id,
                },
            )),
        }
    }

    pub fn code_correctness_action(
        symbol_identifier: SymbolIdentifier,
        range: Range,
        fs_file_path: String,
        tool_use_thinking: String,
    ) -> Self {
        Self {
            symbol_identifier,
            event: SymbolEventSubStep::Edit(SymbolEventEditRequest::CodeCorrectionTool(
                CodeCorrectionToolSelection::new(range, fs_file_path, tool_use_thinking),
            )),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct RequestEventProbeFinished {
    reply: String,
}

impl RequestEventProbeFinished {
    pub fn new(reply: String) -> Self {
        Self { reply }
    }
}

#[derive(Debug, serde::Serialize)]
pub enum RequestEvents {
    ProbingStart,
    ProbeFinished(RequestEventProbeFinished),
}

#[derive(Debug, serde::Serialize)]
pub struct InitialSearchSymbolInformation {
    symbol_name: String,
    fs_file_path: Option<String>,
    is_new: bool,
    thinking: String,
    // send over the range of this symbol
    range: Option<Range>,
}

impl InitialSearchSymbolInformation {
    pub fn new(
        symbol_name: String,
        fs_file_path: Option<String>,
        is_new: bool,
        thinking: String,
        range: Option<Range>,
    ) -> Self {
        Self {
            symbol_name,
            fs_file_path,
            is_new,
            thinking,
            range,
        }
    }
}

pub type GroupedReferences = HashMap<String, Vec<Location>>;

pub type FoundReference = HashMap<String, usize>; // <file_path, count>

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct RelevantReference {
    fs_file_path: String,
    symbol_name: String,
    reason: String,
}

impl RelevantReference {
    pub fn new(fs_file_path: &str, symbol_name: &str, reason: &str) -> Self {
        Self {
            fs_file_path: fs_file_path.to_string(),
            symbol_name: symbol_name.to_string(),
            reason: reason.to_string(),
        }
    }

    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn symbol_name(&self) -> &str {
        &self.symbol_name
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn to_string(&self) -> String {
        format!(
            "File: {}, Symbol: {}, Reason: {}",
            self.fs_file_path, self.symbol_name, self.reason
        )
    }
}

#[derive(Debug, serde::Serialize)]
pub struct InitialSearchSymbolEvent {
    request_id: String,
    symbols: Vec<InitialSearchSymbolInformation>,
}

impl InitialSearchSymbolEvent {
    pub fn new(request_id: String, symbols: Vec<InitialSearchSymbolInformation>) -> Self {
        Self {
            request_id,
            symbols,
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct OpenFileRequest {
    fs_file_path: String,
    request_id: String,
}

#[derive(Debug, serde::Serialize)]
pub struct FrameworkReferencesUsed {
    exchange_id: String,
    variables: Vec<VariableInformation>,
}

impl FrameworkReferencesUsed {
    pub fn new(exchange_id: String, variables: Vec<VariableInformation>) -> Self {
        Self {
            exchange_id,
            variables,
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub enum FrameworkEvent {
    RepoMapGenerationStart(String),
    RepoMapGenerationFinished(String),
    LongContextSearchStart(String),
    LongContextSearchFinished(String),
    InitialSearchSymbols(InitialSearchSymbolEvent),
    OpenFile(OpenFileRequest),
    CodeIterationFinished(String),
    ReferenceFound(FoundReference),
    RelevantReference(RelevantReference), // this naming sucks ass
    GroupedReferences(GroupedReferences),
    SearchIteration(IterativeSearchEvent),
    AgenticTopLevelThinking(String),
    AgenticSymbolLevelThinking(StepListItem),
    ReferencesUsed(FrameworkReferencesUsed),
    TerminalCommand(TerminalCommandEvent),
    ToolUseDetected(ToolUseDetectedEvent),
    ToolThinking(ToolThinkingEvent),
    ToolNotFound(ToolNotFoundEvent),
    ToolTypeFound(ToolTypeFoundEvent),
    ToolParameterFound(ToolParameterFoundEvent),
    ToolOutput(ToolOutputEvent),
}

#[derive(Debug, serde::Serialize)]
pub enum ToolOutputEvent {
    ToolTypeForOutput(ToolTypeForOutputEvent),
    ToolOutputResponse(ToolOutputResponseEvent),
}

#[derive(Debug, serde::Serialize)]
pub struct ToolTypeForOutputEvent {
    tool_type: ToolType,
}

#[derive(Debug, serde::Serialize)]
pub struct ToolOutputResponseEvent {
    delta: String,
    answer_up_until_now: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ToolParameterFoundEvent {
    tool_parameter_input: ToolParameters,
}

#[derive(Debug, serde::Serialize)]
pub struct ToolTypeFoundEvent {
    tool_type: ToolType,
}

#[derive(Debug, serde::Serialize)]
pub struct ToolNotFoundEvent {
    full_output: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ToolThinkingEvent {
    thinking: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ToolUseDetectedEvent {
    tool_use_partial_input: ToolInputPartial,
    thinking: String,
}

#[derive(Debug, serde::Serialize)]
pub struct TerminalCommandEvent {
    session_id: String,
    exchange_id: String,
    command: String,
}

impl TerminalCommandEvent {
    pub fn new(session_id: String, exchange_id: String, command: String) -> Self {
        Self {
            session_id,
            exchange_id,
            command,
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct ChatMessageEvent {
    answer_up_until_now: String,
    delta: Option<String>,
    exchange_id: String,
}

impl ChatMessageEvent {
    pub fn new(answer_up_until_now: String, delta: Option<String>, exchange_id: String) -> Self {
        Self {
            answer_up_until_now,
            delta,
            exchange_id,
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub enum ExchangeMessageEvent {
    RegeneratePlan(RegeneratePlanExchangeEvent),
    FinishedExchange(FinishedExchangeEvent),
    EditsExchangeState(EditsExchangeStateEvent),
    PlansExchangeState(EditsExchangeStateEvent),
    ExecutionState(ExecutionExchangeStateEvent),
    TerminalCommand(TerminalCommandEvent),
}

#[derive(Debug, serde::Serialize)]
pub enum ExecutionExchangeStateEvent {
    Inference,
    InReview,
    Cancelled,
}

#[derive(Debug, serde::Serialize)]
pub enum EditsStateEvent {
    Loading,
    MarkedComplete,
    Cancelled,
    Accepted,
}

#[derive(Debug, serde::Serialize)]
pub struct EditsExchangeStateEvent {
    edits_state: EditsStateEvent,
    files: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct RegeneratePlanExchangeEvent {
    exchange_id: String,
    session_id: String,
}

impl RegeneratePlanExchangeEvent {
    pub fn new(exchange_id: String, session_id: String) -> Self {
        Self {
            exchange_id,
            session_id,
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct FinishedExchangeEvent {
    exchange_id: String,
    session_id: String,
}

impl FinishedExchangeEvent {
    pub fn new(exchange_id: String, session_id: String) -> Self {
        Self {
            exchange_id,
            session_id,
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub enum PlanMessageEvent {
    PlanStepCompleteAdded(PlanStepAddEvent),
    PlanStepTitleAdded(PlanStepTitleEvent),
    PlanStepDescriptionUpdate(PlanStepDescriptionUpdateEvent),
}

#[derive(Debug, serde::Serialize)]
pub struct PlanStepDescriptionUpdateEvent {
    session_id: String,
    exchange_id: String,
    files_to_edit: Vec<String>,
    delta: Option<String>,
    description_up_until_now: String,
    index: usize,
}

#[derive(Debug, serde::Serialize)]
pub struct PlanStepAddEvent {
    session_id: String,
    exchange_id: String,
    files_to_edit: Vec<String>,
    title: String,
    description: String,
    index: usize,
}

#[derive(Debug, serde::Serialize)]
pub struct PlanStepTitleEvent {
    session_id: String,
    exchange_id: String,
    files_to_edit: Vec<String>,
    title: String,
    index: usize,
}
