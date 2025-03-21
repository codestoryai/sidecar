use std::collections::HashSet;
use std::path::PathBuf;

use super::graph::TagGraph;
use super::tag::{Tag, TagIndex};

pub struct TagAnalyzer<'a> {
    tag_index: &'a TagIndex,
    tag_graph: TagGraph,
}

impl<'a> TagAnalyzer<'a> {
    pub fn new(tag_index: &'a TagIndex) -> Self {
        let tag_graph = TagGraph::from_tag_index(tag_index, &HashSet::new());
        Self {
            tag_index,
            tag_graph,
        }
    }

    pub fn get_ranked_tags(&mut self) -> Vec<Tag> {
        self.tag_graph.calculate_and_distribute_ranks();

        let sorted_definitions = self.tag_graph.get_sorted_definitions();

        let graph = self.tag_graph.get_graph();

        let mut tags = vec![];

        for ((node, tag_name), _rank) in sorted_definitions {
            let path = PathBuf::from(&graph[*node]);
            if let Some(definition) = self.tag_index.definitions.get(&(path, tag_name.clone())) {
                tags.extend(definition.to_owned());
            }
        }

        tags
    }

    pub fn debug_print_ranked_tags(&mut self) {
        let ranked_tags = self.get_ranked_tags();
        for tag in &ranked_tags {
            println!("{:?}", tag);
        }
    }
}
