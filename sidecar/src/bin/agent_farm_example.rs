use std::sync::Arc;
use tokio;

use sidecar::agentic::{
    farm::{AgentConfig, AgentFarm, Task, TaskStatus},
    tool::ToolBox,
};

#[tokio::main]
async fn main() {
    // Initialize toolbox and farm
    let tool_box = Arc::new(ToolBox::default());
    let (mut farm, state_tx) = AgentFarm::new(5, tool_box);

    // Create agents with different capabilities
    let rust_agent = AgentConfig {
        id: "rust_agent".to_string(),
        capabilities: vec!["rust".to_string()],
        max_concurrent_tasks: 2,
    };

    let python_agent = AgentConfig {
        id: "python_agent".to_string(),
        capabilities: vec!["python".to_string()],
        max_concurrent_tasks: 2,
    };

    // Add agents to farm
    farm.add_agent(rust_agent, state_tx.clone()).await.unwrap();
    farm.add_agent(python_agent, state_tx).await.unwrap();

    // Create some example tasks
    let tasks = vec![
        Task {
            id: "task1".to_string(),
            description: "Implement a Rust function".to_string(),
            requirements: vec!["rust".to_string()],
            priority: 1,
            status: TaskStatus::Pending,
        },
        Task {
            id: "task2".to_string(),
            description: "Write a Python script".to_string(),
            requirements: vec!["python".to_string()],
            priority: 1,
            status: TaskStatus::Pending,
        },
    ];

    // Submit tasks
    for task in tasks {
        farm.submit_task(task);
    }

    // Process state updates
    loop {
        farm.process_state_updates().await;
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}