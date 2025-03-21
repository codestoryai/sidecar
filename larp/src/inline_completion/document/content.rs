//! We keep track of the document lines properly, so we can get data about which lines have been
//! edited and which are not changed, this way we can know which lines to keep track of

use lazy_static::lazy_static;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use regex::Regex;
use tree_sitter::Tree;

use crate::{
    chunking::{
        editor_parsing::EditorParsing,
        text_document::{Position, Range},
        types::{FunctionInformation, OutlineNode},
    },
    inline_completion::helpers::split_on_lines_editor_compatiable,
};

lazy_static! {
    static ref SPLITTING_WORDS: Regex = Regex::new(r"[\s.,/#!$%^&*;:{}=\-_`~()\[\]><]").unwrap();
}

#[derive(Debug, Default, Clone)]
pub struct IdentifierNodeInformation {
    identifier_nodes: Vec<(String, Range)>,
    function_type_parameters: Vec<(String, Range)>,
    import_nodes: Vec<(String, Range)>,
}

impl IdentifierNodeInformation {
    pub fn new(
        identifier_nodes: Vec<(String, Range)>,
        function_type_parameters: Vec<(String, Range)>,
        import_nodes: Vec<(String, Range)>,
    ) -> Self {
        Self {
            identifier_nodes,
            function_type_parameters,
            import_nodes,
        }
    }

    pub fn identifier_nodes_len(&self) -> usize {
        self.identifier_nodes.len()
    }

    pub fn identifier_nodes(self) -> Vec<(String, Range)> {
        self.identifier_nodes
    }

    pub fn function_type_parameters(self) -> Vec<(String, Range)> {
        self.function_type_parameters
    }

    pub fn import_nodes(self) -> Vec<(String, Range)> {
        self.import_nodes
    }
}

// we want to split the camel case string into parts
fn split_camel_case(text: &str) -> Vec<String> {
    // an easy way to detect if the text is indeed camel case is to check
    // if it only contains alphabets and then we can easily split on the capital
    // ones
    if text.chars().any(|char| !char.is_ascii_alphabetic()) {
        vec![text.to_owned()]
    } else {
        // all fo them are alphabetic, so we can figure out the splitting
        // magic now
        let mut current_word = "".to_owned();
        let mut words = vec![];
        for character in text.chars() {
            if character.is_uppercase() {
                // flush the previous running word and start a new word now
                words.push(current_word.to_owned());
                current_word = String::from(character);
            } else {
                current_word = current_word + &String::from(character);
            }
        }
        if !current_word.is_empty() {
            words.push(current_word);
        }
        // filter out the words which might be empty
        words
            .into_iter()
            .filter(|word| !word.is_empty())
            .map(|word| word.to_lowercase())
            .collect::<Vec<_>>()
    }
}

fn split_into_words(e: &str) -> Vec<String> {
    SPLITTING_WORDS
        .split(e)
        .filter_map(|word| {
            let cleaned_word = word.trim();
            // we want to also detect the camel case words here and then tokenize
            // them further and make them all small cases
            if !cleaned_word.is_empty() {
                let split_words = split_camel_case(cleaned_word);
                Some(split_words)
            } else {
                None
            }
        })
        // we need to flatten here since we are returning a vector when we split
        // on the camel case words as well
        .flatten()
        .collect()
}

fn split_into_hashset(lines: Vec<String>) -> HashSet<String> {
    let mut final_content: HashSet<String> = Default::default();
    lines.into_iter().for_each(|line| {
        let words = split_into_words(&line);
        words.into_iter().for_each(|word| {
            final_content.insert(word);
        })
    });
    final_content
}

#[derive(Debug, Clone)]
pub struct SnippetInformationWithScore {
    snippet_information: SnippetInformation,
    score: f32,
    file_path: String,
}

impl SnippetInformationWithScore {
    pub fn score(&self) -> f32 {
        self.score
    }

    pub fn snippet_information(&self) -> &SnippetInformation {
        &self.snippet_information
    }

    pub fn file_path(&self) -> &str {
        &self.file_path
    }
}

#[derive(Debug, Clone)]
pub struct SnippetInformation {
    snippet_lines: Vec<String>,
    start_line: usize,
    end_line: usize,
}

impl SnippetInformation {
    pub fn new(snippet_lines: Vec<String>, start_line: usize, end_line: usize) -> Self {
        SnippetInformation {
            snippet_lines,
            start_line,
            end_line,
        }
    }

    pub fn snippet(&self) -> String {
        self.snippet_lines.join("\n")
    }

