//! Runs the future with cancellation so we can complete as soon as we are done

use std::future::Future;
pub async fn run_with_cancellation<F, T>(
    cancel_token: tokio_util::sync::CancellationToken,
    future: F,
) -> Option<T>
where
    F: Future<Output = T>,
{
    tokio::select! {
        res = future => Some(res),               // Future completed successfully
        _ = cancel_token.cancelled() => None,    // Cancellation token was triggered
    }
}
