use llm_client::clients::types::LLMClientError;
use std::time::Duration;

use axum::response::{sse, Sse};
use either::Either;
use futures::{stream, StreamExt};
use serde_json::json;
use tracing::error;

use super::types::Result;

use crate::in_line_agent::types::{InLineAgent, InLineAgentAction, InLineAgentMessage};

const TIMEOUT_SECS: u64 = 60;

pub async fn generate_in_line_agent_stream(
    mut in_line_agent: InLineAgent,
    mut action: InLineAgentAction,
    receiver: tokio::sync::mpsc::Receiver<InLineAgentMessage>,
) -> Result<
    Sse<std::pin::Pin<Box<dyn tokio_stream::Stream<Item = anyhow::Result<sse::Event>> + Send>>>,
> {
    let session_id = uuid::Uuid::new_v4();

    let in_line_agent_messages = async_stream::try_stream! {
        let (answer_sender, answer_receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut action_receiver_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(answer_receiver);
        let mut conversation_message_stream = tokio_stream::wrappers::ReceiverStream::new(receiver);

        let result = 'outer: loop {
            use futures::future::FutureExt;

            let in_line_editor_message_stream_left = (&mut conversation_message_stream).map(Either::Left);
            let in_line_agent_update_stream_right = in_line_agent.iterate(action, answer_sender.clone()).into_stream().map(|answer| Either::Right(Either::Left(answer)));

            let in_line_agent_answer_delta_stream_left = (&mut action_receiver_stream).map(|answer| Either::Right(Either::Right(answer)));

            let timeout = Duration::from_secs(TIMEOUT_SECS);

            let mut next = None;
            for await item in tokio_stream::StreamExt::timeout(
                stream::select(in_line_editor_message_stream_left, stream::select(in_line_agent_update_stream_right, in_line_agent_answer_delta_stream_left)),
                timeout,
            ) {
                match item {
                    Ok(Either::Left(in_line_editor_message)) => yield in_line_editor_message,
                    Ok(Either::Right(Either::Left(next_action))) => match next_action {
                        Ok(n) => break next = n,
                        Err(e) => {
                            // Check if error is an LLMClientError::UnauthorizedAccess or RateLimitExceeded
                            if let Some(llm_err) = e.source() {
                                if let Some(llm_client_err) = llm_err.downcast_ref::<LLMClientError>() {
                                    if matches!(llm_client_err, LLMClientError::UnauthorizedAccess | LLMClientError::RateLimitExceeded) {
                                        break 'outer Err(e);
                                    }
                                }
                            }
                            break 'outer Err(anyhow::anyhow!(e))
                        },
                    },
                    Ok(Either::Right(Either::Right(answer_update))) => {
                        let conversation_message = InLineAgentMessage::answer_update(session_id, answer_update);
                        yield conversation_message
                    }
                    Err(_) => break 'outer Err(anyhow::anyhow!("timeout")),
                }
            }

            while let Some(Some(in_line_agent_message)) = conversation_message_stream.next().now_or_never() {
                yield in_line_agent_message
            }

            while let Some(Some(answer_update)) = action_receiver_stream.next().now_or_never() {
                let conversation_message = InLineAgentMessage::answer_update(session_id, answer_update);
                yield conversation_message
            }

            match next {
                Some(a) => action = a,
                None => break Ok(()),
            }
        };
        result?;
    };

    // TODO(skcd): Re-introduce this again when we have a better way to manage
    // server side events on the client side
    let init_stream = futures::stream::once(async move {
        Ok(sse::Event::default()
            .json_data(json!({
                "session_id": session_id,
            }))
            // This should never happen, so we force an unwrap.
            .expect("failed to serialize initialization object"))
    });

    // We know the stream is unwind safe as it doesn't use synchronization primitives like locks.
    let answer_stream = in_line_agent_messages.map(
        |in_line_agent_message: anyhow::Result<InLineAgentMessage>| {
            if let Err(e) = &in_line_agent_message {
                error!("error in conversation message stream: {}", e);
            }
            sse::Event::default()
                .json_data(in_line_agent_message.expect("should not fail deserialization"))
                .map_err(anyhow::Error::new)
        },
    );

    // TODO(skcd): Re-introduce this again when we have a better way to manage
    // server side events on the client side
    let done_stream = futures::stream::once(async move {
        Ok(sse::Event::default()
            .json_data(json!(
                {"done": "[CODESTORY_DONE]".to_owned(),
                "session_id": session_id,
            }))
            .expect("failed to send done object"))
    });

    let stream = init_stream.chain(answer_stream).chain(done_stream);

    Ok(Sse::new(Box::pin(stream)))
}
