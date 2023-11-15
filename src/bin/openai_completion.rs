use futures::StreamExt;
use sidecar::agent::llm_funcs;

fn main() -> anyhow::Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async { main_func().await })
}

async fn main_func() -> anyhow::Result<()> {
    let llm_client = llm_funcs::LlmClient::codestory_infra();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let _ = llm_client
        .stream_completion_call(
            llm_funcs::llm::OpenAIModel::GPT3_5Instruct,
            "What are we doing here",
            sender,
            None,
        )
        .await
        .expect("to not fail");
    let receiver_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);
    receiver_stream
        .for_each(|item| {
            dbg!(&item);
            futures::future::ready(())
        })
        .await;
    Ok(())
}
