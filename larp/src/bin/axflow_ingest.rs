//! Helper script to test ingestion with axflow

use sidecar::agent::llm_funcs;

#[tokio::main]
async fn main() {
    let axflow_client = sidecar::reporting::axflow::client::client();
    let llm_request = llm_funcs::llm::Request {
        messages: llm_funcs::llm::Messages {
            messages: vec![
                llm_funcs::llm::Message::system("testing ingestion"),
                llm_funcs::llm::Message::user("testing ingestion user"),
            ],
        },
        functions: None,
        provider: llm_funcs::llm::Provider::OpenAi,
        max_tokens: None,
        temperature: Some(1.0),
        presence_penalty: None,
        frequency_penalty: None,
        model: "gpt-4".to_owned(),
        extra_stop_sequences: vec![],
        session_reference_id: None,
    };

    let response = axflow_client.capture(llm_request).await;
    dbg!(&response);
}
