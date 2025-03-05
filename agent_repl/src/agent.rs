use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use anyhow::Result;
use log::{debug, error, info, warn};

use crate::models::{AgentAction, AgentResponse, TokenUsage, ToolType};
use crate::tools;

/// Represents the state of the agent
pub struct AgentState {
    repo_path: Option<PathBuf>,
    api_key: Option<String>,
    running: bool,
    files_opened: HashSet<PathBuf>,
    files_edited: HashSet<PathBuf>,
    token_usage: TokenUsage,
    current_tool: Option<ToolType>,
    current_query: Option<String>,
    feedback: Vec<String>,
}

impl AgentState {
    /// Create a new AgentState
    pub fn new() -> Self {
        Self {
            repo_path: None,
            api_key: None,
            running: false,
            files_opened: HashSet::new(),
            files_edited: HashSet::new(),
            token_usage: TokenUsage::default(),
            current_tool: None,
            current_query: None,
            feedback: Vec::new(),
        }
    }
    
    /// Set the repository path
    pub fn set_repo_path(&mut self, path: PathBuf) {
        self.repo_path = Some(path);
    }
    
    /// Get the repository path
    pub fn repo_path(&self) -> Option<&PathBuf> {
        self.repo_path.as_ref()
    }
    
    /// Set the API key
    pub fn set_api_key(&mut self, api_key: String) {
        self.api_key = Some(api_key);
    }
    
    /// Get the API key
    pub fn api_key(&self) -> Option<&String> {
        self.api_key.as_ref()
    }
    
    /// Start the agent with the given query
    pub fn start_agent(&mut self, query: String) {
        self.running = true;
        self.current_query = Some(query);
        self.current_tool = None;
        // Don't reset files_opened, files_edited, or token_usage
        // so we can track them across multiple runs
    }
    
    /// Stop the agent
    pub fn stop_agent(&mut self) {
        self.running = false;
    }
    
    /// Check if the agent is running
    pub fn is_running(&self) -> bool {
        self.running
    }
    
    /// Add a file to the list of opened files
    pub fn add_file_opened(&mut self, path: PathBuf) {
        self.files_opened.insert(path);
    }
    
    /// Get the list of opened files
    pub fn files_opened(&self) -> &HashSet<PathBuf> {
        &self.files_opened
    }
    
    /// Add a file to the list of edited files
    pub fn add_file_edited(&mut self, path: PathBuf) {
        self.files_edited.insert(path);
    }
    
    /// Get the list of edited files
    pub fn files_edited(&self) -> &HashSet<PathBuf> {
        &self.files_edited
    }
    
    /// Add token usage
    pub fn add_token_usage(&mut self, usage: TokenUsage) {
        self.token_usage.add(usage);
    }
    
    /// Get the token usage
    pub fn token_usage(&self) -> &TokenUsage {
        &self.token_usage
    }
    
    /// Set the current tool
    pub fn set_current_tool(&mut self, tool: ToolType) {
        self.current_tool = Some(tool);
    }
    
    /// Get the current tool
    pub fn current_tool(&self) -> Option<&ToolType> {
        self.current_tool.as_ref()
    }
    
    /// Get the current query
    pub fn current_query(&self) -> Option<&String> {
        self.current_query.as_ref()
    }
    
    /// Add feedback
    pub fn add_feedback(&mut self, feedback: String) {
        self.feedback.push(feedback);
    }
    
    /// Get the feedback
    pub fn feedback(&self) -> &Vec<String> {
        &self.feedback
    }
    }

/// Run the agent loop
pub async fn run_agent_loop(
    agent_state: Arc<Mutex<AgentState>>,
    query: String,
    tx: mpsc::Sender<AgentResponse>,
) -> Result<()> {
    // Get the timeout duration
    let timeout_duration = {
        let state = agent_state.lock().unwrap();
        state.timeout_duration()
    };
    
    // Run the agent loop with a timeout
    match timeout(timeout_duration, run_agent_loop_inner(agent_state.clone(), query, tx.clone())).await {
        Ok(result) => result,
        Err(_) => {
            // Timeout occurred
            tx.send(AgentResponse::Error { message: format!("Agent timed out after {:?}", timeout_duration) }).await?;
            agent_state.lock().unwrap().stop_agent();
            Ok(())
        }
    }
}

