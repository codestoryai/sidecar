//! The selector is the module which selectes the best node to use in a given search
//! tree

use core::f32;

use crate::{agentic::tool::r#type::ToolType, mcts::action_node::SearchTree};

use super::uct_score::UCTScore;

use serde::{Deserialize, Serialize};

/// Parameters for configuring the behavior of the Selector in UCT score calculations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Selector {
    /// Weight factor for the exploitation term in the UCT score calculation.
    /// Higher values favor exploitation over exploration.
    exploitation_weight: f32,

    /// If true, uses the average reward across the trajectory for exploitation
    /// calculation instead of the node reward.
    use_average_reward: bool,

    /// Weight factor for the exploration term in the UCT score calculation.
    /// Higher values encourage more exploration of less-visited nodes.
    exploration_weight: f32,

    /// Weight factor for the depth-based components in the UCT score.
    /// Affects both the depth bonus and penalty calculations.
    depth_weight: f32,

    /// Factor used in calculating the depth bonus.
    /// Higher values increase the bonus for exploring deeper nodes, especially near the root.
    depth_bonus_factor: f32,

    /// Threshold for considering a node's reward as "high value."
    /// Used in various bonus calculations.
    high_value_threshold: f32,

    /// Threshold for considering a node's reward as "low value."
    /// Used in various penalty calculations.
    low_value_threshold: f32,

    /// Threshold for considering a node's reward as "very high value."
    /// Used in the high value child penalty calculation.
    very_high_value_threshold: f32,

    /// Constant bonus applied to high-value leaf nodes to encourage their exploration.
    high_value_leaf_bonus_constant: f32,

    /// Constant used in calculating the bonus for high-value nodes with low-value children,
    /// encouraging "auto-correction."
    high_value_bad_children_bonus_constant: f32,

    /// Constant used in penalizing nodes with very high-value children
    /// to prevent over-exploitation of a single path.
    high_value_child_penalty_constant: f32,

    /// Penalty applied to nodes on a trajectory that has already finished with a high reward,
    /// discouraging revisiting completed paths.
    finished_trajectory_penalty: f32,

    /// Bonus applied to nodes expecting correction, prioritizing exploration of potential fix paths.
    expect_correction_bonus: f32,

    /// List of action types to check for when calculating the high-value bad children bonus.
    #[serde(skip)]
    check_for_bad_child_actions: Vec<ToolType>,

    /// Weight factor for the diversity bonus.
    /// Higher values increase the bonus for nodes with low similarity to other explored nodes.
    diversity_weight: f32,

    /// Constant used in penalizing nodes that have duplicate children.
    /// Penalty increases with each duplicate.
    duplicate_child_penalty_constant: f32,

    /// Constant used in penalizing nodes that have siblings with the same action name.
    duplicate_action_penalty_constant: f32,
}

impl Selector {
    pub fn new(
        exploitation_weight: f32,
        use_average_reward: bool,
        exploration_weight: f32,
        depth_weight: f32,
        depth_bonus_factor: f32,
        high_value_threshold: f32,
        low_value_threshold: f32,
        very_high_value_threshold: f32,
        high_value_leaf_bonus_constant: f32,
        high_value_bad_children_bonus_constant: f32,
        high_value_child_penalty_constant: f32,
        finished_trajectory_penalty: f32,
        expect_correction_bonus: f32,
        check_for_bad_child_actions: Vec<ToolType>,
        diversity_weight: f32,
        duplicate_child_penalty_constant: f32,
        duplicate_action_penalty_constant: f32,
    ) -> Self {
        Self {
            exploitation_weight,
            use_average_reward,
            expect_correction_bonus,
            exploration_weight,
            depth_bonus_factor,
            depth_weight,
            diversity_weight,
            high_value_bad_children_bonus_constant,
            high_value_child_penalty_constant,
            high_value_leaf_bonus_constant,
            high_value_threshold,
            very_high_value_threshold,
            low_value_threshold,
            finished_trajectory_penalty,
            check_for_bad_child_actions,
            duplicate_action_penalty_constant,
            duplicate_child_penalty_constant,
        }
    }
    /// Calculate the exploitation component of the UCT score.
    ///
    /// Purpose: Favors nodes with higher rewards, encouraging the algorithm to exploit
    /// known good paths in the search tree.
    pub fn calculate_exploitation(&self, node_index: usize, graph: &SearchTree) -> f32 {
        let reward = if self.use_average_reward {
            graph.calculate_mean_reward(node_index)
        } else {
            let node_present = graph.get_node(node_index);
            match node_present {
                Some(node) => node
                    .reward()
                    .map(|reward| reward.value() as f32)
                    .unwrap_or(0.0),
                None => 0.0,
            }
        };
        reward
    }

