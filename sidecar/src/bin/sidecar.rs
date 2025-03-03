use clap::Command;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    info!("CodeStory Sidecar 🚀");
    
    let matches = Command::new("sidecar")
        .about("CodeStory Sidecar - AI-powered code assistant")
        .version(env!("CARGO_PKG_VERSION"))
        .subcommand(
            Command::new("tools")
                .about("Tools management")
                .subcommand(
                    Command::new("list")
                        .about("List available tools")
                )
        )
        .get_matches();
    
    match matches.subcommand() {
        Some(("tools", tools_matches)) => {
            match tools_matches.subcommand() {
                Some(("list", _)) => {
                    // List all available tools
                    println!("Available tools:");
                    for tool in get_available_tools() {
                        println!("- {}", tool);
                    }
                }
                _ => {
                    println!("Unknown tools subcommand. Try 'sidecar tools list'");
                }
            }
        }
        _ => {
            println!("Unknown command. Try 'sidecar tools list'");
        }
    }
    
    Ok(())
}

fn get_available_tools() -> Vec<String> {
    // Return a list of available tools
    // This is a placeholder implementation
    vec![
        "ListFiles".to_string(),
        "ReadFile".to_string(),
        "WriteFile".to_string(),
        "ExecuteCommand".to_string(),
        "SearchFiles".to_string(),
        "CodeEdit".to_string(),
        "RepoMap".to_string(),
    ]
}