//! The agents and humans can take various actions and interact with each other
//! We need to model this on some level
//! Some ideas:
//! - The agent can decide what tool to use
//! - Each new agent task is well scoped and defined and only certain agents can
//! do heavy work
//! - The human can interact with the agent(s) in any way as required
//! - There is some quantum of work which can be done by each agent, we define
//! this as action
//! - Each action will also have a memory state which will be used through-out the
//! execution of the action
//! - Each action can spawn other actions which are inter-connected together, allowing bigger changes
//! to happen
//! - Each action can either be stopped because of user-input somehow or be complete or failed or in-process or human has taken over
//! - Each action has a dependency on other actions, awaiting for their finish state to be reached
//! - There is an environment where all of this happens, we need to model this somehow
//! - The human can spawn off other agents or the agent (the big one can also spawn other agents as and when required)
//!
//!
//! Nomenclature (cause we keep things professional here, but everyone loves anime and I hate paying tech-debt)
//! agent == mecha
pub mod memory;
pub mod swe_bench;
pub mod symbol;
pub mod tool;

// There are tons of actions happening in the editor, some made by human, some made by AI
// There is the environment which is changing as well, and things are happening to it
// Then there are the agents which can take action(s) as and when required to accomplish a task, the goal of this agent is to always be on task
// We also have a memory of what the agent knows about, so it can gather more context or keep things in mind
// Then we have tools which can be used by either the agent or the human to do things
// If I wanted to have a run loop how would that look like?

// To me it feels like this:
// - environment is the core center piece here
// - we have agents and humans who can use tools to interact with the environment
// - each agent and human can interact with each other (should this also be a tool?)
// - agent and human have a working memory as well
// - End of day is the work togehter on the task and get it done
// - we might need to also lock in resources so human and AI do not override each others work
