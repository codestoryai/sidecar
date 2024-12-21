//! We create and manage the active session the user is working with alongside
//! the agents
//!
//! This keeps track of all the different type of edits which we are going to be
//! working on top of

pub(crate) mod anthropic_computer_editor;
pub mod ask_followup_question;
pub mod attempt_completion;
pub(crate) mod chat;
pub(crate) mod exchange;
pub(crate) mod hot_streak;
pub mod service;
pub mod session;
pub mod tool_use_agent;
