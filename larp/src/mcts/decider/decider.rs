//! decides which is the best trajectory to take from all the nodes present
//! on the MCTS tree

use llm_client::clients::types::LLMClientMessage;

use crate::{
    agentic::symbol::events::message_event::SymbolEventMessageProperties,
    mcts::action_node::{ActionNode, SearchTree},
};

use super::error::DeciderError;

/// Decider is used to decide the best node which we want to pick for our submission
pub struct Decider {}

impl Decider {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn decide(
        &self,
        nodes: Vec<&ActionNode>,
        search_tree: &SearchTree,
        message_properties: SymbolEventMessageProperties,
    ) -> Result<usize, DeciderError> {
        if nodes.is_empty() {
            return Err(DeciderError::NoNodesToCheck);
        }
        if !nodes.iter().any(|node| node.is_finished()) {
            return Err(DeciderError::NoCompletedNode);
        }

        // only keep the finished nodes over here
        let nodes = nodes
            .into_iter()
            .filter_map(|node| if node.is_finished() { Some(node) } else { None })
            .collect::<Vec<_>>();

        // create the message for comparing solutions
        let solutions_message =
            LLMClientMessage::user(self.generate_solution_message(nodes.to_vec())?);

        // get the problem statement
        let root_node = search_tree.root();
        if let None = root_node {
            return Err(DeciderError::NoRootNode);
        }
        let root_node = root_node.expect("if let None to hold");
        let message = root_node.message();
        if let None = message {
            return Err(DeciderError::NoRootMessageFound);
        }
        let problem_statement = message.expect("if let None to hold");
        let answer_node = self
            .select_best_answer(
                nodes,
                solutions_message,
                problem_statement,
                message_properties,
            )
            .await;

        answer_node.map(|node| node.index())
    }

    async fn select_best_answer<'a>(
        &'a self,
        mut nodes: Vec<&'a ActionNode>,
        _solutions_message: LLMClientMessage,
        _problem_statement: String,
        _message_properties: SymbolEventMessageProperties,
    ) -> Result<&'a ActionNode, DeciderError> {
        // be so dumb that people are surprised
        // select the node with the best reward

        // sort the nodes by the reward value
        nodes.sort_by(|first_node, second_node| {
            first_node
                .reward_value()
                .total_cmp(&second_node.reward_value())
        });
        // reverse them so they are in decreasing order
        nodes.reverse();
        // pick the first one which we get
        Ok(nodes.remove(0))
    }

    fn generate_solution_message(
        &self,
        finished_nodes: Vec<&ActionNode>,
    ) -> Result<String, DeciderError> {
        let solutions = finished_nodes
            .into_iter()
            .map(|finished_node| {
                let mut finished_solution = format!("<Solution id={}>", finished_node.index());

                // show the reward
                if let Some(reward) = finished_node.reward() {
                    finished_solution = format!(
                        r#"{}
<Explanation>
{}
</Explanation>
<Reward>
{}
</Reward>"#,
                        finished_solution,
                        reward.explanation(),
                        reward.value()
                    );
                }

                // now show the git-patch for this node
                finished_solution = format!(
                    r#"{finished_solution}
<Patch>
{}
</Patch>
</Solution>"#,
                    finished_node.git_diff_from_main()
                );
                finished_solution
            })
            .collect::<Vec<_>>();
        Ok(solutions.join("\n"))
    }
}