/// Inner function to run the agent loop
async fn run_agent_loop_inner(
    agent_state: Arc<Mutex<AgentState>>,
    query: String,
    tx: mpsc::Sender<AgentResponse>,
) -> Result<()> {
    // First, send a token usage update
    let initial_token_usage = TokenUsage::new(100, 50);
    tx.send(AgentResponse::TokenUsage { usage: initial_token_usage }).await?;
    
    // Get the repository path
    let repo_path = {
        let state = agent_state.lock().unwrap();
        state.repo_path().cloned()
    };
    
    if let Some(repo_path) = repo_path {
        // Initialize the LLM broker if needed
        let llm_broker = {
            let mut state = agent_state.lock().unwrap();
            if state.llm_broker().is_none() {
                let broker = Arc::new(LLMBroker::new().await.map_err(|e| anyhow::anyhow!("Failed to initialize LLM broker: {}", e))?);
                state.set_llm_broker(broker.clone());
                broker
            } else {
                state.llm_broker().unwrap()
            }
        };
        
        // Simulate the agent loop
        
        // Step 1: List files in the repository
        let tool_type = ToolType::ListFiles;
        let thinking = "I need to understand the repository structure first. Let me list the files.".to_string();
        tx.send(AgentResponse::ToolUse { tool_type: tool_type.clone(), thinking }).await?;
        
        // Simulate tool execution
        tokio::time::sleep(Duration::from_millis(500)).await;
        
        // Get a list of files in the repository
        let result = tools::list_files(&repo_path, true)?;
        tx.send(AgentResponse::ToolResult { result }).await?;
        
        // Update token usage
        tx.send(AgentResponse::TokenUsage { usage: TokenUsage::new(200, 100) }).await?;
        
        // Step 2: Search for relevant files
        let tool_type = ToolType::SearchFileContentWithRegex;
        let thinking = "Now I need to find files that might be relevant to the query.".to_string();
        tx.send(AgentResponse::ToolUse { tool_type: tool_type.clone(), thinking }).await?;
        
        // Simulate tool execution
        tokio::time::sleep(Duration::from_millis(500)).await;
        
        // Search for files containing keywords from the query
        let result = tools::search_files(&repo_path, &query, None)?;
        tx.send(AgentResponse::ToolResult { result }).await?;
        
        // Update token usage
        tx.send(AgentResponse::TokenUsage { usage: TokenUsage::new(300, 150) }).await?;
        
        // Step 3: Read a file
        let tool_type = ToolType::OpenFile;
        let thinking = "Let me read one of the relevant files to understand its content.".to_string();
        tx.send(AgentResponse::ToolUse { tool_type: tool_type.clone(), thinking }).await?;
        
        // Simulate tool execution
        tokio::time::sleep(Duration::from_millis(500)).await;
        
        // Find a file to read (just use the first file in the repository for this simulation)
        let files = tools::list_files(&repo_path, false)?;
        let file_lines: Vec<&str> = files.lines().collect();
        
        if !file_lines.is_empty() {
            let file_path = PathBuf::from(file_lines[0]);
            let absolute_path = if file_path.is_absolute() {
                file_path.clone()
            } else {
                repo_path.join(&file_path)
            };
            
            // Read the file
            let result = tools::read_file(&absolute_path)?;
            tx.send(AgentResponse::FileOpened { path: absolute_path.clone() }).await?;
            tx.send(AgentResponse::ToolResult { result }).await?;
            
            // Update token usage
            tx.send(AgentResponse::TokenUsage { usage: TokenUsage::new(500, 200) }).await?;
            
            // Step 4: Edit a file
            let tool_type = ToolType::CodeEditing;
            let thinking = "Based on the query, I need to make some changes to this file.".to_string();
            tx.send(AgentResponse::ToolUse { tool_type: tool_type.clone(), thinking }).await?;
            
            // Simulate tool execution
            tokio::time::sleep(Duration::from_millis(500)).await;
            
            // Simulate editing the file
            let result = "File edited successfully. Added implementation for the requested feature.".to_string();
            tx.send(AgentResponse::FileEdited { path: absolute_path }).await?;
            tx.send(AgentResponse::ToolResult { result }).await?;
            
            // Update token usage
            tx.send(AgentResponse::TokenUsage { usage: TokenUsage::new(800, 300) }).await?;
        }
        
        // Step 5: Execute a command
        let tool_type = ToolType::TerminalCommand;
        let thinking = "Let me run a command to verify that the changes work as expected.".to_string();
        tx.send(AgentResponse::ToolUse { tool_type: tool_type.clone(), thinking }).await?;
        
        // Simulate tool execution
        tokio::time::sleep(Duration::from_millis(500)).await;
        
        // Simulate running a command
        let result = "Command executed successfully. Tests pass and the feature works as expected.".to_string();
        tx.send(AgentResponse::ToolResult { result }).await?;
        
        // Update token usage
        tx.send(AgentResponse::TokenUsage { usage: TokenUsage::new(900, 400) }).await?;
        
        // Step 6: Complete the task
        let tool_type = ToolType::AttemptCompletion;
        let thinking = "I've completed the requested task. Let me summarize what I did.".to_string();
        tx.send(AgentResponse::ToolUse { tool_type: tool_type.clone(), thinking }).await?;
        
        // Simulate tool execution
        tokio::time::sleep(Duration::from_millis(500)).await;
        
        // Simulate completion
        let message = format!("I've completed the task: {}\n\nI made the following changes:\n1. Identified relevant files\n2. Modified the code to implement the requested feature\n3. Verified that the changes work as expected", query);
        tx.send(AgentResponse::Completion { message }).await?;
        
        // Update token usage
        tx.send(AgentResponse::TokenUsage { usage: TokenUsage::new(1000, 500) }).await?;
    } else {
        // No repository path set
        tx.send(AgentResponse::Error { message: "Repository path not set".to_string() }).await?;
    }
    
    Ok(())
}