    /// Calculate the exploration component of the UCT score.
    /// Purpose: Encourages the exploration of less-visited nodes, ensuring a balance
    /// between exploitation and exploration in the search process.
    pub fn calculate_exploration(&self, node_index: usize, graph: &SearchTree) -> f32 {
        graph.calculate_exploration(node_index, self.exploration_weight)
    }

    /// Calculate the depth-based exploration bonus.
    /// Purpose: Provides an incentive to explore deeper into the search tree,
    /// particularly for nodes near the root, to encourage thorough exploration.
    pub fn calculate_depth_bonus(&self, node_index: usize, graph: &SearchTree) -> f32 {
        graph.calculate_depth_bonus(node_index, self.depth_bonus_factor, self.depth_weight)
    }

    /// Calculate the depth penalty for very deep nodes.
    /// Purpose: Discourages excessive depth in the search tree, preventing the
    /// algorithm from getting stuck in overly long paths.
    pub fn calculate_depth_penalty(&self, node_index: usize, graph: &SearchTree) -> f32 {
        graph.calculate_depth_penalty(node_index, self.depth_weight)
    }

    /// Calculate the bonus for nodes with high reward that expanded to low-reward nodes.
    ///
    /// Purpose: Acts as an "auto-correct" mechanism for promising nodes that led to poor
    /// outcomes, likely due to invalid actions (e.g., syntax errors from incorrect code changes).
    /// This bonus gives these nodes a second chance, allowing the algorithm to potentially
    /// recover from or find alternatives to invalid actions.
    ///
    /// The bonus is applied when:
    /// 1. The node has a high reward
    /// 2. It has exactly one child (indicating a single action was taken)
    /// 3. The child action is of a type we want to check (e.g., RequestCodeChange)
    /// 4. The child node has a low reward
    ///
    /// In such cases, we encourage revisiting this node to try different actions,
    /// potentially leading to better outcomes.
    pub fn calculate_high_value_bad_children_bonus(
        &self,
        node_index: usize,
        graph: &SearchTree,
    ) -> f32 {
        graph.calculate_high_value_bad_children_bonus(
            node_index,
            self.high_value_threshold,
            self.check_for_bad_child_actions.to_vec(),
            self.low_value_threshold,
            self.exploitation_weight,
        )
    }

    /// Calculate the bonus for not expanded nodes with high reward.
    ///
    /// Purpose: Encourages the exploration of promising leaf nodes, potentially
    /// leading to valuable new paths in the search tree.
    pub fn calculate_high_value_leaf_bonus(&self, node_index: usize, graph: &SearchTree) -> f32 {
        graph.calculate_high_value_leaf_bonus(
            node_index,
            self.high_value_threshold,
            self.high_value_leaf_bonus_constant,
        )
    }

    /// Calculate the penalty for nodes with a child with very high reward.
    ///
    /// Purpose: Discourages over-exploitation of a single high-value path, promoting
    /// exploration of alternative routes in the search tree.
    pub fn calculate_high_value_child_penalty(&self, node_index: usize, graph: &SearchTree) -> f32 {
        graph.calculate_high_value_child_penalty(
            node_index,
            self.very_high_value_threshold,
            self.high_value_child_penalty_constant,
        )
    }

    /// Calculate the bonus for nodes with low reward that haven't been expanded yet but have high reward parents or not rewarded parents.
    ///
    /// Purpose: Encourages exploration of nodes that might be undervalued due to their
    /// current low reward, especially if they have promising ancestors.
    pub fn calculate_high_value_parent_bonus(&self, node_index: usize, graph: &SearchTree) -> f32 {
        graph.calculate_high_value_parent_bonus(
            node_index,
            self.high_value_threshold,
            self.low_value_threshold,
            self.exploitation_weight,
        )
    }

