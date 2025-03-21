use std::{collections::HashSet, path::PathBuf};

/// Checks if a path is a git directory, it looks for any commit hash present
/// and gets the timestamp for it as a poor-man's check
pub fn is_git_repository(dir: &PathBuf) -> bool {
    gix::open(dir)
        .ok()
        .map(|repo| {
            repo.head().ok().map(|mut head| {
                head.peel_to_commit_in_place()
                    .ok()
                    .map(|commit| commit.time().ok().map(|time| time.seconds))
            })
        })
        // all the flattens over here, F
        .flatten()
        .flatten()
        .flatten()
        .is_some()
}

// TODO(codestory): Improve the name over here
pub fn close_small_gaps_helper(
    lines: HashSet<usize>,
    code_split_by_lines: Vec<String>,
    code_len: usize,
) -> HashSet<usize> {
    // a "closing" operation on the integers in set.
    // if i and i+2 are in there but i+1 is not, I want to add i+1
    // Create a new set for the "closed" lines
    let mut closed_show = lines.clone();
    let mut sorted_show: Vec<usize> = lines.iter().cloned().collect();
    sorted_show.sort_unstable();

    for (i, &value) in sorted_show.iter().enumerate().take(sorted_show.len() - 1) {
        if sorted_show[i + 1] - value == 2 {
            closed_show.insert(value + 1);
        }
    }

    // pick up adjacent blank lines
    for (i, line) in code_split_by_lines.iter().enumerate() {
        if !closed_show.contains(&i) {
            continue;
        }

        // looking at the current line and if its not empty
        // and we are 2 lines above the end and the next line is empty
        if !line.trim().is_empty()
            && i < code_len - 2
            && code_split_by_lines[i + 1].trim().is_empty()
        {
            closed_show.insert(i + 1);
        }
    }

    let mut closed_closed_show = closed_show.clone().into_iter().collect::<Vec<_>>();
    closed_closed_show.sort_unstable();

    closed_show
}

#[cfg(test)]
mod tests {
    use super::close_small_gaps_helper;

    #[test]
    fn test_closing_gap_check() {
        let code = r#"use std::collections::HashSet;
use std::path::PathBuf;

use super::graph::TagGraph;
use super::tag::{Tag, TagIndex};

pub struct TagAnalyzer {
    tag_index: TagIndex,
    tag_graph: TagGraph,
}

impl TagAnalyzer {
    pub fn new(tag_index: TagIndex) -> Self {
        let tag_graph = TagGraph::from_tag_index(&tag_index, &HashSet::new());
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
}"#;
        let lines = vec![
            0, 1, 2, 6, 7, 8, 9, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
            27, 28, 29, 37, 45, 46,
        ];
        let code_len = code.lines().into_iter().collect::<Vec<_>>().len() + 1;
        let updated_lines = close_small_gaps_helper(
            lines.into_iter().collect(),
            code.lines()
                .into_iter()
                .map(|line| line.to_owned())
                .collect::<Vec<_>>(),
            code_len,
        );
        // we do not want line 38 since its empty
        assert!(!updated_lines.contains(&38));
    }
}
