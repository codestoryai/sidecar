pub mod agent;
pub mod agentic;
pub mod application;
pub mod chunking;
pub mod db;
pub mod file_analyser;
pub mod git;
pub mod in_line_agent;
pub mod inline_completion;
pub mod mcts;
pub mod repo;
pub mod repomap;
pub mod reporting;
pub mod reranking;
pub mod state;
pub mod tree_printer;
pub mod user_context;
pub mod webserver;

#[cfg(feature = "grpc")]
pub mod proto {
    tonic::include_proto!("agent_farm");
}

#[cfg(feature = "grpc")]
pub mod grpc;