    /// Calculate the penalty for nodes where there are changes and a child node was already finished with high reward.
    ///
    /// Purpose: Discourages revisiting paths that have already led to successful outcomes,
    /// promoting exploration of new areas in the search space.
    pub fn calculate_finished_trajectory_penalty(
        &self,
        node_index: usize,
        graph: &SearchTree,
    ) -> f32 {
        graph.calculate_finished_trajectory_penalty(node_index, self.finished_trajectory_penalty)
    }

    /// Calculate the bonus for nodes with a parent node that expect correction.
    ///
    /// Purpose: Prioritizes nodes that are marked as expecting correction (e.g., after
    /// a failed test run or an invalid search request). This bonus decreases rapidly
    /// as the parent node accumulates more children, encouraging exploration of less-visited
    /// correction paths.
    pub fn calculate_expect_correction_bonus(&self, node_index: usize, graph: &SearchTree) -> f32 {
        graph.calculate_expect_correction_bonus(node_index, self.expect_correction_bonus)
    }

    /// Calculate penalty for nodes that have children with duplicate action names.
    /// The penalty increases with each duplicate action.
    ///
    /// Purpose: Discourages selecting nodes whose children perform the same type of action
    /// multiple times, promoting more diverse action sequences.
    pub fn calculate_duplicate_action_penalty(&self, node_index: usize, graph: &SearchTree) -> f32 {
        graph.calculate_duplicate_action_penalty(node_index, self.duplicate_action_penalty_constant)
    }

    /// Calculate penalty for nodes that have duplicate children.
    /// The penalty increases with each duplicate child.
    ///
    /// Purpose: Discourages exploration of nodes that tend to generate duplicate states,
    /// as these are likely to be less productive paths in the search space.
    pub fn calculate_duplicate_child_penalty(&self, node_index: usize, graph: &SearchTree) -> f32 {
        graph.calculate_duplicate_child_penalty(node_index, self.duplicate_child_penalty_constant)
    }

    /// Compute the UCT score with additional bonuses and penalties based on node characteristics.
    ///
    /// This method combines various components to create a comprehensive score for node selection,
    /// balancing exploration and exploitation while considering node-specific factors.
    pub fn uct_score(&self, node_index: usize, graph: &SearchTree) -> UCTScore {
        let node_visits = graph.node_visits(node_index);
        if node_visits == 0.0 {
            return UCTScore::final_score(f32::MAX);
        }
        let exploitation = self.calculate_exploitation(node_index, graph);
        let exploration = self.calculate_exploration(node_index, graph);
        let depth_bonus = self.calculate_depth_bonus(node_index, graph);
        let depth_penalty = self.calculate_depth_penalty(node_index, graph);
        let high_value_leaf_node = self.calculate_high_value_leaf_bonus(node_index, graph);
        let high_value_bad_children_bonus =
            self.calculate_high_value_bad_children_bonus(node_index, graph);
        let high_value_child_penalty = self.calculate_high_value_child_penalty(node_index, graph);
        let high_value_parent_bonus = self.calculate_high_value_parent_bonus(node_index, graph);
        let finished_trajectory_penalty =
            self.calculate_finished_trajectory_penalty(node_index, graph);
        let expect_correction_bonus = self.calculate_expect_correction_bonus(node_index, graph);
        // TODO(skcd): We have to update the diversity score over here
        let diversity_bonus = 0.0;
        let duplicate_child_penalty = self.calculate_duplicate_child_penalty(node_index, graph);
        let duplicate_action_penalty = self.calculate_duplicate_action_penalty(node_index, graph);
        let final_score = exploitation + exploration + depth_bonus - depth_penalty
            + high_value_leaf_node
            + high_value_bad_children_bonus
            - high_value_child_penalty
            + high_value_parent_bonus
            - finished_trajectory_penalty
            + expect_correction_bonus
            + diversity_bonus
            - duplicate_child_penalty
            - duplicate_action_penalty;
        UCTScore::new(
            final_score,
            exploitation,
            exploration,
            depth_bonus,
            depth_penalty,
            high_value_leaf_node,
            high_value_bad_children_bonus,
            high_value_child_penalty,
            high_value_parent_bonus,
            finished_trajectory_penalty,
            expect_correction_bonus,
            diversity_bonus,
            duplicate_child_penalty,
            duplicate_action_penalty,
        )
    }
}