    pub fn merge_snippets(self, after: Self) -> Self {
        let start_line = self.start_line;
        let end_line = after.end_line;
        // dbg!(
        //     "merge_snippets",
        //     self.start_line,
        //     self.end_line,
        //     after.start_line,
        //     after.end_line
        // );
        let current_snippet_lines = self
            .snippet_lines
            .iter()
            .enumerate()
            .map(|(idx, line)| {
                let line_number = idx + self.start_line;
                (line_number, line.to_owned())
            })
            .collect::<Vec<_>>();
        let other_snippet_lines = after
            .snippet_lines
            .iter()
            .enumerate()
            .map(|(idx, line)| {
                let line_number = idx + after.start_line;
                (line_number, line.to_owned())
            })
            .collect::<Vec<_>>();
        let mut line_map: HashMap<usize, String> = Default::default();
        current_snippet_lines
            .into_iter()
            .for_each(|(line_number, content)| {
                line_map.insert(line_number, content);
            });
        other_snippet_lines
            .into_iter()
            .for_each(|(line_number, content)| {
                line_map.insert(line_number, content);
            });
        let mut new_content = vec![];
        for index in start_line..end_line + 1 {
            new_content.push(
                line_map
                    .remove(&index)
                    .expect("line number to be always present")
                    .clone(),
            );
        }
        Self {
            snippet_lines: new_content,
            start_line,
            end_line,
        }
    }

    /// We want to make sure that the snippets which should be together are merged
    pub fn coelace_snippets(snippets: Vec<SnippetInformation>) -> Vec<SnippetInformation> {
        let mut snippets = snippets;
        snippets.sort_by(|a, b| a.start_line.cmp(&b.start_line));
        if snippets.is_empty() {
            vec![]
        } else {
            let mut merged_snippets = vec![];
            let mut current_snippet = snippets[0].clone();
            for i in 1..snippets.len() {
                let next_snippet = snippets[i].clone();
                if current_snippet.end_line >= next_snippet.start_line {
                    // we can merge these 2 snippets together
                    current_snippet = current_snippet.merge_snippets(next_snippet);
                } else {
                    merged_snippets.push(current_snippet);
                    current_snippet = snippets[i].clone();
                }
            }
            merged_snippets.push(current_snippet);
            merged_snippets
        }
    }
}

/// This contains the bag of words for the given snippets and it uses a custom
/// tokenizer to extract the words from the code
#[derive(Debug)]
pub struct BagOfWords {
    words: HashSet<String>,
    snippet: SnippetInformation,
}

impl BagOfWords {
    pub fn new(
        snippet_lines: Vec<String>,
        start_line: usize,
        end_line: usize,
        words: HashSet<String>,
    ) -> Self {
        BagOfWords {
            words,
            snippet: SnippetInformation::new(snippet_lines, start_line, end_line),
        }
    }

    fn jaccard_score(&self, other: &Self) -> f32 {
        let intersection_size = self.words.intersection(&other.words).count();
        let union_size = self.words.len() + other.words.len() - intersection_size;
        intersection_size as f32 / union_size as f32
    }
}

/// Keeps track of the lines which have been added and edited into the code
/// Note: This does not keep track of the lines which have been removed
#[derive(Clone, Debug)]
pub enum DocumentLineStatus {
    // also contains the timestamp when the line was last edited
    Edited(i64),
    Unedited,
}

#[derive(Debug)]
pub struct DocumentLine {
    line_status: DocumentLineStatus,
    content: String,
}

impl DocumentLine {
    pub fn line_status(&self) -> DocumentLineStatus {
        self.line_status.clone()
    }

    pub fn is_edited(&self) -> bool {
        matches!(self.line_status, DocumentLineStatus::Edited(_))
    }

    pub fn is_unedited(&self) -> bool {
        matches!(self.line_status, DocumentLineStatus::Unedited)
    }
}

pub struct DocumentEditLines {
    lines: Vec<DocumentLine>,
    file_path: String,
    _language: String,
    // What snippets are in the document
    // Some things we should take care of:
    // when providing context to the inline autocomplete we want to make sure that
    // the private methods are not shown (cause they are not necessary)
    // when showing snippets for jaccard similarity, things are difference
    // we want to show the content for it no matter what
    // basically if its because of a symbol then we should only show the outline here
    // but if that's not the case, then its fine
    window_snippets: Vec<BagOfWords>,
    editor_parsing: Arc<EditorParsing>,
    tree: Option<Tree>,
    function_information: Vec<FunctionInformation>,
    outline_nodes: Vec<OutlineNode>,
    import_identifier_nodes: Vec<(String, Range)>,
    // we should have an option to delete the bag of words, cause this does not
    // make sense
}

