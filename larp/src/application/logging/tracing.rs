use once_cell::sync::OnceCell;
use std::time::Duration;
use tokio::task;
use tracing::{debug, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use super::cleanup::cleanup_old_logs;
use crate::application::config::configuration::Configuration;

static LOGGER_GUARD: OnceCell<tracing_appender::non_blocking::WorkerGuard> = OnceCell::new();

pub fn tracing_subscribe(config: &Configuration) -> bool {
    // Create log directory if it doesn't exist
    let log_dir = config.log_dir();
    if !log_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&log_dir) {
            warn!("Failed to create log directory: {}", e);
            return false;
        }
    }

    let env_filter_layer = fmt::layer().with_filter(
        EnvFilter::from_default_env()
            .add_directive("hyper=off".parse().unwrap())
            .add_directive("tantivy=off".parse().unwrap()),
    );
    let file_appender = tracing_appender::rolling::daily(&log_dir, "codestory.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    _ = LOGGER_GUARD.set(guard);
    let log_writer_layer = fmt::layer().with_writer(non_blocking).with_ansi(false);

    #[cfg(all(tokio_unstable, feature = "debug"))]
    let console_subscriber_layer = Some(console_subscriber::spawn());
    #[cfg(not(all(tokio_unstable, feature = "debug")))]
    let console_subscriber_layer: Option<Box<dyn tracing_subscriber::Layer<_> + Send + Sync>> =
        None;

    let init_success = tracing_subscriber::registry()
        .with(log_writer_layer)
        .with(env_filter_layer)
        .with(console_subscriber_layer)
        .try_init()
        .is_ok();

    if init_success {
        // Start background cleanup task
        let log_dir = log_dir.clone();
        task::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await; // Run daily
                if let Err(e) = cleanup_old_logs(&log_dir).await {
                    warn!("Failed to cleanup old logs: {}", e);
                }
            }
        });
        debug!("Log cleanup task started");
    }

    init_success
}

pub fn tracing_subscribe_default() -> bool {
    let log_dir = "/tmp";
    let env_filter_layer = fmt::layer().with_filter(
        EnvFilter::from_default_env()
            .add_directive("hyper=off".parse().unwrap())
            .add_directive("tantivy=off".parse().unwrap()),
    );
    let file_appender = tracing_appender::rolling::daily(log_dir, "codestory.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    _ = LOGGER_GUARD.set(guard);
    let log_writer_layer = fmt::layer().with_writer(non_blocking).with_ansi(false);

    let init_success = tracing_subscriber::registry()
        .with(log_writer_layer)
        .with(env_filter_layer)
        .try_init()
        .is_ok();

    if init_success {
        // Start background cleanup task for default logger
        let log_dir = log_dir.to_owned();
        task::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await; // Run daily
                if let Err(e) = cleanup_old_logs(&log_dir).await {
                    warn!("Failed to cleanup old logs: {}", e);
                }
            }
        });
        debug!("Log cleanup task started");
    }

    init_success
}
