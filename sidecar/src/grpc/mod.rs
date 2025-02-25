#![cfg(feature = "grpc")]

use std::sync::Arc;
use futures::StreamExt;
use tonic::{transport::Server, Request, Response, Status};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio::time::Duration;

use crate::application::application::Application;
use crate::agentic::symbol::events::environment_event::{EnvironmentEvent, EnvironmentEventType};
use crate::agentic::symbol::events::input::SymbolEventRequestId;
use crate::agentic::symbol::events::lsp::LSPDiagnosticError;
use crate::agentic::symbol::events::message_event::SymbolEventMessageProperties;
use crate::agentic::symbol::events::ui_event::UIEventWithID;
use crate::agentic::symbol::identifier::LLMProperties;
use crate::agentic::symbol::errors::SymbolError;
use crate::agentic::tool::errors::ToolError;
use crate::agentic::tool::session::session::AideAgentMode;
use crate::agentic::symbol::scratch_pad::ScratchPadAgent;
use crate::webserver::plan::{check_session_storage_path, check_scratch_pad_path, check_plan_storage_path, plan_storage_directory};

use llm_client::clients::types::{LLMType, LLMClientError};
use llm_client::provider::{
    CodeStoryLLMTypes, CodestoryAccessToken, LLMProvider, LLMProviderAPIKeys,
};

mod proto {
    tonic::include_proto!("agent_farm");
}

use proto::agent_farm_service_server::{AgentFarmService, AgentFarmServiceServer};
use proto::*;

fn convert_position(pos: &proto::Position) -> text_document::Position {
    text_document::Position::new(pos.line, pos.character)
}

fn convert_range(range: &proto::Range) -> text_document::Range {
    text_document::Range::new(
        convert_position(range.start.as_ref().unwrap()),
        convert_position(range.end.as_ref().unwrap()),
    )
}

fn convert_precise_context(ctx: proto::PreciseContext) -> crate::user_context::types::PreciseContext {
    crate::user_context::types::PreciseContext {
        symbol: ctx.symbol.map(|s| crate::user_context::types::Symbol {
            fuzzy_name: s.fuzzy_name,
        }),
        hover_text: ctx.hover_text,
        definition_snippet: ctx.definition_snippet.map(|d| crate::user_context::types::DefinitionSnippet {
            context: d.context,
            start_line: d.start_line as usize,
            end_line: d.end_line as usize,
        }),
        fs_file_path: ctx.fs_file_path,
        relative_file_path: ctx.relative_file_path,
        range: ctx.range.map(|r| convert_range(&r)),
    }
}

fn convert_cursor_position(pos: proto::CursorPosition) -> crate::user_context::types::CursorPosition {
    crate::user_context::types::CursorPosition {
        start_position: convert_position(&pos.start_position.unwrap()),
        end_position: convert_position(&pos.end_position.unwrap()),
    }
}

fn convert_view_port(vp: proto::ViewPort) -> crate::user_context::types::ViewPort {
    crate::user_context::types::ViewPort {
        start_position: convert_position(&vp.start_position.unwrap()),
        end_position: convert_position(&vp.end_position.unwrap()),
        relative_path: vp.relative_path,
        fs_file_path: vp.fs_file_path,
        text_on_screen: vp.text_on_screen,
    }
}

fn convert_user_context(ctx: proto::UserContext) -> crate::user_context::types::UserContext {
    crate::user_context::types::UserContext {
        repo_ref: ctx.repo_ref,
        precise_context: ctx.precise_context.into_iter().map(convert_precise_context).collect(),
        cursor_position: ctx.cursor_position.map(convert_cursor_position),
        current_view_port: ctx.current_view_port.map(convert_view_port),
        language: ctx.language,
    }
}

fn convert_repo_ref(repo_ref: proto::RepoRef) -> crate::repo::types::RepoRef {
    crate::repo::types::RepoRef {
        repo_name: repo_ref.repo_name,
        branch: repo_ref.branch,
        commit_hash: repo_ref.commit_hash,
    }
}

fn convert_error_to_status(err: anyhow::Error) -> Status {
    Status::internal(err.to_string())
}

// Add conversion functions for other types...

use proto::agent_farm_service_server::{AgentFarmService, AgentFarmServiceServer};
use proto::*;