impl DocumentEditLines {
    pub fn new(
        file_path: String,
        content: String,
        language: String,
        editor_parsing: Arc<EditorParsing>,
    ) -> DocumentEditLines {
        let mut document_lines = if content == "" {
            DocumentEditLines {
                lines: vec![DocumentLine {
                    line_status: DocumentLineStatus::Unedited,
                    content: "".to_string(),
                }],
                file_path,
                _language: language,
                window_snippets: vec![],
                editor_parsing,
                tree: None,
                function_information: vec![],
                outline_nodes: vec![],
                import_identifier_nodes: vec![],
            }
        } else {
            let lines = split_on_lines_editor_compatiable(&content)
                .into_iter()
                .map(|line_content| DocumentLine {
                    line_status: DocumentLineStatus::Unedited,
                    content: line_content.to_string(),
                })
                .collect::<Vec<_>>();
            DocumentEditLines {
                lines,
                file_path,
                _language: language,
                window_snippets: vec![],
                editor_parsing,
                tree: None,
                function_information: vec![],
                outline_nodes: vec![],
                import_identifier_nodes: vec![],
            }
        };
        // This is a very expensive operation for now, we are going to optimize the shit out of this 🍶
        let _ = document_lines.generate_snippets(None);
        document_lines
    }

    pub fn outline_nodes(&self) -> Vec<OutlineNode> {
        self.outline_nodes.to_vec()
    }

    pub fn get_line_content(&self, line_number: usize) -> Option<String> {
        if line_number < self.lines.len() {
            Some(self.lines[line_number].content.to_owned())
        } else {
            None
        }
    }

    fn set_tree(&mut self) {
        if let Some(language_config) = self.editor_parsing.for_file_path(&self.file_path) {
            let tree = language_config.get_tree_sitter_tree(self.get_content().as_bytes());
            self.tree = tree;
        }
    }

