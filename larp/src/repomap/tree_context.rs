use std::{
    cmp::{max, min},
    collections::HashSet,
};

use tree_sitter::{Node, TreeCursor};

use super::helpers::close_small_gaps_helper;

pub struct TreeContext<'a> {
    _filename: String,
    parent_context: bool,
    child_context: bool,
    last_line: bool,
    margin: usize,
    mark_lois: bool,
    header_max: usize,
    show_top_of_file_parent_scope: bool,
    loi_pad: usize,
    // config: TSLanguageConfig,
    lois: HashSet<usize>,
    show_lines: HashSet<usize>, // row numbers
    num_lines: usize,
    lines: Vec<String>,
    line_number: bool,
    done_parent_scopes: HashSet<usize>,
    nodes: Vec<Vec<Node<'a>>>,
    scopes: Vec<HashSet<usize>>, // the starting lines of the nodes that span the line
    header: Vec<Vec<(usize, usize, usize)>>, // the size, start line, end line of the nodes that span the line
}

impl<'a> TreeContext<'a> {
    pub fn new(code: String, fs_file_path: String) -> Self {
        // let ts_parsing = TSLanguageParsing::init();
        // let config = ts_parsing.for_file_path(&filename).unwrap().clone();
        let lines: Vec<String> = code.lines().map(|s| s.to_string()).collect();
        let num_lines = lines.len() + 1;
        Self {
            _filename: fs_file_path,
            parent_context: true,
            child_context: false,
            last_line: false,
            margin: 0,
            mark_lois: false,
            header_max: 10,
            show_top_of_file_parent_scope: false,
            loi_pad: 0,
            lois: HashSet::new(),
            show_lines: HashSet::new(),
            num_lines,
            lines,
            done_parent_scopes: HashSet::new(),
            scopes: vec![HashSet::new(); num_lines],
            header: vec![Vec::new(); num_lines],
            line_number: false,
            nodes: vec![vec![]; num_lines],
        }
    }

    pub fn init(&mut self, cursor: TreeCursor<'a>) {
        self.walk(cursor);
        self.arrange_headers();
    }

