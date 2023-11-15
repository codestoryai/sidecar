pub async fn generate_agent_stream(
    mut agent: Agent,
    mut action: AgentAction,
    receiver: tokio::sync::mpsc::Receiver<ConversationMessage>,
) -> Result<
    Sse<std::pin::Pin<Box<dyn tokio_stream::Stream<Item = anyhow::Result<sse::Event>> + Send>>>,
> {
    // Existing code
    while let Some(Some(conversation_message)) = conversation_message_stream.next().now_or_never() {
        tracing::info!(
            "Yielding from conversation_message_stream: {:?}",
            conversation_message
        );
        yield conversation_message;
    }
}
