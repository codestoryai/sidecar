use once_cell::sync::OnceCell;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::application::config::configuration::Configuration;

static LOGGER_GUARD: OnceCell<tracing_appender::non_blocking::WorkerGuard> = OnceCell::new();

pub fn tracing_subscribe(config: &Configuration) -> bool {
    let env_filter_layer = fmt::layer()
        // Disable the hyper logs or else its a lot of log spam
        .with_filter(
            EnvFilter::from_default_env()
                .add_directive("hyper=off".parse().unwrap())
                .add_directive("tantivy=off".parse().unwrap()), // .add_directive("error".parse().unwrap()),
        );
    let file_appender = tracing_appender::rolling::daily(config.log_dir(), "codestory.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    _ = LOGGER_GUARD.set(guard);
    let log_writer_layer = fmt::layer().with_writer(non_blocking).with_ansi(false);

    #[cfg(all(tokio_unstable, debug_assertions))]
    let console_subscriber_layer = Some(console_subscriber::spawn());
    #[cfg(not(all(tokio_unstable, debug_assertions)))]
    let console_subscriber_layer: Option<Box<dyn tracing_subscriber::Layer<_> + Send + Sync>> =
        None;

    tracing_subscriber::registry()
        .with(log_writer_layer)
        .with(env_filter_layer)
        .with(console_subscriber_layer)
        .try_init()
        .is_ok()
}

pub fn tracing_subscribe_default() -> bool {
    let log_dir = "/tmp";
    let env_filter_layer = fmt::layer()
        // Disable the hyper logs or else its a lot of log spam
        .with_filter(
            EnvFilter::from_default_env()
                .add_directive("hyper=off".parse().unwrap())
                .add_directive("tantivy=off".parse().unwrap()), // .add_directive("error".parse().unwrap()),
        );
    let file_appender = tracing_appender::rolling::daily(log_dir, "codestory.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    _ = LOGGER_GUARD.set(guard);
    let log_writer_layer = fmt::layer().with_writer(non_blocking).with_ansi(false);

    tracing_subscriber::registry()
        .with(log_writer_layer)
        .with(env_filter_layer)
        .try_init()
        .is_ok()
}
