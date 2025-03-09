// This is where we will create the default webserver for running the binary
// locally

use anyhow::Result;
use axum::extract::DefaultBodyLimit;
use axum::routing::get;
use axum::Extension;
use clap::Parser;
use sidecar::application::{application::Application, config::configuration::Configuration};
use std::net::SocketAddr;
use tokio::signal;
use tokio::sync::oneshot;
use tower_http::{catch_panic::CatchPanicLayer, cors::CorsLayer};
use tracing::{debug, error, info};

use axum::http::header::AUTHORIZATION;
use axum::{
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};
pub type Router<S = Application> = axum::Router<S>;

#[tokio::main]
async fn main() -> Result<()> {
    info!("CodeStory 🚀");
    let configuration = Configuration::parse();

    // We get the logging setup first
    debug!("installing logging to local file");
    Application::install_logging(&configuration);

    // We create our scratch-pad directory
    Application::setup_scratch_pad(&configuration).await;

    // Create a oneshot channel
    let (tx, rx) = oneshot::channel();

    // Spawn a task to listen for signals
    tokio::spawn(async move {
        signal::ctrl_c().await.expect("failed to listen for event");
        let _ = tx.send(());
    });

    // We initialize the logging here
    let application = Application::initialize(configuration).await?;
    println!("initialized application");
    debug!("initialized application");

    // Main logic
    tokio::select! {
        // Start the webserver
        _ = run(application) => {
            // Your server logic
        }
        _ = rx => {
            // Signal received, this block will be executed.
            // Drop happens automatically when variables go out of scope.
            debug!("Signal received, cleaning up...");
        }
    }

    Ok(())
}

pub async fn run(application: Application) -> Result<()> {
    let mut joins = tokio::task::JoinSet::new();

    joins.spawn(start(application));

    while let Some(result) = joins.join_next().await {
        if let Ok(Err(err)) = result {
            error!(?err, "sidecar failed");
            return Err(err);
        }
    }

    Ok(())
}

// reintroduce when necessary
async fn _auth_middleware<B>(request: Request<B>, next: Next<B>) -> Result<Response, StatusCode> {
    // Get token from Authorization header
    let auth_header = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|header| header.to_str().ok());

    dbg!(&auth_header);

    match auth_header {
        Some(token) => {
            // Check if token starts with "Bearer "
            if let Some(token) = token.strip_prefix("Bearer ") {
                // Validate token here
                if _is_valid_token(token).await {
                    Ok(next.run(request).await)
                } else {
                    Err(StatusCode::UNAUTHORIZED)
                }
            } else {
                Err(StatusCode::UNAUTHORIZED)
            }
        }
        None => Err(StatusCode::UNAUTHORIZED),
    }
}

// Token validation function (implement your own logic)
async fn _is_valid_token(token: &str) -> bool {
    println!("webserver::is_valid_token::token({})", token);

    match _validate_workos_token(token).await {
        Ok(_) => true,
        Err(_) => false,
    }
}

async fn _validate_workos_token(token: &str) -> Result<bool> {
    let client = reqwest::Client::new();

    let auth_proxy_endpoint = "";

    let response = client
        .get(auth_proxy_endpoint)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    dbg!(&response);

    Ok(response.status().is_success())
}