pub struct AgentFarmGrpcServer {
    app: Application,
}

type AgentResponseStream = ReceiverStream<Result<AgentResponse, Status>>;

#[tonic::async_trait]
impl agent_farm_service_server::AgentFarmService for AgentFarmGrpcServer {
    type AgentSessionChatStream = ReceiverStream<Result<AgentResponse, Status>>;
    type AgentSessionEditStream = ReceiverStream<Result<AgentEditResponse, Status>>;
    type AgentToolUseStream = ReceiverStream<Result<ToolUseResponse, Status>>;
    type CodeSculptingStream = ReceiverStream<Result<AgentResponse, Status>>;
    type CancelRunningExchangeStream = ReceiverStream<Result<AgentResponse, Status>>;
    type UserFeedbackOnExchangeStream = ReceiverStream<Result<AgentResponse, Status>>;

    async fn agent_session_plan(
        &self,
        request: Request<AgentSessionRequest>,
    ) -> Result<Response<Self::AgentResponseStream>, Status> {
        let req = request.into_inner();
        let (tx, rx) = mpsc::channel(32);
        let app = self.app.clone();

        let llm_provider = req.model_configuration.map_or_else(
            || LLMProperties::new(
                LLMType::ClaudeSonnet,
                LLMProvider::CodeStory(CodeStoryLLMTypes::new()),
                LLMProviderAPIKeys::CodeStory(CodestoryAccessToken::new(req.access_token.clone())),
            ),
            |config| config.llm_properties_for_slow_model().unwrap_or_default(),
        );

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        let message_properties = SymbolEventMessageProperties::new(
            SymbolEventRequestId::new(req.exchange_id.clone(), req.session_id.clone()),
            tx.clone(),
            req.editor_url.clone(),
            cancellation_token.clone(),
            llm_provider,
        );

        let session_storage_path = check_session_storage_path(app.config.clone(), req.session_id.clone()).await;
        let plan_storage_directory = plan_storage_directory(app.config.clone()).await;
        let plan_service = PlanService::new(
            app.tool_box.clone(),
            app.symbol_manager.clone(),
            plan_storage_directory,
        );

        let plan_id = plan_service.generate_unique_plan_id(&req.session_id, &req.exchange_id);
        let plan_storage_path = check_plan_storage_path(app.config.clone(), plan_id.clone()).await;

        let session_service = app.session_service.clone();
        tokio::spawn(async move {
            let result = session_service
                .plan_generation(
                    req.session_id.clone(),
                    session_storage_path,
                    plan_storage_path,
                    plan_id,
                    plan_service,
                    req.exchange_id,
                    req.query,
                    req.user_context,
                    req.project_labels,
                    req.repo_ref,
                    req.root_directory,
                    req.codebase_search,
                    req.aide_rules,
                    message_properties,
                )
                .await;

            if let Err(e) = result {
                let error_msg = match e {
                    SymbolError::LLMClientError(LLMClientError::UnauthorizedAccess) => {
                        "Unauthorized access. Please check your API key and try again.".to_string()
                    }
                    _ => format!("Internal server error: {}", e),
                };
                let _ = tx.send(Ok(AgentResponse {
                    response: Some(agent_response::Response::Error(Error {
                        message: error_msg,
                        kind: ErrorKind::Internal as i32,
                    })),
                })).await;
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn agent_session_plan_iterate(
        &self,
        request: Request<AgentSessionRequest>,
    ) -> Result<Response<Self::AgentResponseStream>, Status> {
        let req = request.into_inner();
        let (tx, rx) = mpsc::channel(32);
        let app = self.app.clone();

        let llm_provider = req.model_configuration.map_or_else(
            || LLMProperties::new(
                LLMType::ClaudeSonnet,
                LLMProvider::CodeStory(CodeStoryLLMTypes::new()),
                LLMProviderAPIKeys::CodeStory(CodestoryAccessToken::new(req.access_token.clone())),
            ),
            |config| config.llm_properties_for_slow_model().unwrap_or_default(),
        );

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        let message_properties = SymbolEventMessageProperties::new(
            SymbolEventRequestId::new(req.exchange_id.clone(), req.session_id.clone()),
            tx.clone(),
            req.editor_url.clone(),
            cancellation_token.clone(),
            llm_provider,
        );

        let session_storage_path = check_session_storage_path(app.config.clone(), req.session_id.clone()).await;
        let plan_storage_directory = plan_storage_directory(app.config.clone()).await;
        let plan_service = PlanService::new(
            app.tool_box.clone(),
            app.symbol_manager.clone(),
            plan_storage_directory,
        );

        let plan_id = plan_service.generate_unique_plan_id(&req.session_id, &req.exchange_id);
        let plan_storage_path = check_plan_storage_path(app.config.clone(), plan_id.clone()).await;

        let session_service = app.session_service.clone();
        tokio::spawn(async move {
            let result = session_service
                .plan_iteration(
                    req.session_id.clone(),
                    session_storage_path,
                    plan_storage_path,
                    plan_id,
                    plan_service,
                    req.exchange_id,
                    req.query,
                    req.user_context,
                    req.aide_rules,
                    req.project_labels,
                    req.repo_ref,
                    req.root_directory,
                    req.codebase_search,
                    message_properties,
                )
                .await;

            if let Err(e) = result {
                let error_msg = match e {
                    SymbolError::LLMClientError(LLMClientError::UnauthorizedAccess) => {
                        "Unauthorized access. Please check your API key and try again.".to_string()
                    }
                    _ => format!("Internal server error: {}", e),
                };
                let _ = tx.send(Ok(AgentResponse {
                    response: Some(agent_response::Response::Error(Error {
                        message: error_msg,
                        kind: ErrorKind::Internal as i32,
                    })),
                })).await;
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn agent_session_edit_anchored(
        &self,
        request: Request<AgentSessionRequest>,
    ) -> Result<Response<Self::AgentResponseStream>, Status> {
        let req = request.into_inner();
        let (tx, rx) = mpsc::channel(32);
        let app = self.app.clone();

        let llm_provider = req.model_configuration.map_or_else(
            || LLMProperties::new(
                LLMType::ClaudeSonnet,
                LLMProvider::CodeStory(CodeStoryLLMTypes::new()),
                LLMProviderAPIKeys::CodeStory(CodestoryAccessToken::new(req.access_token.clone())),
            ),
            |config| config.llm_properties_for_slow_model().unwrap_or_default(),
        );

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        let message_properties = SymbolEventMessageProperties::new(
            SymbolEventRequestId::new(req.exchange_id.clone(), req.session_id.clone()),
            tx.clone(),
            req.editor_url.clone(),
            cancellation_token.clone(),
            llm_provider,
        );

        let session_storage_path = check_session_storage_path(app.config.clone(), req.session_id.clone()).await;
        let scratch_pad_path = check_scratch_pad_path(app.config.clone(), req.session_id.clone()).await;
        let scratch_pad_agent = ScratchPadAgent::new(
            scratch_pad_path,
            app.tool_box.clone(),
            app.symbol_manager.hub_sender(),
            None,
        ).await;

        let session_service = app.session_service.clone();
        tokio::spawn(async move {
            let result = session_service
                .code_edit_anchored(
                    req.session_id.clone(),
                    session_storage_path,
                    scratch_pad_agent,
                    req.exchange_id,
                    req.query,
                    req.user_context,
                    req.aide_rules,
                    req.project_labels,
                    req.repo_ref,
                    message_properties,
                )
                .await;

            if let Err(e) = result {
                let error_msg = match e {
                    SymbolError::LLMClientError(LLMClientError::UnauthorizedAccess) => {
                        "Unauthorized access. Please check your API key and try again.".to_string()
                    }
                    _ => format!("Internal server error: {}", e),
                };
                let _ = tx.send(Ok(AgentResponse {
                    response: Some(agent_response::Response::Error(Error {
                        message: error_msg,
                        kind: ErrorKind::Internal as i32,
                    })),
                })).await;
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn agent_session_edit_agentic(
        &self,
        request: Request<AgentSessionRequest>,
    ) -> Result<Response<Self::AgentResponseStream>, Status> {
        let req = request.into_inner();
        let (tx, rx) = mpsc::channel(32);
        let app = self.app.clone();

        let llm_provider = req.model_configuration.map_or_else(
            || LLMProperties::new(
                LLMType::ClaudeSonnet,
                LLMProvider::CodeStory(CodeStoryLLMTypes::new()),
                LLMProviderAPIKeys::CodeStory(CodestoryAccessToken::new(req.access_token.clone())),
            ),
            |config| config.llm_properties_for_slow_model().unwrap_or_default(),
        );

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        let message_properties = SymbolEventMessageProperties::new(
            SymbolEventRequestId::new(req.exchange_id.clone(), req.session_id.clone()),
            tx.clone(),
            req.editor_url.clone(),
            cancellation_token.clone(),
            llm_provider,
        );

        let session_storage_path = check_session_storage_path(app.config.clone(), req.session_id.clone()).await;
        let scratch_pad_path = check_scratch_pad_path(app.config.clone(), req.session_id.clone()).await;
        let scratch_pad_agent = ScratchPadAgent::new(
            scratch_pad_path,
            app.tool_box.clone(),
            app.symbol_manager.hub_sender(),
            None,
        ).await;

        let session_service = app.session_service.clone();
        tokio::spawn(async move {
            let result = session_service
                .code_edit_agentic(
                    req.session_id.clone(),
                    session_storage_path,
                    scratch_pad_agent,
                    req.exchange_id,
                    req.query,
                    req.user_context,
                    req.project_labels,
                    req.repo_ref,
                    req.root_directory,
                    req.codebase_search,
                    req.aide_rules,
                    message_properties,
                )
                .await;

            if let Err(e) = result {
                let error_msg = match e {
                    SymbolError::LLMClientError(LLMClientError::UnauthorizedAccess) => {
                        "Unauthorized access. Please check your API key and try again.".to_string()
                    }
                    _ => format!("Internal server error: {}", e),
                };
                let _ = tx.send(Ok(AgentResponse {
                    response: Some(agent_response::Response::Error(Error {
                        message: error_msg,
                        kind: ErrorKind::Internal as i32,
                    })),
                })).await;
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn agent_session_chat(
        &self,
        request: Request<AgentSessionRequest>,
    ) -> Result<Response<Self::AgentSessionChatStream>, Status> {
        let req = request.into_inner();
        let (tx, rx) = mpsc::channel(32);
        let app = self.app.clone();

        let llm_provider = LLMProperties::new(
            LLMType::ClaudeSonnet,
            LLMProvider::CodeStory(CodeStoryLLMTypes::new()),
            LLMProviderAPIKeys::CodeStory(CodestoryAccessToken::new(req.access_token.clone())),
        );

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        let (ui_sender, ui_receiver) = tokio::sync::mpsc::unbounded_channel();
        let message_properties = SymbolEventMessageProperties::new(
            SymbolEventRequestId::new(req.session_id.clone(), req.session_id.clone()),
            ui_sender.clone(),
            req.editor_url.clone(),
            cancellation_token.clone(),
            llm_provider,
        );

        let session_storage_path = check_session_storage_path(app.config.clone(), req.session_id.clone()).await;
        let session_service = app.session_service.clone();
        let session_id = req.session_id.clone();

        tokio::spawn(async move {
            let result = tokio::task::spawn(async move {
                session_service
                    .human_message(
                        req.session_id.clone(),
                        session_storage_path,
                        req.exchange_id.clone(),
                        req.user_query,
                        req.user_context.map(convert_user_context).unwrap_or_default(),
                        vec![], // project_labels
                        RepoRef::default(), // repo_ref
                        AideAgentMode::Chat,
                        req.aide_rules,
                        message_properties,
                    )
                    .await
            })
            .await;

            match result {
                Ok(Ok(_)) => (),
                Ok(Err(e)) => {
                    let error_msg = match e {
                        SymbolError::LLMClientError(LLMClientError::UnauthorizedAccess)
                        | SymbolError::ToolError(ToolError::LLMClientError(
                            LLMClientError::UnauthorizedAccess,
                        )) => "Unauthorized access. Please check your API key and try again.",
                        SymbolError::LLMClientError(LLMClientError::RateLimitExceeded)
                        | SymbolError::ToolError(ToolError::LLMClientError(
                            LLMClientError::RateLimitExceeded,
                        )) => "Rate limit exceeded. Please try again later.",
                        _ => "Internal server error",
                    };
                    let _ = tx.send(Ok(AgentResponse {
                        response: Some(agent_response::Response::Error(Error {
                            message: error_msg.to_string(),
                            kind: ErrorKind::Internal as i32,
                        })),
                    })).await;
                }
                Err(e) => {
                    let _ = tx.send(Ok(AgentResponse {
                        response: Some(agent_response::Response::Error(Error {
                            message: format!("Internal server error: {}", e),
                            kind: ErrorKind::Internal as i32,
                        })),
                    })).await;
                }
            }
        });

        // Convert UI events to AgentResponse
        tokio::spawn(async move {
            let mut ui_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(ui_receiver);
            while let Some(event) = ui_stream.next().await {
                match event {
                    UIEventWithID::Thinking { message, .. } => {
                        let _ = tx.send(Ok(AgentResponse {
                            response: Some(agent_response::Response::Thinking(message)),
                        })).await;
                    }
                    UIEventWithID::Action { message, .. } => {
                        let _ = tx.send(Ok(AgentResponse {
                            response: Some(agent_response::Response::Action(message)),
                        })).await;
                    }
                    UIEventWithID::Error { message, .. } => {
                        let _ = tx.send(Ok(AgentResponse {
                            response: Some(agent_response::Response::Error(Error {
                                message,
                                kind: ErrorKind::Internal as i32,
                            })),
                        })).await;
                    }
                    _ => {}
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn agent_session_edit(
        &self,
        request: Request<AgentEditRequest>,
    ) -> Result<Response<Self::AgentSessionEditStream>, Status> {
        let req = request.into_inner();
        let (tx, rx) = mpsc::channel(32);
        let app = self.app.clone();

        let session_storage_path = check_session_storage_path(app.config.clone(), req.thread_id.clone()).await;
        let scratch_pad_path = check_scratch_pad_path(app.config.clone(), req.thread_id.clone()).await;
        
        let scratch_pad_agent = ScratchPadAgent::new(
            scratch_pad_path,
            app.tool_box.clone(),
            app.symbol_manager.hub_sender(),
            None,
        ).await;

        let session_service = app.session_service.clone();
        let cloned_thread_id = req.thread_id.clone();

        tokio::spawn(async move {
            let result = tokio::task::spawn(async move {
                session_service
                    .code_edit_anchored(
                        cloned_thread_id.clone(),
                        session_storage_path,
                        scratch_pad_agent,
                        req.thread_id,
                        req.content,
                        req.context.map(convert_user_context).unwrap_or_default(),
                        req.aide_rules,
                        vec![], // project_labels
                        RepoRef::default(), // repo_ref
                        message_properties,
                    )
                    .await
            })
            .await;

            match result {
                Ok(Ok(_)) => (),
                Ok(Err(e)) => {
                    let error_msg = match e {
                        SymbolError::LLMClientError(LLMClientError::UnauthorizedAccess)
                        | SymbolError::ToolError(ToolError::LLMClientError(
                            LLMClientError::UnauthorizedAccess,
                        )) => "Unauthorized access. Please check your API key and try again.",
                        _ => "Internal server error",
                    };
                    let _ = tx.send(Ok(AgentEditResponse {
                        edited_content: String::new(),
                        modified_range: None,
                    })).await;
                }
                Err(e) => {
                    let _ = tx.send(Ok(AgentEditResponse {
                        edited_content: String::new(),
                        modified_range: None,
                    })).await;
                }
            }
        });

        // Convert UI events to AgentEditResponse
        tokio::spawn(async move {
            let mut ui_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(ui_receiver);
            while let Some(event) = ui_stream.next().await {
                match event {
                    UIEventWithID::FileEdit { content, range, .. } => {
                        let _ = tx.send(Ok(AgentEditResponse {
                            edited_content: content,
                            modified_range: Some(Range {
                                start: Some(Position {
                                    line: range.start().line() as u32,
                                    character: range.start().column() as u32,
                                }),
                                end: Some(Position {
                                    line: range.end().line() as u32,
                                    character: range.end().column() as u32,
                                }),
                            }),
                        })).await;
                    }
                    _ => {}
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn agent_tool_use(
        &self,
        request: Request<ToolUseRequest>,
    ) -> Result<Response<Self::AgentToolUseStream>, Status> {
        let req = request.into_inner();
        let (tx, rx) = mpsc::channel(32);
        let app = self.app.clone();

        let llm_provider = LLMProperties::new(
            LLMType::ClaudeSonnet,
            LLMProvider::CodeStory(CodeStoryLLMTypes::new()),
            LLMProviderAPIKeys::CodeStory(CodestoryAccessToken::new(req.access_token.clone())),
        );

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        let (ui_sender, ui_receiver) = tokio::sync::mpsc::unbounded_channel();
        let message_properties = SymbolEventMessageProperties::new(
            SymbolEventRequestId::new(req.session_id.clone(), req.session_id.clone()),
            ui_sender.clone(),
            req.editor_url.clone(),
            cancellation_token.clone(),
            llm_provider,
        );

        let session_storage_path = check_session_storage_path(app.config.clone(), req.thread_id.clone()).await;
        let session_service = app.session_service.clone();
        let cloned_thread_id = req.thread_id.clone();

        tokio::spawn(async move {
            let result = tokio::task::spawn(async move {
                session_service
                    .tool_use_agentic(
                        req.session_id.clone(),
                        session_storage_path,
                        req.tool_name,
                        req.parameters,
                        vec![], // all_files
                        vec![], // open_files
                        req.shell.unwrap_or_default(),
                        vec![], // project_labels
                        crate::repo::types::RepoRef::default(), // repo_ref
                        req.root_directory.unwrap_or_default(),
                        app.tool_box.clone(),
                        app.llm_broker.clone(),
                        req.context.map(convert_user_context).unwrap_or_default(),
                        req.aide_rules,
                        false, // reasoning
                        true,  // running in editor
                        false, // semantic_search
                        None,  // mcts_log_directory
                        None,  // repo_name
                        message_properties,
                        false, // is_devtools_context
                    )
                    .await
            })
            .await;

            match result {
                Ok(Ok(_)) => (),
                Ok(Err(e)) => {
                    let error_msg = match e {
                        SymbolError::LLMClientError(LLMClientError::UnauthorizedAccess)
                        | SymbolError::ToolError(ToolError::LLMClientError(
                            LLMClientError::UnauthorizedAccess,
                        )) => "Unauthorized access. Please check your API key and try again.",
                        SymbolError::LLMClientError(LLMClientError::RateLimitExceeded)
                        | SymbolError::ToolError(ToolError::LLMClientError(
                            LLMClientError::RateLimitExceeded,
                        )) => "Rate limit exceeded. Please try again later.",
                        _ => "Internal server error",
                    };
                    let _ = tx.send(Ok(ToolUseResponse {
                        result: String::new(),
                        success: false,
                        error: Some(error_msg.to_string()),
                    })).await;
                }
                Err(e) => {
                    let _ = tx.send(Ok(ToolUseResponse {
                        result: String::new(),
                        success: false,
                        error: Some(format!("Internal server error: {}", e)),
                    })).await;
                }
            }
        });

        // Convert UI events to ToolUseResponse
        tokio::spawn(async move {
            let mut ui_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(ui_receiver);
            while let Some(event) = ui_stream.next().await {
                match event {
                    UIEventWithID::ToolUse { result, success, .. } => {
                        let _ = tx.send(Ok(ToolUseResponse {
                            result,
                            success,
                            error: None,
                        })).await;
                    }
                    UIEventWithID::Error { message, .. } => {
                        let _ = tx.send(Ok(ToolUseResponse {
                            result: String::new(),
                            success: false,
                            error: Some(message),
                        })).await;
                    }
                    _ => {}
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn edit_file(
        &self,
        request: Request<EditFileRequest>,
    ) -> Result<Response<EditFileResponse>, Status> {
        let req = request.into_inner();
        
        match self.app.tool_box.file_edit(
            req.file_path,
            req.content,
            None,
        ).await {
            Ok(_) => Ok(Response::new(EditFileResponse {
                success: true,
                error: None,
            })),
            Err(e) => Ok(Response::new(EditFileResponse {
                success: false,
                error: Some(e.to_string()),
            })),
        }
    }

    async fn extract_documentation(
        &self,
        request: Request<DocumentationRequest>,
    ) -> Result<Response<DocumentationResponse>, Status> {
        let req = request.into_inner();
        
        self.app.tool_box.extract_documentation(
            &req.file_content,
            &req.language,
        )
        .await
        .map(|docs| Response::new(DocumentationResponse {
            documentation_strings: docs,
        }))
        .map_err(convert_error_to_status)
    }

    async fn validate_tree_sitter(
        &self,
        request: Request<TreeSitterValidationRequest>,
    ) -> Result<Response<TreeSitterValidationResponse>, Status> {
        let req = request.into_inner();
        
        match self.app.tool_box.validate_tree_sitter(
            &req.content,
            &req.language,
        ).await {
            Ok(is_valid) => Ok(Response::new(TreeSitterValidationResponse {
                is_valid,
                error: None,
            })),
            Err(e) => Ok(Response::new(TreeSitterValidationResponse {
                is_valid: false,
                error: Some(e.to_string()),
            })),
        }
    }

    async fn probe_request_stop(
        &self,
        request: Request<ProbeStopRequest>,
    ) -> Result<Response<ProbeStopResponse>, Status> {
        let req = request.into_inner();
        let probe_request_tracker = self.app.probe_request_tracker.clone();
        let _ = probe_request_tracker.cancel_request(&req.request_id).await;
        let anchored_editing_tracker = self.app.anchored_request_tracker.clone();
        let _ = anchored_editing_tracker.cancel_request(&req.request_id).await;
        Ok(Response::new(ProbeStopResponse { done: true }))
    }

    async fn code_sculpting(
        &self,
        request: Request<CodeSculptingRequest>,
    ) -> Result<Response<Self::AgentResponseStream>, Status> {
        let req = request.into_inner();
        let (tx, rx) = mpsc::channel(32);
        let app = self.app.clone();

        let anchor_properties = app.anchored_request_tracker.get_properties(&req.request_id).await;
        if anchor_properties.is_none() {
            let _ = tx.send(Ok(AgentResponse {
                response: Some(agent_response::Response::Error(Error {
                    message: "No properties found for request".to_string(),
                    kind: ErrorKind::NotFound as i32,
                })),
            })).await;
            return Ok(Response::new(ReceiverStream::new(rx)));
        }

        let anchor_properties = anchor_properties.unwrap();
        tokio::spawn(async move {
            let join_handle = tokio::spawn(async move {
                let anchored_symbols = anchor_properties.anchored_symbols;
                let user_provided_context = anchor_properties.user_context_string;
                let environment_sender = anchor_properties.environment_event_sender;
                let message_properties = anchor_properties.message_properties.clone();
                let _ = environment_sender.send(EnvironmentEvent::event(
                    EnvironmentEventType::human_anchor_request(
                        req.instruction,
                        anchored_symbols,
                        user_provided_context,
                    ),
                    message_properties,
                ));
            });
            let anchor_tracker = app.anchored_request_tracker.clone();
            let _ = anchor_tracker
                .override_running_request(&req.request_id, join_handle)
                .await;
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn code_sculpting_heal(
        &self,
        request: Request<CodeSculptingHealRequest>,
    ) -> Result<Response<CodeSculptingHealResponse>, Status> {
        let req = request.into_inner();
        let anchor_properties = self.app.anchored_request_tracker.get_properties(&req.request_id).await;
        
        if anchor_properties.is_none() {
            return Ok(Response::new(CodeSculptingHealResponse { done: false }));
        }

        let anchor_properties = anchor_properties.unwrap();
        let anchored_symbols = anchor_properties.anchored_symbols();
        let relevant_references = anchor_properties.references();
        let file_paths = anchored_symbols
            .iter()
            .filter_map(|r| r.fs_file_path())
            .collect::<Vec<_>>();

        Ok(Response::new(CodeSculptingHealResponse { done: true }))
    }

    async fn push_diagnostics(
        &self,
        request: Request<AgenticDiagnosticsRequest>,
    ) -> Result<Response<AgenticDiagnosticsResponse>, Status> {
        let req = request.into_inner();
        let diagnostics = req.diagnostics.into_iter().map(|d| {
            LSPDiagnosticError::new(
                d.range,
                d.range_content,
                req.fs_file_path.clone(),
                d.message,
                None,
                None,
            )
        }).collect::<Vec<_>>();

        Ok(Response::new(AgenticDiagnosticsResponse { done: true }))
    }

    async fn swe_bench(
        &self,
        request: Request<SweBenchRequest>,
    ) -> Result<Response<SweBenchResponse>, Status> {
        // Currently returns a simple response as per the HTTP implementation
        Ok(Response::new(SweBenchResponse { done: true }))
    }

    async fn verify_model_config(
        &self,
        request: Request<VerifyModelConfigRequest>,
    ) -> Result<Response<VerifyModelConfigResponse>, Status> {
        // Short-circuit the reply as per HTTP implementation
        Ok(Response::new(VerifyModelConfigResponse {
            valid: true,
            error: None,
        }))
    }

    async fn cancel_running_exchange(
        &self,
        request: Request<CancelExchangeRequest>,
    ) -> Result<Response<Self::AgentResponseStream>, Status> {
        let req = request.into_inner();
        let (tx, rx) = mpsc::channel(32);
        let session_service = self.app.session_service.clone();

        if let Some(cancellation_token) = session_service
            .get_cancellation_token(&req.session_id, &req.exchange_id)
            .await
        {
            cancellation_token.cancel();

            let llm_provider = LLMProperties::new(
                LLMType::ClaudeSonnet,
                LLMProvider::CodeStory(CodeStoryLLMTypes::new()),
                LLMProviderAPIKeys::CodeStory(CodestoryAccessToken::new(req.access_token.clone())),
            );

            let cancellation_token = tokio_util::sync::CancellationToken::new();
            let message_properties = SymbolEventMessageProperties::new(
                SymbolEventRequestId::new(req.exchange_id.clone(), req.session_id.clone()),
                tx.clone(),
                req.editor_url,
                cancellation_token.clone(),
                llm_provider,
            );

            let session_storage_path = check_session_storage_path(self.app.config.clone(), req.session_id.clone()).await;

            tokio::time::sleep(Duration::from_millis(600)).await;

            session_service
                .set_exchange_as_cancelled(
                    session_storage_path,
                    req.exchange_id.clone(),
                    message_properties,
                )
                .await
                .unwrap_or_default();

            let _ = tx.send(Ok(AgentResponse {
                response: Some(agent_response::Response::Action(format!(
                    "Exchange {} cancelled",
                    req.exchange_id
                ))),
            })).await;
        }

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn user_feedback_on_exchange(
        &self,
        request: Request<FeedbackExchangeRequest>,
    ) -> Result<Response<Self::AgentResponseStream>, Status> {
        let req = request.into_inner();
        let (tx, rx) = mpsc::channel(32);

        let llm_provider = LLMProperties::new(
            LLMType::ClaudeSonnet,
            LLMProvider::CodeStory(CodeStoryLLMTypes::new()),
            LLMProviderAPIKeys::CodeStory(CodestoryAccessToken::new(req.access_token.clone())),
        );

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        let message_properties = SymbolEventMessageProperties::new(
            SymbolEventRequestId::new(req.exchange_id.clone(), req.session_id.clone()),
            tx.clone(),
            req.editor_url,
            cancellation_token.clone(),
            llm_provider,
        );

        let session_storage_path = check_session_storage_path(self.app.config.clone(), req.session_id.clone()).await;
        let session_service = self.app.session_service.clone();

        tokio::spawn(async move {
            let _ = session_service
                .feedback_for_exchange(
                    &req.exchange_id,
                    req.step_index.map(|i| i as usize),
                    req.accepted,
                    session_storage_path,
                    self.app.tool_box.clone(),
                    message_properties,
                )
                .await;
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn handle_session_undo(
        &self,
        request: Request<SessionUndoRequest>,
    ) -> Result<Response<SessionUndoResponse>, Status> {
        let req = request.into_inner();
        Ok(Response::new(SessionUndoResponse { done: true }))
    }
}

impl AgentFarmGrpcServer {
    pub fn new(app: Application) -> Self {
        Self { app }
    }

    pub async fn serve(self, addr: std::net::SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
        let svc = agent_farm_service_server::AgentFarmServiceServer::new(self);
        Server::builder()
            .add_service(svc)
            .serve(addr)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;