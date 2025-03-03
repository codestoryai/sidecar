use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage, LLMType},
    provider::{CodeStoryLLMTypes, LLMProvider, LLMProviderAPIKeys, CodestoryAccessToken},
};
use std::{collections::HashMap, error::Error};
use tokio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dbg!("Starting O3Mini example...");
    
    // Create LLM broker
    let broker = LLMBroker::new().await.map_err(|e| Box::new(e) as Box<dyn Error>)?;
    dbg!("LLM broker created");

    // Create a CodeStory provider with O3MiniHigh model
    let provider = LLMProvider::CodeStory(CodeStoryLLMTypes {
        llm_type: Some(LLMType::O3MiniHigh),
    });
    dbg!("Provider configured");

    // Example messages for the model
    let messages = vec![
        LLMClientMessage::system("You are a helpful AI assistant.".to_string()),
        LLMClientMessage::user("Hello! How can you help me today?".to_string()),
    ];
    dbg!(&messages);

    // Create the completion request
    let request = LLMClientCompletionRequest::new(
        LLMType::O3MiniHigh,
        messages,
        0.7, // temperature
        None, // frequency penalty
    );
    dbg!("Request created");

    // Example access token (replace with actual token in production)
    let api_key = LLMProviderAPIKeys::CodeStory(CodestoryAccessToken::new(
        std::env::var("CODESTORY_API_KEY").unwrap_or_else(|_| {
            dbg!("Warning: CODESTORY_API_KEY not set, using placeholder");
            "your_access_token".to_string()
        }),
    ));
    dbg!("API key configured");

    // Create channel for receiving completion responses
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    dbg!("Channel created");

    // Stream completion in a separate task
    dbg!("Starting completion task");
    let completion_task = tokio::spawn(async move {
        let metadata = HashMap::new();
        broker
            .stream_completion(api_key, request, provider, metadata, sender)
            .await
    });

    // Process streaming responses
    dbg!("Processing streaming responses");
    while let Some(response) = receiver.recv().await {
        match response.delta() {
            Some(delta) => {
                dbg!(&delta);
                print!("{}", delta);
            }
            None => {
                eprintln!("Received response with no delta");
            }
        }
    }

    // Wait for completion task and get final result
    match completion_task.await.map_err(|e| Box::new(e) as Box<dyn Error>)? {
        Ok(result) => {
            dbg!("Completion task finished successfully");
            println!("\nFinal response: {}", result.answer_up_until_now());
        }
        Err(e) => {
            let err_string = format!("Completion failed: {}", e);
            dbg!(&err_string);
            eprintln!("Error: {}", err_string);
            return Err(Box::new(e) as Box<dyn Error>);
        }
    }

    Ok(())
}