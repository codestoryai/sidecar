use super::tag::TagIndex;
use petgraph::algo::page_rank::page_rank;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::prelude::EdgeIndex;
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;

pub type RankedDefinition = ((NodeIndex, String), f64);

pub type RankedDefinitionsMap = HashMap<(NodeIndex, String), f64>;

pub struct TagGraph {
    graph: DiGraph<String, f64>,
    node_indices: HashMap<String, NodeIndex>,
    edge_to_ident: HashMap<EdgeIndex, String>, // for rank distribution
    ranked_definitions: RankedDefinitionsMap,
    sorted_definitions: Vec<RankedDefinition>,
}

impl TagGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            node_indices: HashMap::new(),
            edge_to_ident: HashMap::new(),
            ranked_definitions: HashMap::new(),
            sorted_definitions: Vec::new(),
        }
    }

    pub fn get_graph(&self) -> &DiGraph<String, f64> {
        &self.graph
    }

    pub fn from_tag_index(tag_index: &TagIndex, mentioned_idents: &HashSet<String>) -> Self {
        let mut tag_graph = Self::new();
        tag_graph.populate_from_tag_index(tag_index, mentioned_idents);
        tag_graph
    }

    // TODO - petgraph does not support personalization vectors

    pub fn populate_from_tag_index(
        &mut self,
        tag_index: &TagIndex,
        mentioned_idents: &HashSet<String>,
    ) {
        for ident in &tag_index.common_tags {
            let mul = self.calculate_multiplier(ident, mentioned_idents);
            let num_refs = tag_index.references[ident].len() as f64;
            let scaled_refs = num_refs.sqrt();

            for referencer in &tag_index.references[ident] {
                for definer in &tag_index.defines[ident] {
                    let referencer_idx = self.get_or_create_node(referencer.to_str().unwrap());
                    let definer_idx = self.get_or_create_node(definer.to_str().unwrap());

                    let edge_index =
                        self.graph
                            .add_edge(referencer_idx, definer_idx, mul * scaled_refs);

                    self.edge_to_ident.insert(edge_index, ident.to_string());
                }
            }
        }
    }

    pub fn calculate_page_ranks(&self) -> Vec<f64> {
        let final_rank = page_rank(&self.graph, 0.85, 1); // for speed, 100 is Aider's default
        final_rank
    }

    pub fn get_ranked_definitions(&self) -> &RankedDefinitionsMap {
        &self.ranked_definitions
    }

    pub fn debug_ranked_definitions(&self) {
        for ((node_index, ident), v) in &self.ranked_definitions {
            println!("{:?}:{} -> {}", self.graph[*node_index], ident, v);
        }
    }

    pub fn debug_sorted_definitions(&self) {
        for ((node_index, ident), v) in &self.sorted_definitions {
            println!("{:?}:{} -> {}", self.graph[*node_index], ident, v);
        }
    }

    pub fn get_sorted_definitions(&self) -> &[RankedDefinition] {
        &self.sorted_definitions
    }

    pub fn calculate_and_distribute_ranks(&mut self) {
        let ranks = self.calculate_page_ranks();
        self.distribute_rank(&ranks);
        self.sort_by_rank();
    }

    fn sort_by_rank(&mut self) {
        let mut vec: Vec<_> = self.ranked_definitions.iter().collect();

        vec.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));

        self.sorted_definitions = vec.into_iter().map(|(k, v)| (k.clone(), *v)).collect();
    }

    fn distribute_rank(&mut self, ranked: &Vec<f64>) {
        for src in self.graph.node_indices() {
            let src_rank = ranked[src.index() as usize];

            let total_outgoing_weights: f64 =
                self.graph.edges(src).map(|edge| *edge.weight()).sum();

            for edge in self.graph.edges(src) {
                let destination = edge.target();
                let weight = *edge.weight();

                // Calculate the new weight for this edge
                // This distributes the source node's rank proportionally based on edge weights
                let new_weight = src_rank * weight / total_outgoing_weights;

                // Get the unique identifier for this edge
                let edge_index = edge.id();
                let ident = self
                    .edge_to_ident
                    .get(&edge_index)
                    .expect("edge_index should always exist in edge_to_ident");

                // Update the rank for the destination node and identifier combination
                // If it doesn't exist in the map, initialize it with 0.0
                // Then add the newly calculated weight to its current value
                *self
                    .ranked_definitions
                    .entry((destination, ident.clone()))
                    .or_insert(0.0) += new_weight;
            }
        }
    }

    pub fn generate_dot_representation(&self) -> String {
        let mut dot = String::new();
        writeln!(&mut dot, "digraph {{").unwrap();

        for node_index in self.graph.node_indices() {
            let node_label = &self.graph[node_index];
            writeln!(
                &mut dot,
                "    {:?} [ label = {:?} ]",
                node_index.index(),
                node_label
            )
            .unwrap();
        }

        for edge in self.graph.edge_references() {
            let (source, target) = (edge.source().index(), edge.target().index());
            let weight = edge.weight();
            writeln!(
                &mut dot,
                "    {:?} -> {:?} [ label = {:?} ]",
                source, target, weight
            )
            .unwrap();
        }

        writeln!(&mut dot, "}}").unwrap();
        dot
    }

    pub fn print_dot(&self) {
        println!("{}", self.generate_dot_representation());
    }

    fn get_or_create_node(&mut self, name: &str) -> NodeIndex {
        *self
            .node_indices
            .entry(name.to_string())
            .or_insert_with(|| self.graph.add_node(name.to_string()))
    }

    fn calculate_multiplier(&self, tag: &str, mentioned_idents: &HashSet<String>) -> f64 {
        if mentioned_idents.contains(tag) {
            10.0
        } else if tag.starts_with('_') {
            0.1
        } else {
            1.0
        }
    }
}
