//! Task execution and state management for the agent farm

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

use super::{Agent, Task, TaskStatus};
use crate::agentic::tool::ToolBox;

pub struct TaskExecutor {
    agent: Arc<Mutex<Agent>>,
    tool_box: Arc<ToolBox>,
    task_rx: mpsc::Receiver<Task>,
    state_tx: mpsc::Sender<TaskState>,
}

#[derive(Debug, Clone)]
pub struct TaskState {
    pub task_id: String,
    pub agent_id: String,
    pub status: TaskStatus,
    pub result: Option<String>,
}

impl TaskExecutor {
    pub fn new(
        agent: Arc<Mutex<Agent>>,
        tool_box: Arc<ToolBox>,
        task_rx: mpsc::Receiver<Task>,
        state_tx: mpsc::Sender<TaskState>,
    ) -> Self {
        Self {
            agent,
            tool_box,
            task_rx,
            state_tx,
        }
    }

    pub async fn run(mut self) -> JoinHandle<()> {
        tokio::spawn(async move {
            while let Some(task) = self.task_rx.recv().await {
                let agent_id = {
                    let agent = self.agent.lock().await;
                    agent.config.id.clone()
                };

                let mut state = TaskState {
                    task_id: task.id.clone(),
                    agent_id,
                    status: TaskStatus::InProgress,
                    result: None,
                };

                // Notify task started
                let _ = self.state_tx.send(state.clone()).await;

                // Execute task using toolbox
                let result = self.execute_task(&task).await;

                // Update task state based on result
                state.status = match result {
                    Ok(output) => {
                        state.result = Some(output);
                        TaskStatus::Completed
                    }
                    Err(e) => TaskStatus::Failed(e),
                };

                // Notify task completion
                let _ = self.state_tx.send(state).await;

                // Update agent's task list
                if let Ok(mut agent) = self.agent.lock().await {
                    if let Some(pos) = agent.current_tasks.iter().position(|t| t.id == task.id) {
                        agent.current_tasks.remove(pos);
                    }
                }
            }
        })
    }

    async fn execute_task(&self, task: &Task) -> Result<String, String> {
        // Here we would use the toolbox to execute the task based on its requirements
        // For now, just simulate task execution
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        Ok(format!("Completed task: {}", task.description))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agentic::farm::{AgentConfig, Agent};

    #[tokio::test]
    async fn test_task_execution() {
        let config = AgentConfig {
            id: "test_agent".to_string(),
            capabilities: vec!["test".to_string()],
            max_concurrent_tasks: 1,
        };

        let (task_tx, task_rx) = mpsc::channel(32);
        let agent = Agent {
            config,
            memory: Default::default(),
            current_tasks: Vec::new(),
            task_tx,
        };

        let (state_tx, mut state_rx) = mpsc::channel(32);
        let tool_box = Arc::new(ToolBox::default());
        
        let executor = TaskExecutor::new(
            Arc::new(Mutex::new(agent)),
            tool_box,
            task_rx,
            state_tx,
        );

        let _handle = executor.run().await;

        // Test will be expanded with actual task execution
    }
}