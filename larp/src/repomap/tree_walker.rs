use std::collections::HashSet;

use tree_sitter::{Node, Tree, TreeCursor};

pub struct TreeWalker2<'a> {
    nodes: Vec<Vec<Node<'a>>>,
    scopes: Vec<HashSet<usize>>, // the starting lines of the nodes that span the line
    header: Vec<Vec<(usize, usize, usize)>>, // the size, start line, end line of the nodes that span the line
}

impl<'a> TreeWalker2<'a> {
    pub fn new(num_lines: usize) -> Self {
        Self {
            // nodes_for_line: vec![Vec::new(); num_lines],
            nodes: vec![Vec::new(); num_lines],
            scopes: vec![HashSet::new(); num_lines],
            header: vec![Vec::new(); num_lines],
        }
    }

    pub fn get_all_true_nodes(&self) -> &Vec<Vec<Node<'a>>> {
        &self.nodes
    }

    pub fn get_nodes_for_line(&self, line: usize) -> &Vec<Node<'a>> {
        &self.nodes[line]
    }

    pub fn walk(&mut self, mut cursor: TreeCursor<'a>) {
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
}

pub struct TreeWalker<'a> {
    scopes: Vec<HashSet<usize>>, // the starting lines of the nodes that span the line
    header: Vec<Vec<(usize, usize, usize)>>, // the size, start line, end line of the nodes that span the line
    nodes: Vec<Vec<Node<'a>>>,               // tree-sitter node requires lifetime parameter
    tree: Tree,
}

impl<'a> TreeWalker<'a> {
    pub fn new(tree: Tree, num_lines: usize) -> Self {
        Self {
            scopes: vec![HashSet::new(); num_lines],
            header: vec![Vec::new(); num_lines],
            nodes: vec![Vec::new(); num_lines],
            tree,
        }
    }

    pub fn get_tree(&self) -> &Tree {
        &self.tree
    }

    // pub fn compute(&'a mut self) {
    //     let root_node = self.tree.root_node();
    //     self.walk_tree(root_node);
    // }

    pub fn walk_tree(&mut self, node: Node<'a>) {
        let start_line = node.start_position().row;
        let end_line = node.end_position().row;
        let size = end_line - start_line;
        // let node = node.clone();
        self.nodes[start_line].push(node);

        // only assign headers to nodes that span more than one line
        // multiple nodes may share the same start line
        if size > 0 {
            self.header[start_line].push((size, start_line, end_line));
        }

        // for every line the node spans, add the position of its start line
        for i in start_line..=end_line {
            self.scopes[i].insert(start_line);
        }

        let mut cursor = node.walk();

        if cursor.goto_first_child() {
            loop {
                self.walk_tree(cursor.node());
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
            cursor.goto_parent();
        }
    }

    pub fn get_scopes(&self) -> &Vec<HashSet<usize>> {
        &self.scopes
    }

    pub fn get_headers(&self) -> &Vec<Vec<(usize, usize, usize)>> {
        &self.header
    }

    pub fn get_nodes(&self) -> &Vec<Vec<Node<'a>>> {
        &self.nodes
    }
}