// TODO(skcd): Add routes here which can do the following:
// - when a file changes, it should still be logged and tracked
// - when a file is opened, it should be tracked over here too
pub async fn start(app: Application) -> anyhow::Result<()> {
    println!("Port: {}", app.config.port);
    let bind = SocketAddr::new(app.config.host.parse()?, app.config.port);

    // routes through middleware
    let protected_routes = Router::new()
        .nest("/agentic", agentic_router())
        .nest("/plan", plan_router());

    // no middleware check
    let public_routes = Router::new()
        .route("/config", get(sidecar::webserver::config::get))
        .route(
            "/reach_the_devs",
            get(sidecar::webserver::config::reach_the_devs),
        )
        .route("/version", get(sidecar::webserver::config::version))
        .nest("/tree_sitter", tree_sitter_router())
        .nest("/file", file_operations_router());

    // both protected and public merged into api
    let mut api = Router::new().merge(protected_routes).merge(public_routes);

    api = api.route("/health", get(sidecar::webserver::health::health));

    // Create the router with application state
    let router = sidecar::webserver::create_router(app.clone()).await?;
    
    let api = api
        .merge(router)
        .layer(Extension(app.clone()))
        .with_state(app.clone())
        .with_state(app.clone())
        .layer(CorsLayer::permissive())
        .layer(CatchPanicLayer::new())
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024));

    let router = Router::new().nest("/api", api);

    axum::Server::bind(&bind)
        .serve(router.into_make_service())
        .await?;

    Ok(())
}

fn plan_router() -> Router {
    use axum::routing::*;
    Router::new()
    // Probe request routes
    // These routes handle starting and stopping probe requests
}

// Define routes for agentic operations
// Define the router for agentic operations
// This router handles various AI-assisted code operations and benchmarking
fn agentic_router() -> Router {
    use axum::routing::*;
    Router::new()
        .route(
            "/probe_request_stop",
            post(sidecar::webserver::agentic::probe_request_stop),
        )
        .route(
            "/code_sculpting_followup",
            post(sidecar::webserver::agentic::code_sculpting),
        )
        .route(
            "/code_sculpting_heal",
            post(sidecar::webserver::agentic::code_sculpting_heal),
        )
        // route for push events coming from the editor
        .route(
            "/diagnostics",
            post(sidecar::webserver::agentic::push_diagnostics),
        )
        // SWE bench route
        // This route is for software engineering benchmarking
        .route("/swe_bench", get(sidecar::webserver::agentic::swe_bench))
        .route(
            "/agent_session_chat",
            post(sidecar::webserver::agentic::agent_session_chat),
        )
        .route(
            "/agent_session_edit_anchored",
            post(sidecar::webserver::agentic::agent_session_edit_anchored),
        )
        .route(
            "/agent_session_edit_agentic",
            post(sidecar::webserver::agentic::agent_session_edit_agentic),
        )
        .route(
            "/agent_session_plan",
            post(sidecar::webserver::agentic::agent_session_plan),
        )
        .route(
            "/agent_session_plan_iterate",
            post(sidecar::webserver::agentic::agent_session_plan_iterate),
        )
        .route(
            "/agent_tool_use",
            post(sidecar::webserver::agentic::agent_tool_use),
        )
        .route(
            "/verify_model_config",
            post(sidecar::webserver::agentic::verify_model_config),
        )
        .route(
            "/cancel_running_event",
            post(sidecar::webserver::agentic::cancel_running_exchange),
        )
        .route(
            "/user_feedback_on_exchange",
            post(sidecar::webserver::agentic::user_feedback_on_exchange),
        )
        .route(
            "/user_handle_session_undo",
            post(sidecar::webserver::agentic::handle_session_undo),
        )
        .route(
            "/move_to_checkpoint",
            post(sidecar::webserver::agentic::move_to_checkpoint),
        )
        .route(
            "/get_mcts_data",
            post(sidecar::webserver::agentic::get_mcts_data),
        )
}

fn tree_sitter_router() -> Router {
    use axum::routing::*;
    Router::new()
        .route(
            "/documentation_parsing",
            post(sidecar::webserver::tree_sitter::extract_documentation_strings),
        )
        .route(
            "/diagnostic_parsing",
            post(sidecar::webserver::tree_sitter::extract_diagnostics_range),
        )
        .route(
            "/tree_sitter_valid",
            post(sidecar::webserver::tree_sitter::tree_sitter_node_check),
        )
        .route(
            "/valid_xml",
            post(sidecar::webserver::tree_sitter::check_valid_xml),
        )
}

fn file_operations_router() -> Router {
    use axum::routing::*;
    Router::new().route("/edit_file", post(sidecar::webserver::file_edit::file_edit))
}