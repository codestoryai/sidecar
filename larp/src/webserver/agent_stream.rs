use llm_client::clients::types::LLMClientError;
use std::time::Duration;

use axum::response::{sse, Sse};
use either::Either;
use futures::stream;
use futures::StreamExt;
use serde_json::json;
use tracing::error;

use crate::agent::types::{Agent, AgentAction, ConversationMessage};

use super::types::Result;

// We give a timeout of 4 minute between responses
const TIMEOUT_SECS: u64 = 60 * 10;

/// We can use this to invoke the agent and get the stream of responses back

pub async fn generate_agent_stream(
    mut agent: Agent,
    mut action: AgentAction,
    receiver: tokio::sync::mpsc::Receiver<ConversationMessage>,
) -> Result<
    Sse<std::pin::Pin<Box<dyn tokio_stream::Stream<Item = anyhow::Result<sse::Event>> + Send>>>,
> {
    let session_id = uuid::Uuid::new_v4();

    // Process the events in parallel here
    let conversation_message_stream = async_stream::try_stream! {
        let (answer_sender, answer_receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut answer_receiver_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(answer_receiver);
        let mut conversation_message_stream = tokio_stream::wrappers::ReceiverStream::new(receiver);

        // poll from both the streams at the same time, we should probably move
        // this to a common place later on as I can see many other places doing
        // the same thing
        let result = 'outer: loop {

            use futures::future::FutureExt;

            let conversation_message_stream_left = (&mut conversation_message_stream).map(Either::Left);
            // map the agent conversation update stream to right::left
            let agent_conversation_update_stream_right = agent
                .iterate(action, answer_sender.clone())
                .into_stream()
                .map(|answer| Either::Right(Either::Left(answer)));
            // map the agent answer stream to right::right
            let agent_answer_delta_stream_left = (&mut answer_receiver_stream).map(|answer| Either::Right(Either::Right(answer)));

            let timeout = Duration::from_secs(TIMEOUT_SECS);
            let mut next = None;
            for await item in tokio_stream::StreamExt::timeout(
                stream::select(conversation_message_stream_left, stream::select(agent_conversation_update_stream_right, agent_answer_delta_stream_left)),
                timeout,
            ) {
                match item {
                    Ok(Either::Left(conversation_message)) => yield conversation_message,
                    Ok(Either::Right(Either::Left(next_action))) => match next_action {
                        Ok(n) => break next = n,
                        Err(e) => {
                            // Check if error is an LLMClientError::UnauthorizedAccess
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
                        // We are going to send the answer update in the same
                        // way as we send the answer
                        let conversation_message = ConversationMessage::answer_update(session_id, answer_update);
                        yield conversation_message
                    }
                    Err(_) => break 'outer Err(anyhow::anyhow!("timeout")),
                }
            }

            // If we have some elements which are still present in the stream, we
            // return them here so as to not loose things in case the timeout got triggered
            // this is basically draining the stream properly
            while let Some(Some(conversation_message)) = conversation_message_stream.next().now_or_never() {
                yield conversation_message.clone();
            }

            // yield the answer from the answer stream so we can send incremental updates here
            while let Some(Some(answer_update)) = answer_receiver_stream.next().now_or_never() {
                let conversation_message = ConversationMessage::answer_update(session_id, answer_update);
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

    // // We know the stream is unwind safe as it doesn't use synchronization primitives like locks.
    let answer_stream = conversation_message_stream.map(
        |conversation_message: anyhow::Result<ConversationMessage>| {
            if let Err(e) = &conversation_message {
                error!("error in conversation message stream: {}", e);
            }
            sse::Event::default()
                .json_data(conversation_message.expect("should not fail deserialization"))
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
