use llm_client::{
    clients::{
        google_ai::GoogleAIStdioClient,
        types::{LLMClient, LLMClientCompletionRequest, LLMClientMessage, LLMType},
    },
    provider::{GoogleAIStudioKey, LLMProviderAPIKeys},
};

#[tokio::main]
async fn main() {
    let api_key = LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new("".to_owned()));
    let request = LLMClientCompletionRequest::from_messages(
        vec![
            LLMClientMessage::system("You are an expert at saying hi to me".to_owned()),
            LLMClientMessage::user("Please say something to me".to_owned()),
        ],
        LLMType::GeminiProFlash,
    );
    let google_ai_client = GoogleAIStdioClient::new();
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let response = google_ai_client
        .stream_completion(api_key, request, sender)
        .await;
    println!("{:?}", &response);
}
