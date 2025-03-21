use std::{
    collections::{HashMap, HashSet},
    process::Stdio,
    sync::Arc,
};

use color_eyre::owo_colors::OwoColorize;
use colored::Colorize;
use llm_client::{broker::LLMBroker, clients::types::LLMClientUsageStatistics};

use crate::{
    agentic::{
        symbol::{events::message_event::SymbolEventMessageProperties, tool_box::ToolBox},
        tool::{code_edit::code_editor::EditorCommand, input::ToolInputPartial, r#type::ToolType},
    },
    mcts::decider::decider::Decider,
    user_context::types::UserContext,
};

use super::{
    agent_settings::settings::AgentSettings,
    execution::inference::InferenceEngine,
    feedback::feedback::FeedbackGenerator,
    selector::selector::Selector,
    value_function::reward::{Reward, RewardGeneration},
};

use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, std::hash::Hash, std::cmp::PartialEq, std::cmp::Eq, Serialize, Deserialize,
)]
pub enum ActionObservationMetadataKey {
    FileContentUpdated(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActionObservation {
    message: String,
    summary: Option<String>,
    /// The thinking which lead to this action being taken
    thinking: Option<String>,
    terminal: bool,
    expect_correction: bool,
    #[serde(skip)]
    metadata: HashMap<ActionObservationMetadataKey, String>,
}

impl ActionObservation {
    pub fn errored(
        message: String,
        thinking: Option<String>,
        expect_correction: bool,
        terminal: bool,
    ) -> Self {
        Self {
            message,
            summary: None,
            thinking,
            terminal,
            expect_correction,
            metadata: Default::default(),
        }
    }

    pub fn new(message: String, summary: String, thinking: Option<String>, terminal: bool) -> Self {
        Self {
            message,
            summary: Some(summary),
            thinking,
            terminal,
            expect_correction: false,
            metadata: Default::default(),
        }
    }

    pub fn file_content_updated(mut self, fs_file_path: String, file_content: String) -> Self {
        self.metadata.insert(
            ActionObservationMetadataKey::FileContentUpdated(fs_file_path),
            file_content,
        );
        self
    }

    pub fn get_updated_file_content(&self) -> HashMap<String, String> {
        self.metadata
            .iter()
            .filter_map(|(key, value)| {
                let ActionObservationMetadataKey::FileContentUpdated(fs_file_path) = key;
                Some((fs_file_path.to_owned(), value.to_owned()))
            })
            .collect()
    }
}

impl ActionObservation {
    pub fn summary(&self) -> Option<String> {
        self.summary.clone()
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn expect_correction(&self) -> bool {
        self.expect_correction
    }

    pub fn thinking(&self) -> Option<String> {
        self.thinking.clone()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActionToolInputPartial {
    tool_use_id: String,
    tool_input_partial: ToolInputPartial,
}

impl ActionToolInputPartial {
    pub fn new(tool_use_id: String, tool_input_partial: ToolInputPartial) -> Self {
        Self {
            tool_use_id,
            tool_input_partial,
        }
    }

    pub fn tool_use_id(&self) -> &str {
        &self.tool_use_id
    }

    pub fn tool_input_partial(&self) -> &ToolInputPartial {
        &self.tool_input_partial
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ActionToolParameters {
    Errored(String),
    Tool(ActionToolInputPartial),
}

impl ActionToolParameters {
    pub fn errored(error_str: String) -> Self {
        Self::Errored(error_str)
    }

    /// Returns the context crunching summary if the action node carries that value
    pub fn context_crunching_summary(&self) -> Option<String> {
        match self {
            Self::Tool(ActionToolInputPartial {
                tool_use_id: _,
                tool_input_partial: ToolInputPartial::ContextCrunching(context_crunching),
            }) => Some(context_crunching.summary().to_owned()),
            _ => None,
        }
    }

    pub fn tool(tool_use_id: String, tool_input: ToolInputPartial) -> Self {
        Self::Tool(ActionToolInputPartial::new(tool_use_id, tool_input))
    }

    pub fn to_string(&self) -> String {
        match self {
            Self::Errored(error_string) => {
                format!("Failed to generate action. Error: {error_string}")
            }
            Self::Tool(tool_input_partial) => tool_input_partial.tool_input_partial.to_string(),
        }
    }

    pub fn to_tool_type(&self) -> Option<ToolType> {
        match self {
            Self::Errored(_) => None,
            Self::Tool(tool_input_partial) => {
                Some(tool_input_partial.tool_input_partial.to_tool_type())
            }
        }
    }
}

/// Defaults to true for the is_active_on_trajectory
fn is_active_on_trajectory() -> bool {
    true
}

/// how do we get the action nodes to be part of the llm inference where we can generate
/// more steps if required etc, thats the important bit here
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ActionNode {
    index: usize,
    action: Option<ActionToolParameters>,
    /// The tree dictates the control vector for the node
    feedback: Option<String>,
    is_duplicate: bool,
    reward: Option<Reward>,
    visits: u32,
    value: f32,
    max_expansions: usize,
    /// time taken for the action node
    time_taken_seconds: Option<f32>,
    // usage statistics
    llm_usage_statistics: Option<LLMClientUsageStatistics>,
    observation: Option<ActionObservation>,
    // this tracks the context associated with the current action node
    user_context: UserContext,
    // the message associated with the node
    message: Option<String>,
    // the reward value for the node
    reward_value: f32,
    // is still tracked on the active trajectory
    #[serde(default = "is_active_on_trajectory")]
    is_active_on_trajectory: bool,
}

impl ActionNode {
    pub fn new(index: usize, max_expansions: usize) -> Self {
        Self {
            index,
            action: None,
            feedback: None,
            is_duplicate: false,
            reward: None,
            visits: 0,
            value: 0.0,
            max_expansions,
            time_taken_seconds: None,
            llm_usage_statistics: None,
            observation: None,
            user_context: UserContext::default(),
            message: None,
            reward_value: 0.0,
            is_active_on_trajectory: true,
        }
    }

    pub fn reward(&self) -> Option<Reward> {
        self.reward.clone()
    }

    pub fn default_with_index(index: usize) -> Self {
        Self::new(index, 1)
    }

    pub fn get_llm_usage_statistics(&self) -> Option<LLMClientUsageStatistics> {
        self.llm_usage_statistics.clone()
    }

    pub fn set_llm_usage_statistics_maybe(
        &mut self,
        llm_usage_satatistics: Option<LLMClientUsageStatistics>,
    ) {
        self.llm_usage_statistics = llm_usage_satatistics;
    }

    /// Updates the time taken for the action node to finish
    pub fn set_time_taken_seconds(&mut self, time_taken_seconds: f32) {
        self.time_taken_seconds = Some(time_taken_seconds);
    }

    pub fn set_action_tools(mut self, tool_input_partial: ToolInputPartial) -> Self {
        self.action = Some(ActionToolParameters::Tool(ActionToolInputPartial {
            tool_use_id: "".to_owned(),
            tool_input_partial,
        }));
        self
    }

    pub fn set_action_error(mut self, tool_input_error: String) -> Self {
        self.action = Some(ActionToolParameters::Errored(tool_input_error));
        self
    }

    pub fn mark_as_terminal(&mut self) {
        if let Some(ref mut observation) = &mut self.observation {
            observation.terminal = true;
        }
    }

    pub fn add_observation_mut(&mut self, message: String) {
        self.observation = Some(ActionObservation::new(
            message.to_owned(),
            message.to_owned(),
            None,
            false,
        ));
    }

    pub fn add_observation(mut self, message: String) -> Self {
        self.observation = Some(ActionObservation::new(
            message.to_owned(),
            message.to_owned(),
            None,
            false,
        ));
        self
    }

    pub fn error_observation(mut self, message: String) -> Self {
        self.observation = Some(ActionObservation::errored(
            message.to_owned(),
            None,
            true,
            false,
        ));
        self
    }

    pub fn tool_type(&self) -> Option<ToolType> {
        self.action().map(|action| action.to_tool_type()).flatten()
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn observation(&self) -> Option<ActionObservation> {
        self.observation.clone()
    }

    pub fn action(&self) -> Option<ActionToolParameters> {
        self.action.clone()
    }

    pub fn set_message(mut self, message: String) -> Self {
        self.message = Some(message);
        self
    }

    pub fn message(&self) -> Option<String> {
        self.message.clone()
    }

    pub fn is_duplicate(&self) -> bool {
        self.is_duplicate
    }

    // TODO(skcd): Fix this and keep track of it properly
    fn has_git_path(&self) -> bool {
        false
    }

    /// Checks if the node is finished by looking for the attempt completion tool
    pub fn is_finished(&self) -> bool {
        if let Some(action) = self.action() {
            matches!(action.to_tool_type(), Some(ToolType::AttemptCompletion))
        } else {
            false
        }
    }

    fn is_terminal_observation(&self) -> bool {
        // do we have a terminal observation
        self.observation
            .as_ref()
            .map(|observation| observation.terminal)
            .unwrap_or_default()
    }

    fn reset(&mut self) {
        self.reward = None;
        self.visits = 0;
        self.value = 0.0;
        self.observation = None;
        self.is_duplicate = false;
        // TODO(skcd): disable the reseting of the feedback, we populate it from the top
        // maybe this is the wrong way to do this, the reset should not care
        // about what sets it up
        // self.feedback = None;
        self.action = None;
    }

    /// Generates th patch from the git(main) for the current node
    /// It includes all the changes which we have performed
    pub fn git_diff_from_main(&self) -> String {
        self.user_context
            .variables
            .to_vec()
            .into_iter()
            .filter(|variable| variable.is_file())
            .filter_map(|variable| {
                if let Some(git_patch) = variable.patch_from_root() {
                    Some(format!(
                        r#"Changes in {}:
{}"#,
                        variable.fs_file_path.to_owned(),
                        git_patch
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn user_context(&self) -> &UserContext {
        &self.user_context
    }

    pub fn feedback(&self) -> Option<String> {
        self.feedback.clone()
    }

    pub fn update_user_context(mut self, user_context: UserContext) -> Self {
        self.user_context = user_context;
        self
    }

    pub fn reward_value(&self) -> f32 {
        self.reward_value
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct SearchTreeMinimal {
    index_to_node: HashMap<usize, ActionNode>,
    node_to_children: HashMap<usize, Vec<usize>>,
    node_to_parent: HashMap<usize, usize>,
    repo_name: String,
    root_directory: String,
    max_finished_nodes: Option<usize>,
    reward_threshold: Option<f32>,
    max_search_try: Option<usize>,
    log_directory: String,
}

impl SearchTreeMinimal {
    pub fn from_action_nodes(
        action_nodes: &[ActionNode],
        repo_name: String,
        root_directory: String,
        log_directory: String,
    ) -> Self {
        let mut index_to_node = HashMap::new();
        let mut node_to_children = HashMap::new();
        let mut node_to_parent = HashMap::new();
        for (action_index, action_node) in action_nodes.into_iter().enumerate() {
            index_to_node.insert(action_index, action_node.clone());
        }
        for index in 0..(action_nodes.len() - 1) {
            node_to_children.insert(index, vec![index + 1]);
        }
        for index in 1..(action_nodes.len()) {
            node_to_parent.insert(index, index - 1);
        }
        SearchTreeMinimal {
            index_to_node,
            node_to_children,
            node_to_parent,
            repo_name,
            root_directory,
            max_finished_nodes: None,
            reward_threshold: None,
            max_search_try: None,
            log_directory,
        }
    }

    pub fn nodes(&self) -> impl Iterator<Item = &ActionNode> {
        self.index_to_node.values()
    }

    pub async fn save_serialised_graph(&self, log_dir: &str, request_id: &str) {
        let graph_serialised = match serde_json::to_string(&self) {
            Ok(serialized) => serialized,
            Err(err) => {
                eprintln!("mcts::select::Failed to serialize graph: {}", err);
                String::from("Failed to serialize graph")
            }
        };

        // Create directory if it doesn't exist
        if let Err(err) = tokio::fs::create_dir_all(log_dir).await {
            eprintln!("Failed to create log directory: {}", err);
            return;
        }

        let log_file_name = format!("{}/mcts-{}.json", log_dir, request_id);

        match tokio::fs::write(&log_file_name, graph_serialised).await {
            Ok(_) => {
                println!(
                    "mcts::action_node::save_serialised_graph::Saved graph to {}",
                    log_file_name
                );
            }
            Err(err) => eprintln!(
                "mcts::action_node::save_serialised_graph::Failed to save graph: {}",
                err
            ),
        }
    }
}

#[derive(Serialize, Clone)]
pub struct SearchTree {
    #[serde(serialize_with = "serialize_usize_map")]
    pub index_to_node: HashMap<usize, ActionNode>,
    #[serde(serialize_with = "serialize_usize_map")]
    node_to_children: HashMap<usize, Vec<usize>>,
    #[serde(serialize_with = "serialize_usize_map")]
    node_to_parent: HashMap<usize, usize>,
    /// the maximum expansions allowed
    max_expansions: usize,
    /// root index of the node which we are interested in
    root_node_index: usize,
    /// maximum depth the nodes can go to
    max_depth: u32,
    /// maximum iterations or actions we will run
    max_iterations: usize,
    /// the maximum finished nodes the tree can have
    max_finished_nodes: Option<usize>,
    /// The min reward threshold to consider before finishing
    reward_threshold: Option<f32>,
    /// The minimum number of finished nodes to consider before finishing
    min_finished_nodes: Option<usize>,
    /// Maximum number of times to try out the search
    max_search_try: Option<usize>,

    selector: Selector,
    #[serde(skip)]
    tools: Vec<ToolType>,
    // the working directory
    root_directory: String,
    // the LLM Client
    #[serde(skip)]
    llm_client: Arc<LLMBroker>,
    // repo-ref
    repo_name: String,
    /// Repository base commit hash
    repo_base_commit_hash: String,
    // The tool box
    #[serde(skip)]
    tool_box: Arc<ToolBox>,
    log_directory: String,
    /// Settings for the agent components
    agent_settings: AgentSettings,
}

impl SearchTree {
    pub fn new(
        max_expansions: usize,
        max_depth: u32,
        max_iterations: usize,
        max_finished_nodes: Option<usize>,
        reward_threshold: Option<f32>,
        min_finished_nodes: Option<usize>,
        max_search_try: Option<usize>,
        root_directory: String,
        repo_name: String,
        repo_base_commit_hash: String,
        problem_statement: String,
        selector: Selector,
        tools: Vec<ToolType>,
        tool_box: Arc<ToolBox>,
        llm_client: Arc<LLMBroker>,
        log_directory: String,
        agent_settings: AgentSettings,
    ) -> Self {
        let root_node = ActionNode::new(0, max_expansions).set_message(problem_statement);
        Self {
            index_to_node: vec![(0, root_node)].into_iter().collect(),
            node_to_children: Default::default(),
            node_to_parent: Default::default(),
            max_expansions,
            root_node_index: 0,
            max_depth,
            max_search_try,
            max_iterations,
            max_finished_nodes,
            reward_threshold,
            min_finished_nodes,
            repo_base_commit_hash,
            selector,
            tool_box,
            tools,
            root_directory,
            llm_client,
            repo_name,
            log_directory,
            agent_settings,
        }
    }

    pub fn from_minimal_tree(
        search_tree_minimal: SearchTreeMinimal,
        selector: Selector,
        llm_client: Arc<LLMBroker>,
        tool_box: Arc<ToolBox>,
        tools: Vec<ToolType>,
    ) -> Self {
        Self {
            agent_settings: AgentSettings::new(true, true),
            index_to_node: search_tree_minimal.index_to_node,
            node_to_children: search_tree_minimal.node_to_children,
            node_to_parent: search_tree_minimal.node_to_parent,
            max_expansions: 1,
            root_node_index: 0,
            max_depth: 40,
            max_iterations: 500,
            max_finished_nodes: search_tree_minimal.max_finished_nodes,
            reward_threshold: search_tree_minimal.reward_threshold,
            min_finished_nodes: search_tree_minimal.max_finished_nodes,
            max_search_try: search_tree_minimal.max_search_try,
            selector,
            tools,
            root_directory: search_tree_minimal.root_directory,
            repo_name: search_tree_minimal.repo_name,
            repo_base_commit_hash: "".to_owned(),
            log_directory: search_tree_minimal.log_directory,
            llm_client,
            tool_box,
        }
    }

    pub fn root(&self) -> Option<&ActionNode> {
        self.index_to_node.get(&self.root_node_index)
    }

    pub fn parent(&self, node: &ActionNode) -> Option<&ActionNode> {
        if let Some(parent_index) = self.node_to_parent.get(&node.index) {
            self.index_to_node.get(parent_index)
        } else {
            None
        }
    }

    pub fn repo_name(&self) -> String {
        self.repo_name.to_owned()
    }

    pub fn root_directory(&self) -> String {
        self.root_directory.to_owned()
    }

    pub fn llm_client(&self) -> Arc<LLMBroker> {
        self.llm_client.clone()
    }

    pub fn tools(&self) -> Vec<ToolType> {
        self.tools.to_vec()
    }

    pub fn tool_box(&self) -> Arc<ToolBox> {
        self.tool_box.clone()
    }

    fn add_node(&mut self, node_index: usize, node: ActionNode) {
        self.index_to_node.insert(node_index, node);
    }

    fn add_node_to_parent(&mut self, parent_index: usize, child_index: usize) {
        self.node_to_parent
            .entry(child_index)
            .or_insert_with(|| parent_index);
    }

    fn add_child(&mut self, parent_index: usize, child_index: usize) {
        self.node_to_children
            .entry(parent_index)
            .or_insert_with(Vec::new)
            .push(child_index);
    }

    fn get_new_node_index(&self) -> usize {
        self.index_to_node.len()
    }

    pub fn get_node_mut(&mut self, node_index: usize) -> Option<&mut ActionNode> {
        self.index_to_node.get_mut(&node_index)
    }

    pub fn get_node(&self, node_index: usize) -> Option<&ActionNode> {
        self.index_to_node.get(&node_index)
    }

    fn finished_nodes(&self) -> Vec<&ActionNode> {
        self.index_to_node
            .values()
            .into_iter()
            .filter_map(|node| match node.action() {
                Some(action) => match action.to_tool_type() {
                    Some(tool_type) => {
                        if tool_type == ToolType::AttemptCompletion {
                            Some(node)
                        } else {
                            None
                        }
                    }
                    None => None,
                },
                None => None,
            })
            .collect()
    }

    /// Detects if wee should be allowed to keep running search or we are done
    pub fn is_finished(&self) -> bool {
        if self.index_to_node.len() >= self.max_iterations {
            true
        } else {
            let finished_nodes = self.finished_nodes();
            let unique_finished_parent_nodes = finished_nodes
                .into_iter()
                .filter_map(|node| {
                    let parent = self.parent(node);
                    match parent {
                        Some(parent) => Some(parent),
                        None => None,
                    }
                })
                .collect::<Vec<_>>();

            let unique_finished_parent_node_ids = unique_finished_parent_nodes
                .iter()
                .map(|node| node.index())
                .collect::<HashSet<usize>>();

            // If we have more finished nodes
            if let Some(max_finished_nodes) = self.max_finished_nodes {
                if unique_finished_parent_node_ids.len() >= max_finished_nodes {
                    return true;
                }
            }

            // if we have reached our reward threshold
            if let Some(reward_threshold) = self.reward_threshold {
                if unique_finished_parent_nodes.iter().any(|node| {
                    if let Some(reward) = node.reward() {
                        if reward.value() as f32 >= reward_threshold {
                            if let Some(min_finished_nodes) = self.min_finished_nodes {
                                if unique_finished_parent_node_ids.len() >= min_finished_nodes {
                                    return true;
                                }
                            }
                        }
                        false
                    } else {
                        false
                    }
                }) {
                    return true;
                }
            }

            let expandable_nodes = self.expandable_node(self.root_node_index);
            if expandable_nodes.is_empty() {
                return true;
            }
            false
        }
    }

    fn children_indices(&self, node: &ActionNode) -> Option<Vec<usize>> {
        self.children(node)
            .map(|children| children.into_iter().map(|child| child.index).collect())
    }

    fn children<'a>(&'a self, node: &ActionNode) -> Option<impl Iterator<Item = &ActionNode> + 'a> {
        self.node_to_children
            .get(&node.index)
            .map(move |child_indices| {
                child_indices
                    .iter()
                    .filter_map(move |idx| self.index_to_node.get(idx))
            })
    }

    pub fn get_sibling_nodes(&self, node_index: usize) -> Vec<&ActionNode> {
        let node = self.get_node(node_index);
        if let None = node {
            return vec![];
        }
        let node = node.expect("if let None to hold");
        let parent = self.parent(node);
        if parent.is_none() {
            return vec![];
        }
        let parent = parent.expect("if let None to hold");

        // look at all the children of the parent and exclude the node we are at
        // to get the siblings for the current node
        self.children(parent)
            .map(|children| {
                children
                    .into_iter()
                    .filter(|child| child.index != node_index)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn check_if_tree_has_branching(&self) -> bool {
        // easiest way to check is if any node has multiple children
        // every node should have 0 or 1 child
        self.node_to_children
            .iter()
            .any(|(_, children)| children.len() > 1)
    }

    pub fn calculate_tree_reward(&self) -> f32 {
        let current_index = self.root_node_index;
        let mut rewards: Vec<f32> = vec![];
        let mut node = self.index_to_node.get(&current_index);
        while node.is_some() {
            let expected_node = node.expect("is_some to hold");
            rewards.push(if expected_node.visits > 0 {
                expected_node.reward_value / (expected_node.visits as f32)
            } else {
                0.0
            });

            let mut children = self
                .children(expected_node)
                .map(|children| children.into_iter().collect::<Vec<_>>())
                .unwrap_or_default();
            if children.is_empty() {
                node = None;
            } else {
                // grab the first child
                node = Some(children.remove(0));
            }
        }

        if rewards.is_empty() {
            0.0
        } else {
            let rewards_len = rewards.len();
            let rewards_sum: f32 = rewards.into_iter().sum();
            rewards_sum / (rewards_len as f32)
        }
    }

    /// Creates the mean reward on the trajectory over here by traversing the tree
    pub fn calculate_mean_reward(&self, node_index: usize) -> f32 {
        let mut node = self.index_to_node.get(&node_index);
        let mut rewards: Vec<f32> = vec![];
        while node.is_some() {
            let expected_node = node.expect("is_some to hold");
            // add the reward
            rewards.push(if expected_node.visits > 0 {
                expected_node.value / (expected_node.visits as f32)
            } else {
                0.0
            });

            // move up the tree
            node = self.parent(expected_node);
        }

        if rewards.is_empty() {
            0.0
        } else {
            let rewards_len = rewards.len();
            let rewards_sum: f32 = rewards.into_iter().sum();
            rewards_sum / (rewards_len as f32)
        }
    }

    pub fn calculate_exploitation(&self, node_index: usize, exploitation_weigth: f32) -> f32 {
        // should we go for the average weight over here on the trajectory
        // or should we go for the absolute weight? open question
        if let Some(reward) = self
            .get_node(node_index)
            .map(|node| node.reward())
            .flatten()
        {
            reward.value() as f32 * exploitation_weigth
        } else {
            0.0
        }
    }

    pub fn calculate_exploration(&self, node_index: usize, exploration_weight: f32) -> f32 {
        // Retrieve the current node
        let node = self
            .get_node(node_index)
            .expect("Node index should be valid");

        // Retrieve the parent visits
        let parent_visits = if let Some(parent_node) = self.parent(node) {
            parent_node.visits as f32
        } else {
            1.0 // Default to 1.0 if there's no parent
        };

        // Retrieve the current node's visits
        let node_visits = node.visits as f32;

        if node_visits == 0.0 {
            f32::INFINITY // Favor exploration of unvisited nodes
        } else {
            exploration_weight * ((parent_visits.ln() / node_visits).sqrt())
        }
    }

    pub fn get_depth(&self, node_index: usize) -> u32 {
        let mut depth = 0;
        let mut node = self.get_node(node_index);
        while node.is_some() {
            let expected_node = node.expect("is_some to hold");
            node = self.parent(expected_node);
            if node.is_some() {
                depth = depth + 1;
            }
        }
        depth
    }

    pub fn calculate_depth_bonus(
        &self,
        node_index: usize,
        depth_bonus_factor: f32,
        depth_weight: f32,
    ) -> f32 {
        // Get the depth of the current node
        let depth = self.get_depth(node_index) as f32;

        // Calculate the depth-based bonus
        if depth == 0.0 {
            depth_bonus_factor * (-depth_weight * (depth - 1.0)).exp()
        } else {
            0.0
        }
    }

    pub fn calculate_depth_penalty(&self, node_index: usize, depth_weight: f32) -> f32 {
        let depth = self.get_depth(node_index) as f32;
        depth_weight * depth.sqrt() * 1.0
    }

    pub fn calculate_high_value_leaf_bonus(
        &self,
        node_index: usize,
        high_value_threshold: f32,
        high_value_leaf_bonus_constant: f32,
    ) -> f32 {
        let node = self.get_node(node_index);
        if let None = node {
            return 0.0;
        }
        let node = node.expect("if let None to hold");
        let children = self.children(node);
        let children = children
            .map(|child_iterator| child_iterator.into_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        if !children.is_empty() {
            if let Some(reward) = node.reward() {
                if reward.value() as f32 >= high_value_threshold {
                    return high_value_leaf_bonus_constant;
                }
            }
        }
        0.0
    }

    pub fn calculate_high_value_bad_children_bonus(
        &self,
        node_index: usize,
        high_value_threshold: f32,
        bad_child_actions: Vec<ToolType>,
        low_value_threshold: f32,
        exploitation_weight: f32,
    ) -> f32 {
        let node = self.get_node(node_index);
        if let None = node {
            return 0.0;
        }
        let node = node.expect("if let None to hold");
        let exploitation = self.calculate_exploitation(node_index, exploitation_weight);
        let node_children = self.children(node);
        let node_children = node_children
            .map(|children| children.into_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        // empty of no children
        if !node_children.is_empty() && exploitation >= high_value_threshold {
            let child_rewards = node_children
                .to_vec()
                .into_iter()
                .filter_map(|child| child.reward())
                .map(|reward| reward.value())
                .collect::<Vec<_>>();

            // if there is a single child with a reward value then we also check
            // if the action we took on this node was a one worth checking
            if child_rewards.len() == 1
                && node_children
                    .to_vec()
                    .into_iter()
                    .filter_map(|child| child.action.clone())
                    .filter_map(|tool_parameters| tool_parameters.to_tool_type())
                    .any(|tool_type| {
                        bad_child_actions
                            .to_vec()
                            .into_iter()
                            .any(|bad_child_tool| bad_child_tool == tool_type)
                    })
            {
                let child_rewards_len = child_rewards.len();
                let average_child_reward_value = (1.0
                    * child_rewards.into_iter().sum::<i32>() as f32)
                    / (1.0 * child_rewards_len as f32);

                // this is an approximation to how much value we can give back
                // the 5 here is sus but I presume it comes from the expansion factor
                if average_child_reward_value <= low_value_threshold {
                    return (exploitation - average_child_reward_value) * 5.0;
                }
            }
        }
        return 0.0;
    }

    pub fn calculate_high_value_child_penalty(
        &self,
        node_index: usize,
        very_high_value_threshold: f32,
        high_value_child_penalty_constant: f32,
    ) -> f32 {
        let node = self.get_node(node_index);
        if let None = node {
            return 0.0;
        }
        let node = node.expect("if let None to hold");
        let node_children = self.children(node);
        let node_children = node_children
            .map(|children| children.into_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        if !node_children.is_empty() {
            let child_rewards = node_children
                .into_iter()
                .filter_map(|child| child.reward())
                .map(|reward| reward.value())
                .collect::<Vec<_>>();

            let maximum_child_reward = child_rewards.into_iter().max();
            if let Some(maximum_child_reward) = maximum_child_reward {
                if maximum_child_reward as f32 >= very_high_value_threshold {
                    return high_value_child_penalty_constant;
                }
            }
        }
        return 0.0;
    }

    pub fn calculate_high_value_parent_bonus(
        &self,
        node_index: usize,
        high_value_threshold: f32,
        low_value_threshold: f32,
        exploitation_weight: f32,
    ) -> f32 {
        let node = self.get_node(node_index);
        if let None = node {
            return 0.0;
        }
        let node = node.expect("if let None to hold");
        let node_children = self.children(node);
        let node_children = node_children
            .map(|children| children.into_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        let exploration = self.calculate_exploitation(node_index, exploitation_weight);
        if !node_children.is_empty() {
            let parent_node = self.parent(node);
            if let Some(parent) = parent_node {
                // if parent is not rewarded yet or if the reward is higher than the
                // threshold we have
                if parent
                    .reward()
                    .map(|reward| reward.value() as f32 >= high_value_threshold)
                    .unwrap_or(true)
                {
                    if exploration <= low_value_threshold {
                        return high_value_threshold - exploration;
                    }
                }
            }
        }
        return 0.0;
    }

    pub fn calculate_finished_trajectory_penalty(
        &self,
        node_index: usize,
        finished_trajectory_penalty: f32,
    ) -> f32 {
        let node = self.get_node(node_index);
        if let None = node {
            return 0.0;
        }
        let node = node.expect("if let None to hold");
        if finished_trajectory_penalty != 0.0
            && node.has_git_path()
            && self.is_on_finished_trajectory(node_index, 100)
        {
            return finished_trajectory_penalty;
        }
        0.0
    }

    fn is_on_finished_trajectory(&self, node_index: usize, minimum_reward_threshold: i32) -> bool {
        let node = self.get_node(node_index);
        if let None = node {
            return false;
        }
        let node = node.expect("if let None to hold");
        let children = self
            .children(node)
            .map(|children| children.into_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for child in children.into_iter() {
            if child.is_finished()
                && child
                    .reward()
                    .map(|reward| reward.value() >= minimum_reward_threshold)
                    .unwrap_or(false)
            {
                return true;
            }

            if self.is_on_finished_trajectory(child.index, minimum_reward_threshold) {
                return true;
            }
        }
        false
    }

    pub fn calculate_expect_correction_bonus(
        &self,
        node_index: usize,
        expect_correction_bonus: f32,
    ) -> f32 {
        let node = self.get_node(node_index);
        if let None = node {
            return 0.0;
        }
        let node = node.expect("if let None to hold");
        let node_observation = &node.observation;
        if let Some(observation) = node_observation {
            let parent_node = self.parent(node);
            if let Some(parent_node) = parent_node {
                if observation.expect_correction
                    && parent_node
                        .observation
                        .as_ref()
                        .map(|observation| observation.expect_correction)
                        .unwrap_or_default()
                {
                    let children = self
                        .children(node)
                        .map(|children| children.into_iter().collect::<Vec<_>>().len())
                        .unwrap_or_default();
                    let delay_factor = 1.0 / (1.0 + children.pow(2) as f32);
                    return expect_correction_bonus * delay_factor;
                }
            }
        }
        return 0.0;
    }

    pub fn calculate_duplicate_action_penalty(
        &self,
        node_index: usize,
        duplicate_action_penalty_constant: f32,
    ) -> f32 {
        let node = self.get_node(node_index);
        if let None = node {
            return 0.0;
        }
        let node = node.expect("if let None to hold");
        let children: Vec<_> = self
            .children(node)
            .map(|children| children.collect())
            .unwrap_or_default();

        // how many times have we performed an action
        let mut action_times: HashMap<ToolType, usize> = Default::default();
        children.into_iter().for_each(|child| {
            let child_tool_type = child.action().map(|action| action.to_tool_type()).flatten();
            if let Some(tool_type) = child_tool_type {
                if let Some(action_times_taken) = action_times.get_mut(&tool_type) {
                    *action_times_taken = *action_times_taken + 1;
                } else {
                    action_times.insert(tool_type, 1);
                }
            }
        });

        let mut penalty = 0.0;
        action_times.values().for_each(|action_taken_times| {
            if *action_taken_times > 0 {
                penalty = penalty
                    + (action_taken_times - 1).pow(2) as f32 * duplicate_action_penalty_constant
            }
        });

        penalty
    }

    pub fn calculate_duplicate_child_penalty(
        &self,
        node_index: usize,
        duplicate_child_penalty_constant: f32,
    ) -> f32 {
        let node = self.get_node(node_index);
        if let None = node {
            return 0.0;
        }
        let node = node.expect("if let None to work");
        let children: Vec<_> = self
            .children(node)
            .map(|children| children.collect())
            .unwrap_or_default();
        let duplicate_children = children
            .into_iter()
            .filter(|children| children.is_duplicate())
            .collect::<Vec<_>>()
            .len();
        if duplicate_children > 0 {
            duplicate_child_penalty_constant * (duplicate_children.pow(2) as f32)
        } else {
            0.0
        }
    }

    /// How many times was the node visited
    pub fn node_visits(&self, node_index: usize) -> f32 {
        let node = self.get_node(node_index);
        if let None = node {
            return 0.0;
        }
        let node = node.expect("if let None to work");
        node.visits as f32
    }

    pub fn is_node_fully_expanded(&self, node_index: usize) -> bool {
        let node = self.get_node(node_index);
        // if node is not found, then we can't expand it
        if let None = node {
            return false;
        }
        let node = node.expect("if let None to hold");
        let children = self.children(node);
        let children_len = children
            .map(|children| children.into_iter().collect::<Vec<_>>())
            .unwrap_or_default()
            .len();
        children_len >= node.max_expansions
    }

    fn is_node_duplicate(&self, node_index: usize) -> bool {
        let node = self.get_node(node_index);
        // we return the worst case answer always, in case of checking for duplicates
        // that is true
        if let None = node {
            return true;
        }
        let node = node.expect("if let None to hold");
        node.is_duplicate
    }

    /// Recursively grabs all the expandable node starting from the root
    fn expandable_node(&self, node_index: usize) -> Vec<usize> {
        let mut expandable_node_indices = vec![];
        let node = self.get_node(node_index);
        if let None = node {
            return vec![];
        }
        let node = node.expect("if let None to hold");

        if !node.is_terminal_observation()
            && !self.is_node_fully_expanded(node_index)
            && !self.is_node_duplicate(node_index)
        {
            expandable_node_indices.push(node_index);
        }

        // now check for all the children
        let children = self.children_indices(node).unwrap_or_default();
        for child_index in children.into_iter() {
            expandable_node_indices.extend(self.expandable_node(child_index));
        }

        expandable_node_indices
    }

    /// Select the expandable nodes which are present in our search tree
    ///
    /// This only allows nodes to be selected within the max_depth limit
    /// and sorts the nodes by the UTC score
    pub fn select(&mut self) -> Option<usize> {
        let expandable_nodes = self.expandable_node(self.root_node_index);
        println!(
            "Selection phase::expandable nodes - {}",
            expandable_nodes
                .to_vec()
                .into_iter()
                .map(|node| node.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        let mut filtered_nodes = vec![];
        for expandable_node_index in expandable_nodes.into_iter() {
            let node = self.get_node(expandable_node_index);
            if let None = node {
                continue;
            }
            let node = node.expect("if let None to hold");
            let depth = self.get_depth(node.index);
            if depth < self.max_depth {
                filtered_nodes.push(node.index);
            }
        }

        if filtered_nodes.is_empty() {
            // so we're hitting this branch for first run, which causes us to break?
            return None;
        } else {
            // find the selector
            let mut filtered_node_to_score = filtered_nodes
                .into_iter()
                .map(|filtered_node_index| {
                    (
                        filtered_node_index,
                        self.selector.uct_score(filtered_node_index, &self),
                    )
                })
                .collect::<Vec<_>>();
            filtered_node_to_score.sort_by(|first_node, second_node| {
                first_node
                    .1
                    .get_final_score()
                    .total_cmp(&second_node.1.get_final_score())
            });
            filtered_node_to_score.reverse();
            // this will never panic because the array is not empty
            Some(filtered_node_to_score[0].0)
        }
    }

    pub fn expand<'a>(
        &'a mut self,
        node_index: usize,
        // This allows us to go beyond the child capacity of the current node
        allow_larger_child_expansions: bool,
    ) -> Option<usize> {
        let node = self.get_node(node_index);
        if let None = node {
            return None;
        }
        let node = node.expect("if let None to hold");
        let children_indices = self.children_indices(node).unwrap_or_default();
        let children_len = children_indices.len();
        for children_index in children_indices.into_iter() {
            let child_node = self.get_node(children_index);
            if let Some(child_node) = child_node {
                // the child is not executed so we grab it
                // and also not a duplicate since duplicates do not have observation
                if child_node.observation.is_none() && !child_node.is_duplicate {
                    return Some(child_node.index);
                }
            }
        }

        // we have already expanded beyond the limit
        if children_len >= self.max_expansions {
            if !allow_larger_child_expansions {
                // we are not allowed to go beyond our current children lenght
                // so fail hard
                return None;
            }
        }

        let child_node_index = self.get_new_node_index();

        let child_node = ActionNode::new(child_node_index, self.max_expansions)
            // transfer the user context properly
            .update_user_context(node.user_context.clone().copy_at_instance());
        // keep track of the child node
        self.add_node(child_node_index, child_node);
        // keep track of the edges
        self.add_child(node_index, child_node_index);
        // add the reverse edge
        self.add_node_to_parent(node_index, child_node_index);
        Some(child_node_index)
    }

    fn reset_children_for_node(&mut self, node_index: usize) {
        let node = self.get_node(node_index);
        if let None = node {
            return;
        }
        let node = node.expect("if let None to hold");
        let children_indices = self.children_indices(node).unwrap_or_default();
        // remove all the child edges node_to_childres
        self.node_to_children
            .get_mut(&node_index)
            .map(|children_indices| children_indices.clear());
        // remove all the parent edges node_to_parent
        children_indices.into_iter().for_each(|child_index| {
            self.node_to_parent.remove(&child_index);
        });
    }

    /// This gets the trajectory in order from the leaf to the root
    /// the direction here should be more apparant cause get_trajectory
    /// does not encapsulate that in the name over heer
    fn leaf_to_root(&self, node_index: usize) -> Vec<&ActionNode> {
        let node = self.get_node(node_index);
        if let None = node {
            vec![]
        } else {
            let node = node.expect("if let None to hold");
            let mut nodes = vec![node];
            let parent_node = self.parent(node);
            match parent_node {
                Some(parent_node) => {
                    nodes.extend(self.leaf_to_root(parent_node.index));
                    nodes
                }
                None => nodes,
            }
        }
    }

    /// is_duplicate checks the siblings to make sure that this is not a duplicate node
    pub fn is_duplicate(
        &self,
        current_node: &ActionNode,
        action_to_take: &ActionToolParameters,
    ) -> bool {
        let siblings = self.get_sibling_nodes(current_node.index);
        let is_duplicate = siblings.into_iter().any(|sibling| {
            if let Some(action) = sibling.action() {
                match (action, action_to_take) {
                    (
                        ActionToolParameters::Errored(first_error),
                        &ActionToolParameters::Errored(ref second_error),
                    ) => &first_error == second_error,
                    (
                        ActionToolParameters::Tool(first_tool_input_parameters),
                        &ActionToolParameters::Tool(ref second_tool_input_parameters),
                    ) => {
                        let first_tool_type = first_tool_input_parameters
                            .tool_input_partial()
                            .to_tool_type();
                        let second_tool_type = second_tool_input_parameters
                            .tool_input_partial()
                            .to_tool_type();
                        if first_tool_type != second_tool_type {
                            false
                        } else {
                            // now we can compare the tool input parameters
                            // since they do not have the thinking over here
                            first_tool_input_parameters.tool_input_partial().to_string()
                                == second_tool_input_parameters
                                    .tool_input_partial()
                                    .to_string()
                        }
                    }
                    _ => false,
                }
            } else {
                false
            }
        });

        is_duplicate
    }

    pub fn trajectory(&self, node_index: usize) -> Vec<&ActionNode> {
        let mut leaf_to_root = self.leaf_to_root(node_index);
        leaf_to_root.reverse();
        leaf_to_root
    }

    fn update_node(
        &mut self,
        node_index: usize,
        action_observation: Option<ActionObservation>,
        action_tool_parameters: ActionToolParameters,
        is_duplicate: bool,
    ) {
        let node = self.get_node_mut(node_index);
        if let None = node {
            return;
        }
        let node = node.expect("if let None to hold");
        node.is_duplicate = is_duplicate;
        node.action = Some(action_tool_parameters);
        node.observation = action_observation;
        // update the node content over here
        if let Some(observation) = node.observation() {
            let updated_file_content = observation.get_updated_file_content();
            println!(
                "update_node::node_file_content_updated::node_index({})::({})",
                node_index,
                updated_file_content
                    .keys()
                    .into_iter()
                    .map(|fs_file_path| fs_file_path.to_owned())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            updated_file_content
                .into_iter()
                .for_each(|(fs_file_path, updated_file_content)| {
                    // now we update the file content present in the user context
                    node.user_context
                        .update_file_content(&fs_file_path, &updated_file_content);
                })
        }
    }

    pub async fn run_node(
        &mut self,
        node_index: usize,
        is_duplicate_allowed: bool,
        message_properties: SymbolEventMessageProperties,
    ) {
        println!("Simulating node {}", node_index);
        {
            let node = self.get_node_mut(node_index);
            if let None = node {
                return;
            }
            let node = node.expect("if let None to hold");
            // reset the node
            node.reset();
            // reset the graph at this node as well
            self.reset_children_for_node(node_index);
        }

        // first we generate the message which we want to run inference for the
        // trajectory
        let nodes_trajectory = self.trajectory(node_index);

        let inference_engine = InferenceEngine::new(self.agent_settings.clone());
        // pick the next action we want to take over here
        // - execute the action
        // - add the observation to the node
        let inference_result = inference_engine
            .execute(
                nodes_trajectory,
                &self,
                is_duplicate_allowed,
                self.tool_box.clone(),
                message_properties.clone(),
            )
            .await;

        // - generate the value reward
        match inference_result {
            Err(e) => {
                println!("Node {} simulation failed: {}", node_index, e);
                return;
            }
            Ok(inference_result) => {
                println!("Node {} simulation complete", node_index);
                let action_observation = inference_result.action_observation();
                let observation_present = action_observation.is_some();
                let action_tool_parameters = inference_result.action_tool_parameters();
                let is_duplicate = inference_result.is_duplicate();
                self.update_node(
                    node_index,
                    action_observation,
                    action_tool_parameters,
                    is_duplicate,
                );

                // generate the reward
                if !is_duplicate && observation_present {
                    // TODO(skcd): Figure out the correct conditions for giving
                    // the reward over here
                    let nodes_trajectory = self.trajectory(node_index);
                    let reward = RewardGeneration::new()
                        .generate_reward(nodes_trajectory, &self, message_properties.clone())
                        .await;

                    println!("rewared_for_node:({node_index})::reward({:?})", &reward);

                    let node = self.get_node_mut(node_index);
                    if let Some(node) = node {
                        match reward {
                            Ok(reward) => {
                                node.reward = Some(reward);
                            }
                            Err(_e) => {
                                node.reward = None;
                            }
                        }
                    };
                }
            }
        }
    }

    pub fn backpropogate(&mut self, node_index: usize) {
        println!("Starting backpropagation from node {}", node_index);
        let node_reward = self
            .get_node(node_index)
            .map(|node| node.reward().clone())
            .flatten();
        if let None = node_reward {
            return;
        }
        let node_reward = node_reward.expect("if let None to hold");
        let reward_value = node_reward.value();
        let mut current_node_index = node_index;
        loop {
            let node_parent_index = {
                let node = self.get_node(current_node_index);
                if let Some(node) = node {
                    let parent = self.parent(node);
                    parent.map(|parent_node| parent_node.index())
                } else {
                    None
                }
            };
            let node = self.get_node_mut(current_node_index);
            if let Some(node) = node {
                node.reward_value = node.reward_value + reward_value as f32;
                node.visits = node.visits + 1;
                if let Some(parent_index) = node_parent_index {
                    current_node_index = parent_index
                } else {
                    // if we have no parent, we have reached the root so we are done
                    break;
                }
            } else {
                break;
            }
        }
    }

    async fn generate_feedback_for_node(
        &mut self,
        node_index: usize,
        message_properties: SymbolEventMessageProperties,
    ) {
        let nodes_trajectory = self.trajectory(node_index);
        let feedback = FeedbackGenerator::new(self.agent_settings.clone())
            .generate_feedback_for_node(nodes_trajectory, &self, message_properties)
            .await;
        if let Ok(Some(feedback)) = feedback {
            let node = self.get_node_mut(node_index);
            if let Some(node) = node {
                node.feedback = Some(feedback.feedback().to_owned());
            }
        }
    }

    /// Rests the file system to the current node since we are working on a lot
    /// of different nodes at the same time and jumping around
    async fn reset_file_system(&self, node_index: usize) {
        // - restore the file system to the original state over here
        // git add . && git stash
        // - apply the file content which this node has as part of the base_content in the variables
        // - profit
        let node = self.get_node(node_index);
        if let None = node {
            return;
        }
        let node = node.expect("if let None above to hold");

        // we run the git-command manually over here in the working directory for
        // the test repo
        let _output = tokio::process::Command::new("git")
            .args(&["add", "."])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(self.root_directory.to_owned())
            .output()
            .await
            .expect("to work");
        let _output = tokio::process::Command::new("git")
            .arg("stash")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(self.root_directory.to_owned())
            .output()
            .await
            .expect("to work");
        // also run git reset --hard to the base commit
        let _output = tokio::process::Command::new("git")
            .arg("reset")
            .arg("--hard")
            .arg(self.repo_base_commit_hash.to_owned())
            .current_dir(self.root_directory.to_owned())
            .output()
            .await
            .expect("to work");

        let git_diff_output = tokio::process::Command::new("git")
            .arg("diff")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(self.root_directory.to_owned())
            .output()
            .await;

        if let Ok(git_diff_output) = git_diff_output {
            println!(
                "post_rest::git_diff_output:({})::stderr({})",
                String::from_utf8(git_diff_output.stdout).expect("to work"),
                String::from_utf8(git_diff_output.stderr).expect("to work")
            );
        }

        // now update the file system to the current node
        for file_variable in node
            .user_context()
            .variables
            .iter()
            .filter(|variable| variable.is_file())
        {
            let current_content = file_variable.base_content();
            let fs_file_path = file_variable.fs_file_path.to_owned();
            let _ = tokio::fs::write(fs_file_path, current_content).await;
        }
    }

    pub async fn run_search(&mut self, message_properties: SymbolEventMessageProperties) {
        println!("\n=== Starting MCTS Search ===");
        let mut iteration = 0;

        // This is the main search loop here depending on how many times we want to try
        // out the search we can put this in a proper loop over and keep running based on top
        // of that
        let max_search_loops = if let Some(max_search_tries) = self.max_search_try {
            println!(
                "==== DETECTED SINGLE CHILD MODE, TRAJECTORIES TO GENERATE: {} ====",
                max_search_tries
            );
            max_search_tries
        } else {
            1
        };
        // we use this to pivot back to the root node and start again
        // this allows us to run multiple trajectories at the same time
        // and literally expand on the search space
        for traj_counter in 0..max_search_loops {
            loop {
                iteration += 1;
                println!("\n--- Traj: {}, Iteration {} ---", traj_counter, iteration);

                // Add tree visualization after each iteration
                self.print_tree(&mut vec![]);

                // change as necessary
                self.save_serialised_graph(
                    &self.log_directory,
                    &message_properties.root_request_id(),
                )
                .await;

                // only finish when the current iteration is finished
                // and the iteration is not equal to 1 which means we are inside
                // and deep into the tree
                if self.is_finished() && iteration != 1 {
                    println!("Search finished - termination condition met");
                    break;
                }

                // Selection phase
                let selected_node = if iteration == 1 {
                    // If this is the first iteration we have to alwys select
                    // the root node
                    Some(self.root_node_index)
                } else {
                    self.select()
                };
                if let Some(selected_index) = selected_node {
                    self.log_tree_state(selected_index, "Selected:");
                } else {
                    println!("No node selected - terminating search");
                    break;
                }

                // Expansion phase
                let is_maximum_exceed_allowed = iteration == 1; //  only allowed when we are at the root
                let new_node =
                    selected_node.and_then(|n| self.expand(n, is_maximum_exceed_allowed));
                if let Some(new_index) = new_node {
                    self.log_tree_state(new_index, "Expanded:");
                } else {
                    println!("No expansion possible - terminating search");
                    break;
                }

                let new_index = new_node.expect("Already checked above");

                // Reset and prepare node
                self.reset_file_system(new_index).await;

                self.generate_feedback_for_node(new_index, message_properties.clone())
                    .await;

                // Simulation
                let is_duplicate_allowed = iteration == 1; // only allowed duplicates for the start of the traj
                self.run_node(new_index, is_duplicate_allowed, message_properties.clone())
                    .await;
                self.log_tree_state(new_index, "After simulation:");

                // Backpropagation
                self.backpropogate(new_index);

                // change as necessary is saved over here
                self.save_serialised_graph(
                    &self.log_directory,
                    &message_properties.root_request_id(),
                )
                .await;

                // Log tree statistics
                println!(
                    "Tree state: {} total nodes, {} expandable",
                    self.index_to_node.len(),
                    self.expandable_node(self.root_node_index).len()
                );
            }
            println!("==== SEARCH TREE IS COMPLETE {} ===", traj_counter);
            // resetting iteration count here and starting again for the new traj
            println!("=== RESETTING ITERATION UNIT ===");
            iteration = 0;
        }
        println!("=== Search Complete ===\n");

        // Print final tree state
        self.print_tree(&mut vec![]);

        // save the tree again at the very end when everything has been updated
        self.save_serialised_graph(&self.log_directory, &message_properties.root_request_id())
            .await;

        println!("=== Deciding answer ===\n");
        let best_node = Decider::new()
            .decide(self.finished_nodes(), &self, message_properties.clone())
            .await;

        match best_node {
            Ok(best_node) => {
                // now update the state of the repo to this node and call it a day
                // we are done
                println!("Best answer selected: Node {}", best_node);
                self.reset_file_system(best_node).await;
            }
            Err(e) => {
                println!("Deciding answer failed: {}", e.to_string())
            }
        }
    }

    // Add this helper method for logging tree state
    fn log_tree_state(&self, node_index: usize, prefix: &str) {
        let node = self.get_node(node_index).unwrap();
        let depth = self.get_depth(node_index);
        let visits = node.visits;
        let reward = node.reward_value;

        println!(
            "{prefix} [Node {}] Depth: {}, Visits: {}, Reward: {}",
            node_index, depth, visits, reward
        );

        // Log action if present
        if let Some(action) = &node.action {
            println!("{prefix}   Action: {}", action.to_string());
        }
    }

    pub fn print_midwit_tree(&self, current_index: usize, steps: &mut Vec<String>) {
        // print it recursively to the same buffer?
        let node = self.get_node(current_index);
        if let None = node {
            return;
        }
        let expected_node = node.expect("if let None to hold");
        let node_information = format!(
            "## Action Taken:
{:?}

## Observation:
{:?}

## Reward:
{:?}",
            expected_node
                .action()
                .map(|action| format!("{:?}", action))
                .unwrap_or("Errored when getting action".to_owned()),
            expected_node
                .observation()
                .map(|observation| format!("{:?}", observation))
                .unwrap_or("Observation errored out".to_owned()),
            expected_node
                .reward()
                .map(|reward| format!("{:?}", reward))
                .unwrap_or("Reward not found".to_owned())
        );
        steps.push(node_information);
        let child = self.children(expected_node);
        let children_vec = child
            .map(|children| children.into_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        if children_vec.is_empty() {
            return;
        }
        self.print_midwit_tree(
            children_vec
                .get(0)
                .expect("is_empty to check for this")
                .index,
            steps,
        )
    }

    pub fn print_tree(&self, tree_output: &mut Vec<String>) {
        println!("MCTS Tree");
        tree_output.push("MCTS Tree".to_owned());
        self.print_node(self.root_node_index, "", true, tree_output);
    }

    fn print_node(
        &self,
        node_index: usize,
        prefix: &str,
        is_last: bool,
        tree_output: &mut Vec<String>,
    ) {
        let node = match self.get_node(node_index) {
            Some(n) => n,
            None => return,
        };

        // Build state parameters
        let mut state_params = Vec::new();
        if let Some(action) = &node.action {
            match action {
                ActionToolParameters::Errored(_err) => {
                    // Show errors in bold red
                    state_params.push("Error".bold().red().to_string());
                }
                ActionToolParameters::Tool(tool) => {
                    let tool_type = tool.tool_input_partial().to_tool_type();
                    let tool_str = match tool.tool_input_partial() {
                        ToolInputPartial::CodeEditorParameters(parameters) => {
                            // Unique colors for each EditorCommand
                            match &parameters.command {
                                EditorCommand::Create => {
                                    "str_replace_editor::create".blue().to_string()
                                }
                                EditorCommand::Insert => {
                                    "str_replace_editor::insert".yellow().to_string()
                                }
                                EditorCommand::StrReplace => {
                                    "str_replace_editor::str_replace".blue().to_string()
                                }
                                EditorCommand::UndoEdit => {
                                    "str_replace_editor::undo_edit".white().to_string()
                                }
                                EditorCommand::View => {
                                    "str_replace_editor::view".purple().to_string()
                                }
                            }
                        }
                        ToolInputPartial::FindFile(_) => {
                            tool_type.to_string().bright_yellow().to_string()
                        }
                        ToolInputPartial::CodeEditing(_) => {
                            tool_type.to_string().bright_purple().to_string()
                        }
                        ToolInputPartial::ListFiles(_) => {
                            tool_type.to_string().bright_yellow().to_string()
                        }
                        ToolInputPartial::SearchFileContentWithRegex(_) => {
                            tool_type.to_string().bright_purple().to_string()
                        }
                        ToolInputPartial::OpenFile(_) => {
                            tool_type.to_string().bright_magenta().to_string()
                        }
                        ToolInputPartial::SemanticSearch(_) => {
                            tool_type.to_string().bright_purple().to_string()
                        }
                        ToolInputPartial::LSPDiagnostics(_) => {
                            tool_type.to_string().bright_cyan().to_string()
                        }
                        ToolInputPartial::TerminalCommand(_) => {
                            tool_type.to_string().bright_red().to_string()
                        }
                        ToolInputPartial::AskFollowupQuestions(_) => {
                            tool_type.to_string().bright_white().to_string()
                        }
                        ToolInputPartial::AttemptCompletion(_) => {
                            tool_type.to_string().bright_green().to_string()
                        }
                        ToolInputPartial::RepoMapGeneration(_) => {
                            tool_type.to_string().magenta().to_string()
                        }
                        ToolInputPartial::TestRunner(_) => tool_type.to_string().red().to_string(),
                        ToolInputPartial::Reasoning(_) => {
                            tool_type.to_string().bright_blue().to_string()
                        }
                        ToolInputPartial::ContextCrunching(_) => {
                            tool_type.to_string().bright_blue().to_string()
                        }
                        ToolInputPartial::RequestScreenshot(_) => {
                            tool_type.to_string().bright_white().to_string()
                        }
                        ToolInputPartial::McpTool(_) => tool_type.to_string().cyan().to_string(),
                        ToolInputPartial::Thinking(_) => {
                            tool_type.to_string().bright_blue().to_string()
                        }
                    };
                    state_params.push(tool_str);
                }
            }

            if let Some(observation) = &node.observation {
                if observation.expect_correction {
                    state_params.push("expect_correction".to_string().red().to_string());
                }
            }
        }

        // Construct state_info
        let state_info = if !state_params.is_empty() {
            format!("Node ({})", state_params.join(", "))
            // format!("Node {} ({})", node_index, state_params.join(", "))
        } else {
            format!("Node ()")
            // format!("Node {} ()", node_index)
        };

        // Construct node_str based on reward
        let node_str = if let Some(reward) = &node.reward {
            format!("[{}]", reward.value())
        } else {
            format!("[-]")
        };

        // Reward string
        let reward_str = if let Some(reward) = &node.reward {
            format!("{}", reward.value())
        } else {
            "0".to_string()
        };

        // Decide which branch to draw
        let branch = if is_last { "└── " } else { "├── " };

        // Print the current node
        if node.is_duplicate {
            tree_output.push(format!(
                "{}{}{} {} (dup)",
                prefix, branch, node_str, state_info
            ));
            println!(
                "{}",
                format!("{}{}{} {} (dup)", prefix, branch, node_str, state_info).bright_red()
            );
        } else {
            tree_output.push(format!(
                "{}{}{} {} (ex: {}, re: {})",
                prefix,
                branch,
                node_str,
                state_info,
                self.children_indices(node)
                    .map(|child| child.len())
                    .unwrap_or_default(),
                reward_str,
            ));
            println!(
                "{}{}{} {} (ex: {}, re: {})",
                prefix,
                branch,
                node_str,
                state_info,
                self.children_indices(node)
                    .map(|child| child.len())
                    .unwrap_or_default(),
                reward_str,
            );
        }

        // Prepare prefix for child nodes
        let new_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });

        // Get children of the current node
        let children = self
            .node_to_children
            .get(&node_index)
            .cloned()
            .unwrap_or_default();

        let child_count = children.len();

        // Recursively print each child node
        for (i, child_index) in children.iter().enumerate() {
            let is_last_child = i == child_count - 1;
            self.print_node(*child_index, &new_prefix, is_last_child, tree_output);
        }
    }

    // Add this helper method for logging node_to_children
    fn _log_node_to_children(&self) {
        println!("Current node_to_children mapping:");
        for (parent_index, children_indices) in &self.node_to_children {
            println!("Node {}: {:?}", parent_index, children_indices);
        }
    }

    /// use to debug the graph
    fn _print_serialised_graph(&self) {
        let graph_serialised = match serde_json::to_string(&self) {
            Ok(serialized) => serialized,
            Err(err) => {
                eprintln!("mcts::select::Failed to serialize graph: {}", err);
                String::from("Failed to serialize graph")
            }
        };

        println!("{}", graph_serialised);
    }

    async fn save_serialised_graph(&self, log_dir: &str, request_id: &str) {
        let graph_serialised = match serde_json::to_string(&self) {
            Ok(serialized) => serialized,
            Err(err) => {
                eprintln!("mcts::select::Failed to serialize graph: {}", err);
                String::from("Failed to serialize graph")
            }
        };

        // Create directory if it doesn't exist
        if let Err(err) = tokio::fs::create_dir_all(log_dir).await {
            eprintln!("Failed to create log directory: {}", err);
            return;
        }

        let log_file_name = format!("{}/mcts-{}.json", log_dir, request_id);

        match tokio::fs::write(&log_file_name, graph_serialised).await {
            Ok(_) => {
                println!(
                    "mcts::action_node::save_serialised_graph::Saved graph to {}",
                    log_file_name
                );
            }
            Err(err) => eprintln!(
                "mcts::action_node::save_serialised_graph::Failed to save graph: {}",
                err
            ),
        }
    }
}

fn serialize_usize_map<S, T>(map: &HashMap<usize, T>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
    T: serde::Serialize,
{
    use serde::ser::SerializeMap;
    let mut map_serializer = serializer.serialize_map(Some(map.len()))?;
    for (k, v) in map {
        map_serializer.serialize_entry(&k.to_string(), v)?;
    }
    map_serializer.end()
}