    pub fn get_content(&self) -> String {
        self.lines
            .iter()
            .map(|line| line.content.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn get_lines_in_range(&self, range: &Range) -> String {
        let mut content = String::new();
        for i in range.start_line()..range.end_line() {
            let line = &self.lines[i];
            if line.is_edited() {
                content.push_str(("+".to_owned() + &self.lines[i].content).as_str());
            } else {
                content.push_str(&self.lines[i].content);
            }
            content.push('\n');
        }
        content
    }

    pub fn get_symbols_in_ranges(&self, range: &[Range]) -> Vec<OutlineNode> {
        // over here we are trying to get the outline node and do it this way:
        // - we look at the range which is requested and generate it back
        // - we check if the symbols are going to intersect with the range we are querying for
        // if there is intersection we insert it (super easy)
        let selected_ranges = range.iter().collect::<HashSet<&Range>>();
        self.outline_nodes
            .iter()
            .filter_map(|outline_node| {
                let outline_node_range = outline_node.range();
                if selected_ranges
                    .iter()
                    .any(|selected_range| outline_node_range.contains(selected_range))
                {
                    Some(outline_node.clone())
                } else {
                    // we are totally okay over here
                    None
                }
            })
            .collect::<Vec<_>>()
    }

    pub fn get_edited_lines_in_range(&self, range: &Range) -> Vec<usize> {
        (range.start_line()..range.end_line())
            .into_iter()
            .filter_map(|line_number| {
                let line = &self.lines[line_number];
                if line.is_edited() {
                    Some(line_number)
                } else {
                    None
                }
            })
            .collect()
    }

    fn indentation_at_position(&self, position: &Position) -> usize {
        let mut indentation = 0;
        let line_number = position.line();
        let line_content = &self.lines[line_number].content;
        // indentation is consistent so we do not have to worry about counting
        // the spaces which tabs will take
        for c in line_content.chars() {
            if c == ' ' {
                indentation += 1;
            } else if c == '\t' {
                indentation += 1;
            } else {
                break;
            }
        }
        indentation
    }

    pub fn get_identifier_nodes(&self, position: Position) -> IdentifierNodeInformation {
        // grab the function definition here
        let current_indentation_position = self.indentation_at_position(&position);
        let contained_function = self
            .function_information
            .iter()
            .filter(|function_information| {
                function_information.range().contains_position(&position)
            })
            .next();
        let mut identifier_nodes = vec![];
        let mut function_parameters_nodes = vec![];
        if let Some(contained_function) = contained_function {
            contained_function
                .get_identifier_nodes()
                .map(|function_identifier_nodes| {
                    identifier_nodes = function_identifier_nodes
                        .iter()
                        .filter_map(|identifier_node| {
                            let identifier_node_position = identifier_node.1.start_position();
                            // remove the nodes which are indented more than the current position
                            // this will help reduce the number of identifier nodes we get back
                            if identifier_node.1.end_position().before_other(&position)
                                && self.indentation_at_position(&identifier_node_position)
                                    <= current_indentation_position
                            {
                                Some((identifier_node.0.to_owned(), identifier_node.1.clone()))
                            } else {
                                None
                            }
                        })
                        .collect();
                });
        }
        if let Some(contained_function) = contained_function {
            function_parameters_nodes = contained_function
                .get_function_parameters()
                .into_iter()
                .map(|function_parameter| function_parameter.clone())
                .flatten()
                .collect::<Vec<_>>();
        }
        // TODO(skcd): Add the import nodes here
        IdentifierNodeInformation::new(identifier_nodes, function_parameters_nodes, vec![])
    }

    fn remove_range(&mut self, range: Range) {
        let start_line = range.start_line();
        let start_column = range.start_column();
        let end_line = range.end_line();
        let end_column = range.end_column();
        // Why are we putting a -1 here, well there is a reason for it
        // when vscode provides us the range to replace, it gives us the end
        // column as the last character of the selection + 1, for example
        // if we have the content as: "abcde"
        // and we want to replace "de" in "abcde", we get back
        // the range as:
        // start_line: 0, start_column: 3, end_line: 0, end_column: 5 (note this is + 1 the final position)
        // so we subtract it with -1 here to keep things sane
        // a catch here is that the end_column can also be 0 if we are removing empty lines
        // so we guard and then subtract
        if start_line == end_line {
            if start_column == end_column {
                return;
            } else {
                let end_column = if range.end_column() != 0 {
                    range.end_column() - 1
                } else {
                    range.end_column()
                };
                // we get the line at this line number and remove the content between the start and end columns
                let line = self.lines.get_mut(start_line).unwrap();
                let start_index = start_column;
                let end_index = end_column;
                let mut characters = line.content.chars().collect::<Vec<_>>();
                let start_index = start_index as usize;
                let end_index = end_index as usize;
                characters.drain(start_index..end_index + 1);
                line.content = characters.into_iter().collect();
            }
        } else {
            // This is a more complicated case
            // we handle it by the following ways:
            // - handle the start line and keep the prefix required
            // - handle the end line and keep the suffix as required
            // - remove the lines in between
            // - merge the prefix and suffix of the start and end lines

            // get the start of line prefix
            let start_line_characters = self.lines[start_line].content.chars().collect::<Vec<_>>();
            let start_line_prefix = start_line_characters[..start_column as usize].to_owned();
            // get the end of line suffix
            let end_column = range.end_column();
            let end_line_characters = self.lines[end_line].content.chars().collect::<Vec<_>>();
            let end_line_suffix = end_line_characters[end_column..].to_owned();
            {
                let start_doc_line = self.lines.get_mut(start_line).unwrap();
                start_doc_line.content = start_line_prefix.into_iter().collect::<String>()
                    + &end_line_suffix.into_iter().collect::<String>();
            }
            // which lines are we draining in between?
            // dbg!(
            //     "sidecar.drain_lines.remove_range",
            //     &start_line + 1,
            //     &end_line + 1
            // );
            // remove the lines in between the start line and the end line
            self.lines.drain(start_line + 1..end_line + 1);
        }
    }

    fn insert_at_position(&mut self, position: Position, content: String, timestamp: i64) {
        // If this is strictly a removal, then we do not need to insert anything
        if content == "" {
            return;
        }
        // when we want to insert at the position so first we try to start appending it at the line number from the current column
        // position and also add the suffix which we have, this way we get the new lines which need to be inserted
        let line_content = self.lines[position.line()].content.to_owned();
        let characters = line_content.chars().into_iter().collect::<Vec<_>>();
        // get the prefix right before the column position
        let prefix = characters[..position.column() as usize]
            .to_owned()
            .into_iter()
            .collect::<String>();
        // get the suffix right after the column position
        let suffix = characters[position.column() as usize..]
            .to_owned()
            .into_iter()
            .collect::<String>();
        // the new content here is the prefix + content + suffix
        let new_content = format!("{}{}{}", prefix.to_owned(), content, suffix);

        // now we get the new lines which need to be inserted
        // TODO(skcd): We should check here if the contents of the lines are same
        // if thats the case then we should not mark the current line as edited at all
        let mut new_lines = split_on_lines_editor_compatiable(&new_content)
            .into_iter()
            .map(|line| DocumentLine {
                line_status: DocumentLineStatus::Edited(timestamp),
                content: line.to_owned(),
            })
            .collect::<Vec<_>>();
        // we are also checking if the first line is the same as before, in which case
        // its not been edited
        new_lines.first_mut().map(|first_changed_line| {
            if first_changed_line.content == line_content {
                first_changed_line.line_status = DocumentLineStatus::Unedited;
            }
        });
        // dbg!("sidecar.insert_at_position", &new_lines);
        // we also need to remove the line at the current line number
        self.lines.remove(position.line());
        // now we add back the lines which need to be inserted
        self.lines
            .splice(position.line()..position.line(), new_lines);
    }

    fn snippets_using_sliding_window(&mut self, lines: Vec<String>) {
        // useful links: https://github.com/thakkarparth007/copilot-explorer/blob/4a572ef4653811e789a05b370d3da49a784d6172/codeviz/data/module_codes_renamed/1016.js#L21-L25
        // https://github.com/thakkarparth007/copilot-explorer/blob/4a572ef4653811e789a05b370d3da49a784d6172/codeviz/data/module_codes_renamed/1016.js#L61-L87
        // Maximum snippet size here is 50 lines and we want to generate the snippets using the lines
        // we also want to keep a sliding window like context so we can skip over
        // 10 lines in the window while still getting good perf out of this
        let mut final_snippets = vec![];

        // we are going to change the algorithm to be the following:
        // - iterate over each line and split them using the regex which we have above
        // - create a running hashmap of the words and their frequency occurance
        // - for each window iterate over the entries in the hashmap and create the hashset of the words
        // - profit?

        let mut line_map: HashMap<usize, Vec<String>> = Default::default();
        for (idx, line) in lines.iter().enumerate() {
            line_map.insert(idx, split_into_words(line));
        }

        let mut running_word_count: HashMap<String, usize> = Default::default();

        for index in 0..std::cmp::min(lines.len(), 50) {
            let bag_of_words = line_map.get(&index);
            if let Some(bag_of_words) = bag_of_words {
                bag_of_words.iter().for_each(|word| {
                    if !running_word_count.contains_key(word) {
                        running_word_count.insert(word.to_owned(), 1);
                    } else {
                        running_word_count.insert(
                            word.to_owned(),
                            running_word_count
                                .get(word)
                                .expect("if !contains_key to work")
                                + 1,
                        );
                    }
                });
            }
        }

        // using +1 notation here so we do not run into subtraction errors when using usize
        if lines.len() <= 50 {
            let line_length = lines.len();
            final_snippets.push(BagOfWords::new(
                lines,
                1,
                line_length,
                running_word_count
                    .into_iter()
                    .filter_map(|(key, value)| {
                        if value > 0 {
                            Some(key.to_owned())
                        } else {
                            None
                        }
                    })
                    .collect(),
            ));
        } else {
            let last_index = lines.len() - 50 - 1;
            for index in 1..(lines.len() - 50) {
                // first line is: 0 - 49
                // second line is: 1 - 50 (for this we need to remove 0th line and add the 50th line)
                // we need to remove the entries of the previous line from the running hashmap
                // and then we need to add the entries from the index + 50th line
                let removing_line = index - 1;
                let adding_line = index + (50 - 1);
                let removing_line_bag = line_map.get(&removing_line);
                let added_line_bag = line_map.get(&adding_line);
                if let Some(removing_line_bag) = removing_line_bag {
                    removing_line_bag.iter().for_each(|word| {
                        // how is this showing up??
                        let value = running_word_count.get(word).expect("to be present");
                        running_word_count.insert(word.to_owned(), value - 1);
                    });
                }
                if let Some(added_line_bag) = added_line_bag {
                    added_line_bag.iter().for_each(|word| {
                        if !running_word_count.contains_key(word) {
                            running_word_count.insert(word.to_owned(), 1);
                        } else {
                            running_word_count.insert(
                                word.to_owned(),
                                running_word_count
                                    .get(word)
                                    .expect("if !contains_key to work")
                                    + 1,
                            );
                        }
                    });
                }
                let current_lines = lines[index..index + 50].to_vec();
                // always take the 1 in 4th in 500 lines
                if index % 4 == 0 || index == last_index {
                    final_snippets.push(BagOfWords::new(
                        current_lines,
                        index + 1,
                        index + 1 + 49, // index + 1 + (50 - 1) since we are adding 50 lines
                        running_word_count
                            .iter()
                            .filter_map(|(key, value)| {
                                if value > &0 {
                                    Some(key.to_owned())
                                } else {
                                    None
                                }
                            })
                            .collect(),
                    ));
                }
            }
        }
        self.window_snippets = final_snippets;
    }

    /// Returns the nodes which have been changed as a side-effect of calling this function
    /// can be used for understanding how the file is changing
    fn generate_snippets(&mut self, changed_range: Option<Range>) -> Vec<OutlineNode> {
        // We should debounce the requests here to rate-limit on the CPU
        // these operations are all CPU intensive and generally if the same file
        // is being edited, we kind of want to make sure that the last edit makes it
        // way here always, the rest can be fine even if they are not immediately acted
        // upon
        self.set_tree();
        // dbg!("document_lines.set_tree", &instant.elapsed());
        // we need to create ths symbol map here for the file so we can lookup the symbols
        // and add the skeleton of it to the inline completion

        // update the function information we are getting from tree-sitter
        let content = self.get_content();
        let content_bytes = content.as_bytes();
        self.function_information = if let (Some(language_config), Some(tree)) = (
            self.editor_parsing.for_file_path(&self.file_path),
            self.tree.as_ref(),
        ) {
            language_config.capture_function_data_with_tree(content_bytes, tree, true)
        } else {
            vec![]
        };
        // dbg!("document_lines.function_information", &instant.elapsed());

        self.outline_nodes = if let (Some(language_config), Some(tree)) = (
            self.editor_parsing.for_file_path(&self.file_path),
            self.tree.as_ref(),
        ) {
            language_config.generate_outline(content_bytes, tree, self.file_path.to_owned())
        } else {
            vec![]
        };
        // dbg!(
        //     "document_lines.generate_outline",
        //     &instant.elapsed(),
        //     &self.outline_nodes.len()
        // );

        self.import_identifier_nodes = if let (Some(language_config), Some(tree)) = (
            self.editor_parsing.for_file_path(&self.file_path),
            self.tree.as_ref(),
        ) {
            language_config.generate_import_identifier_nodes(content_bytes, tree)
        } else {
            vec![]
        };
        // dbg!("document_lines.import_identifier_nodes", &instant.elapsed());

        // check for nodes which have been changed and belong to the range that
        // was inserted, we need to check here in a heriarchial way putting important
        // to the functions which are present in the class as well
        let changed_outline_nodes = if let Some(range_with_changes) = changed_range {
            self.outline_nodes
                .iter()
                .filter_map(|outline_node| {
                    // dbg!("outline_node", &outline_node.range(), &range_with_changes);
                    outline_node.check_smallest_member_in_range(&range_with_changes)
                })
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        // what are we doing over here and why is it slow???
        // dbg!("document_lines.changed_outline_nodes", &instant.elapsed());

        // after filtered content we have to grab the sliding window context, we generate the windows
        // we have some interesting things we can do while generating the code context
        self.snippets_using_sliding_window(
            content
                .lines()
                .map(|line| line.to_owned())
                .collect::<Vec<_>>(),
        );
        // dbg!(
        //     "document_lines.snippets_using_sliding_window",
        //     &instant.elapsed()
        // );

        changed_outline_nodes
    }

    /// If the contents have changed, we need to mark the new lines which have changed
    /// Additionally returns the nodes which have been chagned because of this edit
    pub fn content_change(
        &mut self,
        range: Range,
        new_content: String,
        timestamp: i64,
    ) -> Vec<OutlineNode> {
        // dbg!("content.change", &range, &new_content);
        self.remove_range(range);
        // dbg!("content.removed", &instant.elapsed());
        // Then we insert the new content at the range
        self.insert_at_position(range.start_position(), new_content, timestamp);
        // dbg!("content.insert_at_position", &instant.elapsed());
        // We want to get the code snippets here and make sure that the edited code snippets
        // are together when creating the window
        let outline_nodes = self.generate_snippets(Some(range));
        // is it still slow
        // dbg!("content.generate_snippets", &instant.elapsed());
        outline_nodes
    }

    pub fn get_edited_lines(&self) -> Vec<usize> {
        self.lines
            .iter()
            .enumerate()
            .filter_map(|(idx, line)| if line.is_edited() { Some(idx) } else { None })
            .collect()
    }

    pub fn grab_similar_context(
        // the only reason for this to be mut is so we can generate the window snippets
        &mut self,
        context: &str,
        // This line we always and forever want to skip if present
        // this right now is coming from the current file
        skip_line: Option<usize>,
    ) -> Vec<SnippetInformationWithScore> {
        // go through all the snippets and see which ones are similar to the context
        let lines = context
            .lines()
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        let bag_of_words_hashset = split_into_hashset(lines.to_vec());
        let bag_of_words = BagOfWords::new(lines, 0, 0, bag_of_words_hashset);
        let mut scored_snippets = self
            .window_snippets
            .iter()
            .filter_map(|snippet| {
                let score = snippet.jaccard_score(&bag_of_words);
                if score > 0.0 {
                    Some((score, snippet))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        // f32 comparison should work
        scored_snippets.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        // we remove the snippets which are overlapping and are already included at the top
        let mut included_ranges: Vec<(usize, usize)> = if let Some(skip_line) = skip_line {
            vec![(skip_line, skip_line)]
        } else {
            vec![]
        };
        let mut final_snippets = scored_snippets
            .into_iter()
            .filter_map(|scored_snippet| {
                let snippet_information = &scored_snippet.1.snippet;
                let start_line = snippet_information.start_line;
                let end_line = snippet_information.end_line;
                // check if any of the included ranges already overlaps with the current snippet
                let intersects_range = included_ranges.iter().any(|range| {
                    if range.0 >= start_line && range.0 <= end_line {
                        return true;
                    }
                    if range.1 >= start_line && range.1 <= end_line {
                        return true;
                    }
                    return false;
                });
                if intersects_range {
                    None
                } else {
                    // push the new range to the included ranges
                    included_ranges.push((start_line, end_line));
                    Some(scored_snippet)
                }
            })
            .collect::<Vec<_>>();
        // log the final snippets length
        // we take at the very most 10 snippets from a single file
        // this prevents a single file from giving out too much data
        final_snippets.truncate(10);

        final_snippets
            .into_iter()
            .map(|snippet| SnippetInformationWithScore {
                snippet_information: snippet.1.snippet.clone(),
                score: snippet.0,
                file_path: self.file_path.clone(),
            })
            .collect::<Vec<_>>()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        chunking::{
            editor_parsing::EditorParsing,
            text_document::{Position, Range},
        },
        inline_completion::document::content::split_camel_case,
    };

    use super::DocumentEditLines;

    #[test]
    fn test_document_lines_works() {
        let editor_parsing = Arc::new(EditorParsing::default());
        let document = DocumentEditLines::new(
            "".to_owned(),
            r#"


"#
            .to_owned(),
            "".to_owned(),
            editor_parsing,
        );
        assert_eq!(document.lines.len(), 4);
    }

    #[test]
    fn test_remove_range_works_as_expected() {
        let editor_parsing = Arc::new(EditorParsing::default());
        let mut document = DocumentEditLines::new(
            "".to_owned(),
            r#"FIRST LINE
SECOND LINE
THIRD LINE
FOURTH LINE
FIFTH LINE 🫡
SIXTH LINE 🫡🚀"#
                .to_owned(),
            "".to_owned(),
            editor_parsing,
        );
        let range = Range::new(Position::new(4, 0, 0), Position::new(5, 0, 0));
        document.remove_range(range);
        let updated_content = document.get_content();
        assert_eq!(
            updated_content,
            r#"FIRST LINE
SECOND LINE
THIRD LINE
FOURTH LINE
SIXTH LINE 🫡🚀"#
        );
    }

    #[test]
    fn test_remove_range_empty_works() {
        let editor_parsing = Arc::new(EditorParsing::default());
        let mut document = DocumentEditLines::new(
            "".to_owned(),
            r#"SOMETHING"#.to_owned(),
            "".to_owned(),
            editor_parsing,
        );
        let range = Range::new(Position::new(0, 0, 0), Position::new(0, 0, 0));
        document.remove_range(range);
        let updated_content = document.get_content();
        assert_eq!(updated_content, "SOMETHING");
    }

    #[test]
    fn test_insert_at_position_works_as_expected() {
        let editor_parsing = Arc::new(EditorParsing::default());
        let mut document = DocumentEditLines::new(
            "".to_owned(),
            r#"FIRST LINE
SECOND LINE
THIRD LINE
🫡🫡🫡🫡
FIFTH LINE 🫡
SIXTH LINE 🫡🚀"#
                .to_owned(),
            "".to_owned(),
            editor_parsing,
        );
        let position = Position::new(3, 1, 0);
        document.insert_at_position(position, "🚀🚀🚀\n🪨🪨".to_owned(), 0);
        let updated_content = document.get_content();
        assert_eq!(
            updated_content,
            r#"FIRST LINE
SECOND LINE
THIRD LINE
🫡🚀🚀🚀
🪨🪨🫡🫡🫡
FIFTH LINE 🫡
SIXTH LINE 🫡🚀"#
        );
    }

    #[test]
    fn test_insert_on_empty_document_works() {
        let editor_parsing = Arc::new(EditorParsing::default());
        let mut document =
            DocumentEditLines::new("".to_owned(), "".to_owned(), "".to_owned(), editor_parsing);
        let position = Position::new(0, 0, 0);
        document.insert_at_position(position, "SOMETHING".to_owned(), 0);
        let updated_content = document.get_content();
        assert_eq!(updated_content, "SOMETHING");
    }

    #[test]
    fn test_removing_all_content() {
        let editor_parsing = Arc::new(EditorParsing::default());
        let mut document = DocumentEditLines::new(
            "".to_owned(),
            r#"FIRST LINE
SECOND LINE
THIRD LINE
🫡🫡🫡🫡
FIFTH LINE 🫡
SIXTH LINE 🫡🚀"#
                .to_owned(),
            "".to_owned(),
            editor_parsing,
        );
        let range = Range::new(Position::new(0, 0, 0), Position::new(5, 13, 0));
        document.remove_range(range);
        let updated_content = document.get_content();
        assert_eq!(updated_content, "");
    }

    #[test]
    fn test_removing_content_single_line() {
        let editor_parsing = Arc::new(EditorParsing::default());
        let mut document = DocumentEditLines::new(
            "".to_owned(),
            "blah blah\n// bbbbbbbb\nblah blah".to_owned(),
            "".to_owned(),
            editor_parsing,
        );
        let range = Range::new(Position::new(1, 3, 0), Position::new(1, 11, 0));
        document.remove_range(range);
        let updated_content = document.get_content();
        assert_eq!(updated_content, "blah blah\n// \nblah blah");
    }

    #[test]
    fn test_insert_content_multiple_lines_blank() {
        let editor_parsing = Arc::new(EditorParsing::default());
        let mut document = DocumentEditLines::new(
            "".to_owned(),
            r#"aa

bb

camelCase

dd

ee






fff"#
                .to_owned(),
            "".to_owned(),
            editor_parsing,
        );
        let range = Range::new(Position::new(9, 0, 0), Position::new(13, 0, 0));
        document.content_change(range, "".to_owned(), 0);
        let updated_content = document.get_content();
        let expected_output = r#"aa

bb

camelCase

dd

ee


fff"#;
        assert_eq!(updated_content, expected_output);
    }

    #[test]
    fn test_updating_document_multiline_does_not_break() {
        let original_content = r#"aa

bb

camelCase

dd

ee


fff"#;
        let mut document_lines = DocumentEditLines::new(
            "".to_owned(),
            original_content.to_owned(),
            "".to_owned(),
            Arc::new(EditorParsing::default()),
        );
        let range = Range::new(Position::new(6, 0, 0), Position::new(8, 2, 0));
        document_lines.content_change(range, "expected_output".to_owned(), 0);
        let updated_content = document_lines.get_content();
        let expected_output = r#"aa

bb

camelCase

expected_output


fff"#;
        assert_eq!(updated_content, expected_output);
    }

    #[test]
    fn test_updating_document_multiline_add_remove_leaves_nothing() {
        let original_content = r#"fn something() {
        // bb
        // cc
}"#;
        let mut document_lines = DocumentEditLines::new(
            "/tmp/something.rs".to_owned(),
            original_content.to_owned(),
            "rust".to_owned(),
            Arc::new(EditorParsing::default()),
        );
        let range = Range::new(Position::new(1, 0, 0), Position::new(2, 0, 0));
        let changed_nodes = document_lines.content_change(range, "".to_owned(), 0);
        assert_eq!(changed_nodes[0].name(), "something".to_owned());
        let updated_content = document_lines.get_content();
        let expected_output = r#"fn something() {
        // cc
}"#;
        assert_eq!(updated_content, expected_output);
        // now what if we add it back??
        let range = Range::new(Position::new(1, 0, 0), Position::new(1, 0, 0));
        let changed_nodes = document_lines.content_change(range, "        // bb\n".to_owned(), 0);
        let updated_content = document_lines.get_content();
        let expected_output = r#"fn something() {
        // bb
        // cc
}"#;
        assert_eq!(changed_nodes[0].name(), "something".to_owned());
        assert_eq!(updated_content, expected_output);
    }

    #[test]
    fn test_splitting_camel_case() {
        assert_eq!(split_camel_case("something_else")[0], "something_else");

        let small_start_alternate_camel_case = "smallStartAlternateCamelCase";
        let answer = split_camel_case(small_start_alternate_camel_case);
        assert_eq!(answer.len(), 5);
        assert_eq!(answer[0], "small");
        assert_eq!(answer[1], "start");
        assert_eq!(answer[2], "alternate");
        assert_eq!(answer[3], "camel");
        assert_eq!(answer[4], "case");
    }
}
