// This is where we will create the default webserver for running the binary
// locally

use anyhow::Result;
use axum::extract::DefaultBodyLimit;
use axum::routing::get;
use axum::Extension;
use clap::Parser;
use sidecar::{
    application::{application::Application, config::configuration::Configuration},
    bg_poll::background_polling::poll_repo_updates,
    semantic_search::qdrant_process::{wait_for_qdrant, QdrantServerProcess},
};
use std::net::SocketAddr;
use tokio::signal;
use tokio::sync::oneshot;
use tower_http::{catch_panic::CatchPanicLayer, cors::CorsLayer};
use tracing::{debug, error, info};

pub type Router<S = Application> = axum::Router<S>;

#[tokio::main]
async fn main() -> Result<()> {
    info!("CodeStory 🚀");
    let configuration = Configuration::parse();

    // Star the qdrant server and make sure that it has started up
    // let qdrant_process = QdrantServerProcess::initialize(&configuration).await?;
    // HC the process here to make sure that it has started up
    // wait_for_qdrant().await;
    debug!("qdrant server started");

    // We get the logging setup first
    debug!("installing logging to local file");
    Application::install_logging(&configuration);

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
            // We drop the qdrant process explicitly to not leave anything behind
            // drop(qdrant_process);
        }
    }

    Ok(())
}

pub async fn run(application: Application) -> Result<()> {
    let mut joins = tokio::task::JoinSet::new();

    // Start background tasks here
    if application.config.enable_background_polling {
        tokio::spawn(poll_repo_updates(application.clone()));
    }

    joins.spawn(start(application));

    while let Some(result) = joins.join_next().await {
        if let Ok(Err(err)) = result {
            error!(?err, "sidecar failed");
            return Err(err);
        }
    }

    Ok(())
}

// TODO(skcd): Add routes here which can do the following:
// - when a file changes, it should still be logged and tracked
// - when a file is opened, it should be tracked over here too
pub async fn start(app: Application) -> anyhow::Result<()> {
    println!("Port: {}", app.config.port);
    let bind = SocketAddr::new(app.config.host.parse()?, app.config.port);
    let mut api = Router::new()
        .route("/config", get(sidecar::webserver::config::get))
        .route(
            "/reach_the_devs",
            get(sidecar::webserver::config::reach_the_devs),
        )
        .route("/version", get(sidecar::webserver::config::version))
        .nest("/repo", repo_router())
        .nest("/agent", agent_router())
        .nest("/in_editor", in_editor_router())
        .nest("/tree_sitter", tree_sitter_router())
        .nest("/file", file_operations_router())
        .nest("/inline_completion", inline_completion())
        .nest("/agentic", agentic_router());

    api = api.route("/health", get(sidecar::webserver::health::health));

    let api = api
        .layer(Extension(app.clone()))
        .with_state(app.clone())
        .with_state(app.clone())
        .layer(CorsLayer::permissive())
        .layer(CatchPanicLayer::new())
        // I want to set the bytes limit here to 20 MB
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024));

    let router = Router::new().nest("/api", api);

    axum::Server::bind(&bind)
        .serve(router.into_make_service())
        .await?;

    Ok(())
}

fn repo_router() -> Router {
    use axum::routing::*;
    Router::new()
        // 127.0.0.1:42424/api/repo/sync?backend=local/{path_absolute}
        .route("/sync", get(sidecar::webserver::repos::sync))
        .route("/status", get(sidecar::webserver::repos::index_status))
        // Gives back the status of the queue
        .route("/queue", get(sidecar::webserver::repos::queue_status))
        // Gives back the repos we know about
        .route("/repo_list", get(sidecar::webserver::repos::repo_status))
}

fn agentic_router() -> Router {
    use axum::routing::*;
    Router::new()
        .route(
            "/probe_request",
            post(sidecar::webserver::agentic::probe_request),
        )
        .route("/swe_bench", post(sidecar::webserver::agentic::swe_bench))
}

fn agent_router() -> Router {
    use axum::routing::*;
    Router::new()
        .route(
            "/search_agent",
            get(sidecar::webserver::agent::search_agent),
        )
        .route(
            "/hybrid_search",
            get(sidecar::webserver::agent::hybrid_search),
        )
        .route("/explain", get(sidecar::webserver::agent::explain))
        .route(
            "/followup_chat",
            post(sidecar::webserver::agent::followup_chat),
        )
}

fn in_editor_router() -> Router {
    use axum::routing::*;
    Router::new().route(
        "/answer",
        post(sidecar::webserver::in_line_agent::reply_to_user),
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

fn inline_completion() -> Router {
    use axum::routing::*;
    Router::new()
        .route(
            "/inline_completion",
            post(sidecar::webserver::inline_completion::inline_completion),
        )
        .route(
            "/cancel_inline_completion",
            post(sidecar::webserver::inline_completion::cancel_inline_completion),
        )
        .route(
            "/document_open",
            post(sidecar::webserver::inline_completion::inline_document_open),
        )
        .route(
            "/document_content_changed",
            post(sidecar::webserver::inline_completion::inline_completion_file_content_change),
        )
        .route(
            "/get_document_content",
            post(sidecar::webserver::inline_completion::inline_completion_file_content),
        )
        .route(
            "/get_identifier_nodes",
            post(sidecar::webserver::inline_completion::get_identifier_nodes),
        )
        .route(
            "/get_symbol_history",
            post(sidecar::webserver::inline_completion::symbol_history),
        )
}

// TODO(skcd): Figure out why we are passing the context in the suffix and not the prefix