    // ✅
    pub fn walk(&mut self, mut cursor: TreeCursor<'a>) {
        // It is dfs in a way
        loop {
            let start_line = cursor.node().start_position().row;
            let end_line = cursor.node().end_position().row;
            let size = end_line - start_line;

            self.nodes[start_line].push(cursor.node());

            if size > 0 {
                self.header[start_line].push((size, start_line, end_line));
            }

            for i in start_line..=end_line {
                self.scopes[i].insert(start_line);
            }

            // Try to move to the first child
            if cursor.goto_first_child() {
                continue;
            }

            // If no children, try to move to the next sibling
            if cursor.goto_next_sibling() {
                continue;
            }

            // If no next sibling, go up the tree
            loop {
                if !cursor.goto_parent() {
                    // We've reached the root again, we're done
                    return;
                }

                // go to next sibling, break to continue outer loop
                if cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    // pub fn get_config(&self) -> &TSLanguageConfig {
    //     &self.config
    // }

    pub fn get_lois(&self) -> &HashSet<usize> {
        &self.lois
    }

    /// add lines of interest to the context
    pub fn add_lois(&mut self, lois: Vec<usize>) {
        self.lois.extend(lois);
    }

    pub fn print_state(&self) {
        // we want to see the headers which have been set
        // we want to see the lois
        // the nodes which have been set
        // scopes is the most important
        println!("tree_context::scope_debugging");
        self.scopes
            .iter()
            .enumerate()
            .for_each(|(line_number, values)| {
                if !values.is_empty() {
                    println!(
                        "scope::({})::({})",
                        line_number + 1,
                        values
                            .iter()
                            .map(|value| (value + 1).to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                }
            });
    }

    /// Updated state from this is the show lines
    pub fn add_context(&mut self) {
        if self.lois.is_empty() {
            return;
        }

        self.show_lines = self.lois.clone();

        if self.loi_pad > 0 {
            // for each interesting line
            for line in self.show_lines.clone().iter() {
                // for each of their surrounding lines
                for new_line in
                    line.saturating_sub(self.loi_pad)..=line.saturating_add(self.loi_pad)
                // since new_line usize could be negative
                {
                    if new_line >= self.num_lines {
                        continue;
                    }

                    self.show_lines.insert(new_line);
                }
            }
        }

        if self.last_line {
            // add the bottom line
            // we are adding \n and then we need to go back to 0 based indexing
            // so we need to do - 2
            let bottom_line = self.num_lines - 2;
            self.show_lines.insert(bottom_line);
            self.add_parent_scopes(bottom_line, vec![]);
        }

        if self.parent_context {
            for index in self.lois.clone().iter() {
                self.add_parent_scopes(*index, vec![]);
            }
        }

        if self.child_context {
            for index in self.lois.clone().iter() {
                self.add_child_context(*index);
            }
        }

        // this shows the top of the lines
        if self.margin > 0 {
            self.show_lines.extend(0..self.margin);
        }

        let mut line_numbers = self.show_lines.clone().into_iter().collect::<Vec<_>>();
        line_numbers.sort_unstable();
        self.close_small_gaps();
    }

    fn close_small_gaps(&mut self) {
        self.show_lines =
            close_small_gaps_helper(self.show_lines.clone(), self.lines.to_vec(), self.num_lines);
    }

    fn add_child_context(&mut self, index: usize) {
        if self.nodes[index].is_empty() {
            return;
        }

        // are we getting the largest node here all the time??
        let last_line = self.get_last_line_of_scope(index);
        let size = last_line - index;

        if size < 5 {
            self.show_lines.extend(index..=last_line); // inclusive
            return;
        }

        let mut children: Vec<Node> = vec![];

        // for all nodes that start at line[index], extend children.
        for node in self.nodes[index].iter() {
            // check find_all_children function over here
            children.extend(self.find_all_children(*node));
        }

        children.sort_by_key(|node| node.end_position().row - node.start_position().row);
        children.reverse();

        let currently_showing = self.show_lines.len();

        let max_to_show = 25;
        let min_to_show = 5;
        let percent_to_show = 0.10;
        let max_to_show = max(
            min((size as f64 * percent_to_show) as usize, max_to_show),
            min_to_show,
        );

        let child_start_lines: Vec<usize> = children
            .iter()
            .map(|child| child.start_position().row)
            .collect();

        for &child_start_line in child_start_lines.iter() {
            if self.show_lines.len() > currently_showing + max_to_show {
                return;
            }
            self.add_parent_scopes(child_start_line, vec![]);
        }
    }

    fn find_all_children(&self, node: Node<'a>) -> Vec<Node<'a>> {
        let mut children = vec![node];
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            children.push(child);
        }

        children
    }

    fn get_last_line_of_scope(&self, index: usize) -> usize {
        self.nodes[index]
            .iter()
            .map(|node| node.end_position().row)
            .max()
            .unwrap()
    }

    pub fn format(&self) -> String {
        if self.show_lines.is_empty() {
            return String::new();
        }

        let mut cloned_show_lines = self.show_lines.clone().into_iter().collect::<Vec<_>>();
        cloned_show_lines.sort();

        let mut output = String::new();

        // understand this 🤷‍♂️
        let mut dots = !(self.show_lines.contains(&0));

        for (index, _line) in self.lines.iter().enumerate() {
            if !self.show_lines.contains(&index) {
                if dots {
                    if self.line_number {
                        output.push_str("...⋮...\n");
                    } else {
                        output.push_str("⋮...\n");
                    }

                    dots = false;
                }
                continue;
            }

            let spacer = if self.lois.contains(&index) && self.mark_lois {
                "█".to_owned()
            } else {
                "|".to_owned()
            };

            let line_output = format!("{}{}\n", spacer, &self.lines[index]);

            output.push_str(&line_output);

            dots = true;
        }

        output
    }

    // TODO: understand this completely since we are messing with states
    // maybe this is correct?
    pub fn add_parent_scopes(&mut self, index: usize, recurse_depth: Vec<usize>) {
        if self.done_parent_scopes.contains(&index) {
            return;
        }

        self.done_parent_scopes.insert(index);

        // here for the scopes we are getting the headers and then figuring out
        // the first header (or the biggest one which we want to keep for the scope)
        if self.scopes.get(index).is_none() {
            return;
        }
        for line_num in self.scopes[index].clone().iter() {
            let (_size, head_start, head_end) = self.header[*line_num]
                .first()
                .expect("header to be always present");

            if head_start > &0 || self.show_top_of_file_parent_scope {
                self.show_lines.extend(*head_start..*head_end);
            }

            if self.last_line {
                let mut recurse_depth_cloned = recurse_depth.clone();
                recurse_depth_cloned.push(index);
                let last_line = self.get_last_line_of_scope(*line_num);
                self.add_parent_scopes(last_line, recurse_depth_cloned);
            }
        }
    }

    /// We want to grab the right header range?
    fn arrange_headers(&mut self) {
        for line_number in 0..self.num_lines {
            // what is the sorting doing over here??
            self.header[line_number].sort_unstable();
            // determine the header's start and end lines
            let start_end_maybe = if self.header[line_number].len() > 1 {
                let (size, start, end) = self.header[line_number][0];

                // if the node spans more than the max header size, curtail the header
                Some(if size > self.header_max {
                    (start, start + self.header_max)
                } else {
                    (start, end)
                })
            } else if self.header[line_number].len() == 1 {
                // if the node spans only one line
                Some((line_number, line_number + 1))
            } else {
                Some((line_number, line_number + 1))
            };

            if let Some((start_line, end_line)) = start_end_maybe {
                // size is now redundant
                self.header[line_number] = vec![(0, start_line, end_line)];
            }
        }
    }
}
