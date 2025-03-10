use clap::{Parser, Subcommand};
use llm_client::{
    broker::LLMBroker,
    clients::types::LLMType,
    provider::{LLMProvider, LLMProviderAPIKeys, OpenAIProvider},
};
use sidecar::{
    agentic::symbol::identifier::LLMProperties,
    agentic::tool::broker::{ToolBroker, ToolBrokerConfiguration},
    agentic::tool::r#type::ToolType,
    agentic::tool::code_edit::models::broker::CodeEditBroker,
    chunking::editor_parsing::EditorParsing,
    chunking::languages::TSLanguageParsing,
    inline_completion::symbols_tracker::SymbolTrackerInline,
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::timeout;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List available tools
    Tools {
        #[clap(subcommand)]
        command: ToolsCommands,
    },
}

#[derive(Subcommand)]
enum ToolsCommands {
    /// List all available tools
    List {
        /// Optional timeout in seconds (default: 10)
        #[clap(short, long, default_value = "10")]
        timeout: u64,
    },
}

async fn initialize_tool_broker() -> Result<Arc<ToolBroker>, Box<dyn std::error::Error>> {
    // Initialize LLMBroker
    let llm_broker = Arc::new(LLMBroker::new().await?);
    
    // Initialize EditorParsing
    let editor_parsing = Arc::new(EditorParsing::default());
    
    // Initialize SymbolTrackerInline with EditorParsing
    let symbol_tracking = Arc::new(SymbolTrackerInline::new(editor_parsing.clone()));
    
    // Initialize TSLanguageParsing using init() method
    let language_broker = Arc::new(TSLanguageParsing::init());
    
    // Initialize CodeEditBroker
    let code_edit_broker = Arc::new(CodeEditBroker::new());
    
    // Configure ToolBroker
    let tool_broker_config = ToolBrokerConfiguration::new(
        None, // No editor agent
        false, // Don't apply edits directly
    );
    
    // Create fallover LLM properties with required parameters
    let fail_over_llm = LLMProperties::new(
        LLMType::Gpt4O, // Correct variant name
        LLMProvider::OpenAI, // Default provider
        LLMProviderAPIKeys::OpenAI(OpenAIProvider::new("".to_string())), // Empty API key string
    );
    
    // Initialize ToolBroker
    let tool_broker = ToolBroker::new(
        llm_broker,
        code_edit_broker,
        symbol_tracking,
        language_broker,
        tool_broker_config,
        fail_over_llm,
    ).await;
    
    Ok(Arc::new(tool_broker))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Tools { command }) => match command {
            ToolsCommands::List { timeout: timeout_seconds } => {
                println!("Initializing tool broker (timeout: {} seconds)...", timeout_seconds);
                let start_time = Instant::now();
                
                // Initialize the tool broker with a timeout
                let tool_broker_result = timeout(
                    Duration::from_secs(*timeout_seconds),
                    initialize_tool_broker()
                ).await;
                
                match tool_broker_result {
                    Ok(Ok(tool_broker)) => {
                        let elapsed = start_time.elapsed();
                        println!("Tool broker initialized in {:.2?}", elapsed);
                        
                        println!("\nAvailable tools:");
                        
                        // List built-in tools (we can't access the private 'tools' field directly)
                        // Instead, we'll list all possible ToolType variants
                        for tool_type in [
                            ToolType::CodeEditing,
                            ToolType::OpenFile,
                            ToolType::GoToDefinitions,
                            ToolType::GoToReferences,
                            ToolType::LSPDiagnostics,
                            ToolType::ReRank,
                            ToolType::FindCodeSnippets,
                            ToolType::RequestImportantSymbols,
                            ToolType::FindCodeSymbolsCodeBaseWide,
                            ToolType::UtilityCodeSymbolSearch,
                            ToolType::GrepInFile,
                            ToolType::GoToImplementations,
                            ToolType::FilterCodeSnippetsForEditing,
                            ToolType::FilterCodeSnippetsSingleSymbolForEditing,
                            ToolType::EditorApplyEdits,
                            ToolType::GetQuickFix,
                            ToolType::ApplyQuickFix,
                            ToolType::TerminalCommand,
                            ToolType::SearchFileContentWithRegex,
                            ToolType::ListFiles,
                            ToolType::AskFollowupQuestions,
                            ToolType::AttemptCompletion,
                            ToolType::FindFiles,
                            // Add more built-in tools as needed
                        ] {
                            println!("- {}", tool_type);
                        }
                        
                        // List MCP tools
                        for tool in tool_broker.mcp_tools.iter() {
                            match tool {
                                ToolType::McpTool(name) => println!("- {} (MCP Tool)", name),
                                _ => println!("- {}", tool),
                            }
                        }
                    },
                    Ok(Err(e)) => {
                        eprintln!("Error initializing tool broker: {}", e);
                        return Err(e);
                    },
                    Err(_) => {
                        eprintln!("Timeout: Tool broker initialization took longer than {} seconds", timeout_seconds);
                        return Err("Initialization timeout".into());
                    }
                }
                
                Ok(())
            }
        },
        None => {
            println!("No command specified. Use --help for usage information.");
            Ok(())
        }
    }
}