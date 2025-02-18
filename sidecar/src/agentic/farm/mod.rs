//! Agent Farm implementation for managing multiple agents and their tasks
//! This module provides functionality for:
//! - Managing a pool of agents
//! - Distributing tasks among agents
//! - Handling inter-agent communication
//! - Managing farm resources and state

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use super::memory::base::Memory;
use super::tool;

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub id: String,
    pub capabilities: Vec<String>,
    pub max_concurrent_tasks: usize,
}

#[derive(Debug)]
pub struct Task {
    pub id: String,
    pub description: String,
    pub requirements: Vec<String>,
    pub priority: u8,
    pub status: TaskStatus,
}

#[derive(Debug, Clone)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed(String),
}

pub struct Agent {
    pub config: AgentConfig,
    pub memory: Memory,
    pub current_tasks: Vec<Task>,
    pub task_tx: mpsc::Sender<Task>,
}

pub struct AgentFarm {
    agents: HashMap<String, Arc<Mutex<Agent>>>,
    task_queue: VecDeque<Task>,
    max_agents: usize,
}

impl AgentFarm {
    pub fn new(max_agents: usize) -> Self {
        Self {
            agents: HashMap::new(),
            task_queue: VecDeque::new(),
            max_agents,
        }
    }

    pub fn add_agent(&mut self, config: AgentConfig) -> Result<(), String> {
        if self.agents.len() >= self.max_agents {
            return Err("Maximum number of agents reached".to_string());
        }

        let (task_tx, mut task_rx) = mpsc::channel(32);
        let agent = Agent {
            config: config.clone(),
            memory: Memory::default(),
            current_tasks: Vec::new(),
            task_tx,
        };

        self.agents.insert(config.id.clone(), Arc::new(Mutex::new(agent)));
        Ok(())
    }

    pub fn submit_task(&mut self, task: Task) {
        self.task_queue.push_back(task);
        self.distribute_tasks();
    }

    fn distribute_tasks(&mut self) {
        while let Some(task) = self.task_queue.pop_front() {
            if let Some(agent) = self.find_suitable_agent(&task) {
                if let Ok(mut agent) = agent.lock() {
                    if agent.current_tasks.len() < agent.config.max_concurrent_tasks {
                        agent.current_tasks.push(task.clone());
                        let _ = agent.task_tx.try_send(task);
                    } else {
                        self.task_queue.push_front(task);
                        break;
                    }
                }
            } else {
                self.task_queue.push_front(task);
                break;
            }
        }
    }

    fn find_suitable_agent(&self, task: &Task) -> Option<Arc<Mutex<Agent>>> {
        self.agents.values()
            .find(|agent| {
                if let Ok(agent) = agent.lock() {
                    agent.config.capabilities.iter()
                        .any(|cap| task.requirements.contains(cap))
                    && agent.current_tasks.len() < agent.config.max_concurrent_tasks
                } else {
                    false
                }
            })
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_farm_state_management() {
        let tool_box = Arc::new(ToolBox::default());
        let (mut farm, state_tx) = AgentFarm::new(5, tool_box);
        
        let config = AgentConfig {
            id: "agent1".to_string(),
            capabilities: vec!["rust".to_string()],
            max_concurrent_tasks: 3,
        };

        assert!(farm.add_agent(config, state_tx).await.is_ok());
        
        // Add more state management tests
    }
}