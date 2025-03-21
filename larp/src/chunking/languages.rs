use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use tree_sitter::Tree;

use crate::{
    chunking::types::FunctionNodeInformation,
    repomap::tag::{Tag, TagKind},
};

use super::{
    go::go_language_config,
    javascript::javascript_language_config,
    python::python_language_config,
    rust::rust_language_config,
    text_document::{Position, Range},
    types::{
        ClassInformation, ClassNodeType, ClassWithFunctions, FunctionInformation, FunctionNodeType,
        OutlineNode, OutlineNodeContent, OutlineNodeType, TypeInformation, TypeNodeType,
    },
    typescript::typescript_language_config,
};

fn naive_chunker(buffer: &str, line_count: usize, overlap: usize) -> Vec<Span> {
    let mut chunks: Vec<Span> = vec![];
    let current_chunk = buffer
        .lines()
        .into_iter()
        .map(|line| line.to_owned())
        .collect::<Vec<_>>();
    let chunk_length = current_chunk.len();
    let mut start = 0;
    while start < chunk_length {
        let end = (start + line_count).min(chunk_length);
        let chunk = current_chunk[start..end].to_owned();
        let span = Span::new(start, end, None, Some(chunk.join("\n")));
        chunks.push(span);
        start += line_count - overlap;
    }
    chunks
}

fn get_string_from_bytes(source_code: &Vec<u8>, start_byte: usize, end_byte: usize) -> String {
    String::from_utf8(source_code[start_byte..end_byte].to_vec()).unwrap_or_default()
}

fn get_string_from_lines(lines: &[String], start_line: usize, end_line: usize) -> String {
    lines[start_line..=end_line].join("\n")
}

/// We are going to use tree-sitter to parse the code and get the chunks for the
/// code. we are going to use the algo sweep uses for tree-sitter
///
#[derive(Debug, Clone)]
pub struct TSLanguageConfig {
    /// A list of language names that can be processed by these scope queries
    /// e.g.: ["Typescript", "TSX"], ["Rust"]
    pub language_ids: &'static [&'static str],

    /// Extensions that can help classify the file: rs, js, tx, py, etc
    pub file_extensions: &'static [&'static str],

    /// tree-sitter grammar for this language
    pub grammar: fn() -> tree_sitter::Language,

    /// Namespaces defined by this language,
    /// E.g.: type namespace, variable namespace, function namespace
    pub namespaces: Vec<Vec<String>>,

    /// The documentation query which will be used by this language
    pub documentation_query: Vec<String>,

    /// The queries to get the function body for the language
    pub function_query: Vec<String>,

    /// The different constructs for the language and their tree-sitter node types
    pub construct_types: Vec<String>,

    /// The different expression statements which are present in the language
    pub expression_statements: Vec<String>,

    /// The queries we use to get the class definitions
    pub class_query: Vec<String>,

    pub r#type_query: Vec<String>,

    /// The namespaces of the symbols which can be applied to a code symbols
    /// in case of typescript it can be `export` keyword
    pub namespace_types: Vec<String>,

    /// Hoverable queries are used to get identifier which we can hover over
    /// or written another way these are the important parts of the document
    /// which a user can move their marker over and get back data
    pub hoverable_query: String,

    /// The comment prefix for the language, typescript is like // and rust
    /// is like //, python is like #
    pub comment_prefix: String,

    /// This is used to keep track of the end of line situations in many lanaguages
    /// if they exist
    pub end_of_line: Option<String>,

    /// Tree sitter node types used to detect imports which are present in the file
    pub import_identifier_queries: String,

    /// Block start detection for the language
    pub block_start: Option<String>,

    /// Queries which help us figure out the variable identifiers so we can use go-to-definition
    /// on top of them
    pub variable_identifier_queries: Vec<String>,

    /// Generates the outline for the file which will be used to get the
    /// outline of the file
    pub outline_query: Option<String>,

    /// The file paths which need to be excluded when providing the
    /// type definitions
    pub excluded_file_paths: Vec<String>,

    /// The language string which can be used as an identifier for this language
    pub language_str: String,

    /// Used to specify which object a method or property belongs to.
    pub object_qualifier: String,

    /// Used to get the definitions for the file
    pub file_definitions_query: String,

    /// Required parameters of functions query
    pub required_parameter_types_for_functions: String,

    /// Grabs the span of a function call for example in rust: a.b.c.d(bllbbbbbb)
    /// this query can capture a.b.c.d (very useful when catching errors llm make with
    /// function hallucinations)
    pub function_call_path: Option<String>,
}

impl TSLanguageConfig {
    pub fn is_python(&self) -> bool {
        self.language_ids.contains(&"python")
    }

    pub fn is_rust(&self) -> bool {
        self.language_ids.contains(&"rust")
    }

    pub fn is_js_like(&self) -> bool {
        self.language_ids.contains(&"javascript") || self.language_ids.contains(&"typescript")
    }

    /// If the language is of type where there is a single implementation block making the changes
    pub fn is_single_implementation_block_language(&self) -> bool {
        self.is_python() || self.is_js_like()
    }

    pub fn get_language(&self) -> Option<String> {
        self.language_ids.first().map(|s| s.to_string())
    }

    pub fn is_valid_code(&self, code: &str) -> bool {
        let grammar = self.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let tree_maybe = parser.parse(code, None);
        tree_maybe
            .map(|tree| !tree.root_node().has_error())
            .unwrap_or_default()
    }

    pub fn is_file_relevant(&self, file_path: &str) -> bool {
        !self
            .excluded_file_paths
            .iter()
            .any(|file_path_part| file_path.contains(file_path_part))
    }

    /// We return a range here for all the nodes which we can hover on and do editor
    /// operations
    pub fn hoverable_nodes(&self, source_code: &[u8]) -> Vec<Range> {
        let grammar = self.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let hoverable_query = self.hoverable_query.to_owned();
        let tree = parser.parse(source_code, None).unwrap();
        let query = tree_sitter::Query::new(grammar(), &hoverable_query).expect("to work");
        let mut cursor = tree_sitter::QueryCursor::new();
        let query_captures = cursor.captures(&query, tree.root_node(), source_code);
        let mut hoverable_nodes: HashSet<Range> = Default::default();
        query_captures.into_iter().for_each(|capture| {
            capture.0.captures.into_iter().for_each(|capture| {
                let hover_range = Range::for_tree_node(&capture.node);
                if !hoverable_nodes.contains(&hover_range) {
                    hoverable_nodes.insert(hover_range);
                }
            })
        });
        hoverable_nodes.into_iter().collect::<Vec<_>>()
    }

    /// Generates a fresh outline node and does quite a bit of heavy-lifting
    /// Tree generation by tree-sitter is computiationally expensive, we should
    /// default to always storing the tree and maintaining it by looking at the edits
    ///
    /// This can be used to generate a fresh instance and parse the code
    pub fn generate_outline_fresh(
        &self,
        source_code: &[u8],
        fs_file_path: &str,
    ) -> Vec<OutlineNode> {
        let grammar = self.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(grammar())
            .expect("for lanaguage parser from tree_sitter should not fail");
        let tree = parser.parse(source_code, None).unwrap();
        self.generate_outline(source_code, &tree, fs_file_path.to_owned())
    }

    pub fn generate_outline(
        &self,
        source_code: &[u8],
        tree: &Tree,
        fs_file_path: String,
    ) -> Vec<OutlineNode> {
        let grammar = self.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let outline_query = self.outline_query.clone();
        if let None = outline_query {
            return vec![];
        }
        let node = tree.root_node();
        let outline_query = outline_query.expect("if let None to hold");
        let query = tree_sitter::Query::new(grammar(), &outline_query).expect("to work");
        let mut cursor = tree_sitter::QueryCursor::new();
        let query_captures = cursor.captures(&query, node, source_code);
        let mut outline_nodes: Vec<(OutlineNodeType, Range)> = vec![];
        let mut range_set: HashSet<Range> = HashSet::new();
        let mut compressed_outline: Vec<OutlineNode> = vec![];
        query_captures.into_iter().for_each(|capture| {
            capture.0.captures.into_iter().for_each(|capture| {
                let capture_name = query
                    .capture_names()
                    .to_vec()
                    .remove(capture.index.try_into().unwrap());
                let outline_name = OutlineNodeType::from_str(&capture_name);
                let outline_range = Range::for_tree_node(&capture.node);
                if !range_set.contains(&outline_range) {
                    if let Some(outline_name) = outline_name {
                        outline_nodes.push((outline_name, outline_range));
                        range_set.insert(outline_range);
                    }
                }
            });
        });

        let mut start_index = 0;
        let source_code_vec = source_code.to_vec();
        let lines: Vec<String> = String::from_utf8(source_code_vec.to_vec())
            .expect("utf8-parsing to work")
            .lines()
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        let lines_slice = lines.as_slice();
        let mut independent_functions_for_class: HashMap<String, Vec<OutlineNodeContent>> =
            Default::default();
        let mut decorator_range: Option<Range> = None;
        while start_index < outline_nodes.len() {
            // cheap clone so this is fine
            let (outline_node_type, outline_range) = outline_nodes[start_index].clone();
            match outline_node_type {
                OutlineNodeType::Class | OutlineNodeType::ClassDefinition => {
                    // If we are in a class, we might have functions or class names
                    // which we want to parse out
                    let mut end_index = start_index + 1;
                    let mut class_name = None;
                    let mut class_name_range = None;
                    let mut function_range = None;
                    let mut function_name = None;
                    let mut function_name_range = None;
                    let mut function_class_name = None;
                    let mut class_implementation_trait = None;
                    let mut children = vec![];
                    while end_index < outline_nodes.len() {
                        let (child_node_type, child_range) = outline_nodes[end_index].clone();
                        if !outline_range.contains(&child_range) {
                            // This is not required as we are breaking the loop over here
                            // start_index = end_index;
                            break;
                        }
                        match child_node_type {
                            OutlineNodeType::ClassTrait => {
                                if class_implementation_trait.is_none() {
                                    class_implementation_trait = Some(get_string_from_bytes(
                                        &source_code_vec,
                                        child_range.start_byte(),
                                        child_range.end_byte(),
                                    ));
                                }
                            }
                            OutlineNodeType::ClassName => {
                                // we might have inner classes inside the same class
                                // its best to avoid tracking them again
                                if class_name.is_none() {
                                    class_name = Some(get_string_from_bytes(
                                        &source_code_vec,
                                        child_range.start_byte(),
                                        child_range.end_byte(),
                                    ));
                                    class_name_range = Some(child_range);
                                }
                            }
                            OutlineNodeType::Function => {
                                if self.language_str == "python" && function_range.is_some() {
                                    // do nothing in this case, since we have the decorator
                                    // query over here which will also emit the same event for
                                    // us
                                } else {
                                    function_range = Some(child_range);
                                }
                            }
                            OutlineNodeType::FunctionName => {
                                if function_range.is_some() {
                                    let current_function_name = get_string_from_bytes(
                                        &source_code_vec,
                                        child_range.start_byte(),
                                        child_range.end_byte(),
                                    );
                                    function_name_range = Some(child_range.clone());
                                    function_name = Some(current_function_name);
                                }
                            }
                            OutlineNodeType::FunctionClassName => {
                                if let Some(_) = function_range {
                                    let current_function_class_name = get_string_from_bytes(
                                        &source_code_vec,
                                        child_range.start_byte(),
                                        child_range.end_byte(),
                                    );
                                    function_class_name = Some(current_function_class_name);
                                }
                            }
                            OutlineNodeType::FunctionBody => {
                                if let (
                                    Some(function_range),
                                    Some(function_name),
                                    Some(function_name_range),
                                ) = (function_range, function_name, function_name_range)
                                {
                                    if let Some(function_class_name) = function_class_name {
                                        let class_functions = independent_functions_for_class
                                            .entry(function_class_name)
                                            .or_insert_with(|| vec![]);
                                        class_functions.push(OutlineNodeContent::new(
                                            function_name,
                                            function_range,
                                            OutlineNodeType::Function,
                                            // grab it using the lines since we want
                                            // the proper prefix before the function start
                                            // as indentation is important
                                            get_string_from_lines(
                                                lines_slice,
                                                function_range.start_line(),
                                                child_range.end_line(),
                                            ),
                                            fs_file_path.to_owned(),
                                            function_name_range,
                                            child_range,
                                            self.language_str.to_owned(),
                                            None,
                                        ));
                                    } else {
                                        children.push(OutlineNodeContent::new(
                                            function_name,
                                            function_range,
                                            OutlineNodeType::Function,
                                            get_string_from_lines(
                                                lines_slice,
                                                function_range.start_line(),
                                                child_range.end_line(),
                                            ),
                                            fs_file_path.to_owned(),
                                            function_name_range,
                                            child_range,
                                            self.language_str.to_owned(),
                                            None,
                                        ));
                                    }
                                }
                                function_class_name = None;
                                function_range = None;
                                function_name = None;
                            }
                            OutlineNodeType::Class => {
                                // can not have a class inside another class
                            }
                            OutlineNodeType::FunctionParameterIdentifier => {
                                // we need to track this as well
                            }
                            OutlineNodeType::ClassDefinition => {
                                // can not have another class definition inside
                                // it
                            }
                            OutlineNodeType::Decorator => {
                                // if its a decorator we just skip for now
                            }
                            OutlineNodeType::DefinitionAssignment => {
                                // if this is a definition assignment we just skip for now
                            }
                            OutlineNodeType::DefinitionIdentifier => {
                                // if this is a definition identifer we are not interested in this for
                                // now so keep skipping
                            }
                            OutlineNodeType::File => {
                                // we want to not do anything if its a file over here
                            }
                        }
                        end_index = end_index + 1;
                    }
                    // if we have a decorator start the body from there
                    // this allows us to capture decorations on to of the class
                    // symbols
                    let class_range = if let Some(decorator_range) = decorator_range {
                        Range::new(
                            decorator_range.start_position(),
                            outline_range.end_position(),
                        )
                    } else {
                        outline_range
                    };
                    // reset the decorator range
                    decorator_range = None;
                    let class_outline = OutlineNodeContent::new(
                        class_name.expect("class name to be present"),
                        class_range,
                        outline_node_type,
                        get_string_from_bytes(
                            &source_code_vec,
                            class_range.start_byte(),
                            class_range.end_byte(),
                        ),
                        fs_file_path.to_owned(),
                        class_name_range.expect("class name range to be present"),
                        // This is incorrect
                        class_range,
                        self.language_str.to_owned(),
                        class_implementation_trait.clone(),
                    );
                    compressed_outline.push(OutlineNode::new(
                        class_outline,
                        children,
                        self.language_str.to_owned(),
                    ));
                    start_index = end_index;
                }
                OutlineNodeType::Function => {
                    // If the outline is a function, then we just want to grab the
                    // next node which is a function name
                    let mut end_index = start_index + 1;
                    let mut function_name: Option<String> = None;
                    let mut function_range = None;
                    let mut function_class_name: Option<String> = None;
                    while end_index < outline_nodes.len() {
                        let (child_node_type, child_range) = outline_nodes[end_index].clone();
                        if !outline_range.contains(&child_range) {
                            break;
                        }
                        if let OutlineNodeType::FunctionName = child_node_type {
                            function_name = Some(get_string_from_bytes(
                                &source_code_vec,
                                child_range.start_byte(),
                                child_range.end_byte(),
                            ));
                            function_range = Some(child_range);
                            end_index = end_index + 1;
                        } else if let OutlineNodeType::FunctionClassName = child_node_type {
                            function_class_name = Some(get_string_from_bytes(
                                &source_code_vec,
                                child_range.start_byte(),
                                child_range.end_byte(),
                            ));
                            end_index = end_index + 1;
                        } else if let OutlineNodeType::FunctionBody = child_node_type {
                            if let (Some(ref function_name), Some(ref function_name_range)) =
                                (&function_name, function_range)
                            {
                                if let Some(ref function_class_name) = function_class_name {
                                    let class_functions = independent_functions_for_class
                                        .entry(function_class_name.to_owned())
                                        .or_insert_with(|| vec![]);
                                    class_functions.push(OutlineNodeContent::new(
                                        function_name.to_owned(),
                                        outline_range,
                                        OutlineNodeType::Function,
                                        // we get the string using lines since
                                        // we also want the prefix on the line
                                        // where the function is starting
                                        get_string_from_lines(
                                            lines_slice,
                                            outline_range.start_line(),
                                            child_range.end_line(),
                                        ),
                                        fs_file_path.to_owned(),
                                        function_name_range.clone(),
                                        child_range,
                                        self.language_str.to_owned(),
                                        None,
                                    ));
                                } else {
                                    compressed_outline.push(OutlineNode::new(
                                        OutlineNodeContent::new(
                                            function_name.to_owned(),
                                            outline_range,
                                            OutlineNodeType::Function,
                                            // we get the string using lines since
                                            // we also want the prefix on the line
                                            // where the function is starting
                                            get_string_from_lines(
                                                lines_slice,
                                                outline_range.start_line(),
                                                // since this is a function, we want
                                                // the complete range of the function here
                                                // and not just the child range end which would
                                                // be the function body
                                                outline_range.end_line(),
                                            ),
                                            fs_file_path.to_owned(),
                                            function_name_range.clone(),
                                            child_range,
                                            self.language_str.to_owned(),
                                            None,
                                        ),
                                        vec![],
                                        self.language_str.to_owned(),
                                    ));
                                }
                            }
                            end_index = end_index + 1;
                        } else if let OutlineNodeType::FunctionParameterIdentifier = child_node_type
                        {
                            end_index = end_index + 1;
                        } else if let OutlineNodeType::DefinitionAssignment = child_node_type {
                            end_index = end_index + 1;
                        } else if let OutlineNodeType::DefinitionIdentifier = child_node_type {
                            end_index = end_index + 1;
                        } else {
                            break;
                        }
                    }
                    start_index = end_index;
                }
                OutlineNodeType::FunctionName => {
                    start_index = start_index + 1;
                    // If the outline is just a function name, then we are again fucked :)
                }
                OutlineNodeType::ClassName => {
                    start_index = start_index + 1;
                    // If the outline is just a class, name we are totally fucked :)
                }
                OutlineNodeType::FunctionBody => {
                    start_index = start_index + 1;
                    // If the outline is just a function body, then we are totally fucked :)
                }
                OutlineNodeType::FunctionClassName => {
                    start_index = start_index + 1;
                    // If the outline is just a function class name, then we are totally fucked :)
                }
                OutlineNodeType::FunctionParameterIdentifier => {
                    start_index = start_index + 1;
                    // we want to track this going on
                }
                OutlineNodeType::Decorator => {
                    // Sets the decorator range over here
                    decorator_range = Some(outline_range);
                    start_index = start_index + 1;
                    // we are not going to track the decorators right now, we will figure out
                    // what to do about decorators in a bit
                }
                OutlineNodeType::DefinitionAssignment => {
                    let end_index = start_index + 1;
                    while end_index < outline_nodes.len() {
                        let next_node = outline_nodes[end_index].clone();
                        if next_node.0 == OutlineNodeType::DefinitionIdentifier {
                            compressed_outline.push(OutlineNode::new(
                                OutlineNodeContent::new(
                                    get_string_from_bytes(
                                        &source_code_vec,
                                        next_node.1.start_byte(),
                                        next_node.1.end_byte(),
                                    ),
                                    outline_nodes[start_index].1.clone(),
                                    OutlineNodeType::DefinitionAssignment,
                                    get_string_from_lines(
                                        lines_slice,
                                        outline_nodes[start_index].1.start_line(),
                                        outline_nodes[start_index].1.end_line(),
                                    ),
                                    fs_file_path.to_owned(),
                                    next_node.1.clone(),
                                    outline_nodes[start_index].1.clone(),
                                    self.language_str.to_owned(),
                                    None,
                                ),
                                vec![],
                                self.language_str.to_owned(),
                            ));
                        }
                        break;
                    }
                    start_index = end_index;
                    // the immediate next node after this is a definition identifier which is inside
                    // the definition assignment
                    // we are not tracking the definition assignment right now, we will figure
                    // out what to do about this in a bit
                }
                OutlineNodeType::DefinitionIdentifier => {
                    start_index = start_index + 1;
                    // we are not tracking the definition identifier globally for now, we will
                    // figure out what to do about this in a bit
                }
                OutlineNodeType::ClassTrait => {
                    start_index = start_index + 1;
                }
                OutlineNodeType::File => {
                    // skipping this one, we are not tracking this
                    start_index = start_index + 1;
                }
            }
        }

        // Now at the very end we check for our map which contains functions which might be
        // part of the class because they say so and we want to include them as children
        let mut result = compressed_outline
            .into_iter()
            .map(|mut outline_node| {
                if !outline_node.is_class() {
                    outline_node
                } else {
                    let relevant_children =
                        independent_functions_for_class.remove(outline_node.name());
                    if let Some(relevant_children) = relevant_children {
                        outline_node.add_children(relevant_children);
                    }
                    outline_node
                }
            })
            .collect::<Vec<_>>();

        if result.is_empty() {
            // If no nodes were found, create a single File node
            result.push(OutlineNode::new(
                OutlineNodeContent::new(
                    fs_file_path.clone(),
                    Range::new(
                        Position::new(0, 0, 0),
                        Position::new(lines.len(), 0, source_code.len()),
                    ),
                    OutlineNodeType::File,
                    String::from_utf8_lossy(source_code).to_string(),
                    fs_file_path,
                    Range::new(Position::new(0, 0, 0), Position::new(0, 0, 0)),
                    Range::new(
                        Position::new(0, 0, 0),
                        Position::new(lines.len(), 0, source_code.len()),
                    ),
                    self.language_str.to_owned(),
                    None,
                ),
                vec![],
                self.language_str.to_owned(),
            ));
        }

        result
    }

    /// Generates the function call paths completely so we can use that for better
    /// tactics when trying to handle or preempt some kind of errors which can arise
    /// when the LLM is writing code
    pub fn generate_function_call_paths(&self, source_code: &[u8]) -> Option<Vec<(String, Range)>> {
        let grammar = self.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(grammar())
            .expect("for lanaguage parser from tree_sitter should not fail");
        let tree = parser.parse(source_code, None).unwrap();
        let function_call_path = self.function_call_path.to_owned();
        let node = tree.root_node();
        if function_call_path.is_none() {
            return None;
        }
        let function_call_path_query = function_call_path.expect("is_none to hold");
        let query = tree_sitter::Query::new(grammar(), &function_call_path_query).expect("to work");
        let mut cursor = tree_sitter::QueryCursor::new();
        let query_captures = cursor.captures(&query, node, source_code);
        let mut function_call_paths: Vec<(String, Range)> = vec![];
        let mut range_set: HashSet<Range> = Default::default();
        let source_code_vec = source_code.to_vec();
        query_captures.into_iter().for_each(|capture| {
            capture.0.captures.into_iter().for_each(|capture| {
                let range = Range::for_tree_node(&capture.node);
                let node_name =
                    get_string_from_bytes(&source_code_vec, range.start_byte(), range.end_byte());
                if !range_set.contains(&range) {
                    range_set.insert(range);
                    function_call_paths.push((node_name, range));
                }
            })
        });
        Some(function_call_paths)
    }

    /// Generate the return types and the function parameters which we can go-to-definition
    /// on to learn more about the types
    pub fn generate_function_insights(&self, source_code: &[u8]) -> Vec<(String, Range)> {
        let grammar = self.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(grammar())
            .expect("for lanaguage parser from tree_sitter should not fail");
        let tree = parser.parse(source_code, None).unwrap();
        let required_parameter_types_for_functions =
            self.required_parameter_types_for_functions.to_owned();
        let node = tree.root_node();
        let query = tree_sitter::Query::new(grammar(), &required_parameter_types_for_functions)
            .expect("to work");
        let mut cursor = tree_sitter::QueryCursor::new();
        let query_captures = cursor.captures(&query, node, source_code);
        let mut function_clickable_insights: Vec<(String, Range)> = vec![];
        let mut range_set: HashSet<Range> = Default::default();
        let source_code_vec = source_code.to_vec();
        query_captures.into_iter().for_each(|capture| {
            capture.0.captures.into_iter().for_each(|capture| {
                let range = Range::for_tree_node(&capture.node);
                let node_name =
                    get_string_from_bytes(&source_code_vec, range.start_byte(), range.end_byte());
                if !range_set.contains(&range) {
                    range_set.insert(range);
                    function_clickable_insights.push((node_name, range));
                }
            })
        });
        function_clickable_insights
    }

    /// This function generates the tree by parsing the source code and can be
    /// used when we do not have the tree sitter tree already created
    pub fn generate_import_identifiers_fresh(&self, source_code: &[u8]) -> Vec<(String, Range)> {
        let grammar = self.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(grammar())
            .expect("for lanaguage parser from tree_sitter should not fail");
        let tree = parser.parse(source_code, None).unwrap();
        self.generate_import_identifier_nodes(source_code, &tree)
    }

    pub fn generate_import_identifier_nodes(
        &self,
        source_code: &[u8],
        tree: &Tree,
    ) -> Vec<(String, Range)> {
        let grammar = self.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let import_identifier_query = self.import_identifier_queries.to_owned();
        let node = tree.root_node();
        let query = tree_sitter::Query::new(grammar(), &import_identifier_query).expect("to work");
        let mut cursor = tree_sitter::QueryCursor::new();
        let query_captures = cursor.captures(&query, node, source_code);
        let mut import_identifier_nodes: Vec<(String, Range)> = vec![];
        let mut range_set: HashSet<Range> = Default::default();
        let source_code_vec = source_code.to_vec();
        query_captures.into_iter().for_each(|capture| {
            capture.0.captures.into_iter().for_each(|capture| {
                let range = Range::for_tree_node(&capture.node);
                let node_name =
                    get_string_from_bytes(&source_code_vec, range.start_byte(), range.end_byte());
                if !range_set.contains(&range) {
                    range_set.insert(range);
                    import_identifier_nodes.push((node_name, range));
                }
            })
        });
        import_identifier_nodes
    }

    pub fn generate_identifier_nodes(
        &self,
        source_code: &str,
        position: Position,
        old_tree: Option<&Tree>,
    ) -> HashMap<String, Range> {
        // First we want to get the function nodes which are present in the file
        // Then we are going to check if our position belongs in the range of any
        // function. If it does, we will invoke the query to get the identifier nodes
        // within that function above that position
        let function_ranges = match old_tree {
            Some(tree) => self.capture_function_data_with_tree(source_code.as_bytes(), tree, true),
            None => self.capture_function_data(source_code.as_bytes()),
        };
        let possible_function = function_ranges
            .into_iter()
            .filter(|function_information| {
                function_information.range().contains_position(&position)
            })
            .next();

        if let Some(possible_function) = possible_function {
            // Now we need to find the identifier nodes in this part of the function and then grab them
            // and return them, this function might need to be enclosed in either a class or can be independent
            // so we will have to check for both
            let _function_body = possible_function.content(source_code);
            Default::default()
        } else {
            Default::default()
        }
    }

    pub fn generate_file_symbols(&self, source_code: &[u8]) -> Vec<ClassWithFunctions> {
        let function_ranges = self.capture_function_data(source_code);
        let class_ranges = self.capture_class_data(source_code);
        let mut classes_with_functions = Vec::new();
        let mut standalone_functions = Vec::new();

        // This is where we maintain the list of functions which we have already
        // added to a class
        let mut added_functions = vec![false; function_ranges.len()];

        for class in class_ranges {
            let mut functions = Vec::new();

            for (i, function) in function_ranges.iter().enumerate() {
                if (function.range().start_byte() >= class.range().start_byte()
                    && function.range().end_byte() <= class.range().end_byte()
                    && function.get_node_information().is_some())
                    || function
                        .class_name()
                        .map(|func_class_name| func_class_name == class.get_name())
                        .unwrap_or(false)
                {
                    functions.push(function.clone());
                    added_functions[i] = true; // Mark function as added
                }
            }

            classes_with_functions.push(ClassWithFunctions::class_functions(class, functions));
        }

        // Add standalone functions, those which are not within any class range
        for (i, function) in function_ranges.iter().enumerate() {
            if !added_functions[i] && function.get_node_information().is_some() {
                standalone_functions.push(function.clone());
            }
        }

        classes_with_functions.push(ClassWithFunctions::functions(standalone_functions));
        classes_with_functions
    }

    // The file outline looks like this:
    // function something(arguments): return_value_something
    // Class something_else
    //    function inner_function(arguments_here): return_value_function
    //    function something_else(arguments_here): return_value_something_here
    // ...
    // We will generate a proper outline later on, but for now work with this
    // TODO(skcd): This can be greatly improved here
    pub fn generate_file_outline_str(&self, source_code: &[u8]) -> String {
        let function_ranges = self.capture_function_data(source_code);
        let class_ranges = self.capture_class_data(source_code);
        let language = self
            .get_language()
            .expect("to have some language")
            .to_lowercase();
        let mut outline = format!("```{language}\n");

        // This is where we maintain the list of functions which we have already
        // printed out
        let mut printed_functions = vec![false; function_ranges.len()];

        for class in class_ranges {
            let class_name = class.get_name();
            outline = outline + "\n" + &format!("Class {class_name}") + "\n";
            // Find and print functions within the class range
            for (i, function) in function_ranges.iter().enumerate() {
                if function.range().start_byte() >= class.range().start_byte()
                    && function.range().end_byte() <= class.range().end_byte()
                    && function.get_node_information().is_some()
                {
                    let node_information = function
                        .get_node_information()
                        .expect("AND check above to hold");
                    outline = outline
                        + "\n"
                        + &format!(
                            "    function {} {} {}",
                            node_information.get_name(),
                            node_information.get_parameters(),
                            node_information.get_return_type()
                        );
                    printed_functions[i] = true; // Mark function as printed
                }
            }
        }

        // Print standalone functions, those which are not within any class range
        for (i, function) in function_ranges.iter().enumerate() {
            if !printed_functions[i] && function.get_node_information().is_some() {
                let node_information = function
                    .get_node_information()
                    .expect("AND check above to hold");
                // Check if the function has not been printed yet
                outline = outline
                    + "\n"
                    + &format!(
                        "function {} {} {}",
                        node_information.get_name(),
                        node_information.get_parameters(),
                        node_information.get_return_type()
                    )
                    + "\n";
            }
        }

        outline = outline + "\n" + "```";
        outline
    }

    pub fn capture_documentation_queries(&self, source_code: &[u8]) -> Vec<(Range, String)> {
        // Now we try to grab the documentation strings so we can add them to the functions as well
        let mut parser = tree_sitter::Parser::new();
        let grammar = self.grammar;
        parser.set_language(grammar()).unwrap();
        let parsed_data = parser.parse(source_code, None).unwrap();
        let node = parsed_data.root_node();
        let mut range_set = HashSet::new();
        let documentation_queries = self.documentation_query.to_vec();
        let source_code_vec = source_code.to_vec();
        // We want to capture here the range of the comment line and the comment content
        // we can then concat this with the function itself and expand te range of the function
        // node so it covers this comment as well
        let mut documentation_string_information: Vec<(Range, String)> = vec![];
        documentation_queries
            .into_iter()
            .for_each(|documentation_query| {
                let query = tree_sitter::Query::new(grammar(), &documentation_query)
                    .expect("documentation queries are well formed");
                let mut cursor = tree_sitter::QueryCursor::new();
                cursor
                    .captures(&query, node, source_code)
                    .into_iter()
                    .for_each(|capture| {
                        capture.0.captures.into_iter().for_each(|capture| {
                            if !range_set.contains(&Range::for_tree_node(&capture.node)) {
                                let documentation_string = get_string_from_bytes(
                                    &source_code_vec,
                                    capture.node.start_byte(),
                                    capture.node.end_byte(),
                                );
                                documentation_string_information.push((
                                    Range::for_tree_node(&capture.node),
                                    documentation_string,
                                ));
                                range_set.insert(Range::for_tree_node(&capture.node));
                            }
                        })
                    });
            });
        documentation_string_information
    }

    pub fn get_tree_sitter_tree(&self, source_code: &[u8]) -> Option<Tree> {
        let grammar = self.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        parser.parse(source_code, None)
    }

    pub fn capture_type_data(&self, source_code: &[u8]) -> Vec<TypeInformation> {
        let type_queries = self.type_query.to_vec();

        let grammar = self.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let parsed_data = parser.parse(source_code, None).unwrap();
        let node = parsed_data.root_node();

        let mut type_nodes = vec![];
        let mut range_set = HashSet::new();
        type_queries.into_iter().for_each(|type_query| {
            let query = tree_sitter::Query::new(grammar(), &type_query)
                .expect("type queries are well formed");
            let mut cursor = tree_sitter::QueryCursor::new();
            cursor
                .captures(&query, node, source_code)
                .into_iter()
                .for_each(|capture| {
                    capture.0.captures.into_iter().for_each(|capture| {
                        let capture_name = query
                            .capture_names()
                            .to_vec()
                            .remove(capture.index.try_into().unwrap());
                        let capture_type = TypeNodeType::from_str(&capture_name);
                        if !range_set.contains(&Range::for_tree_node(&capture.node)) {
                            if let Some(capture_type) = capture_type {
                                if capture_type == TypeNodeType::TypeDeclaration {
                                    // if we have the type declaration here, we want to check if
                                    // we should go to the parent of this node and check if its
                                    // an export stament here, since if that's the case
                                    // we want to handle that too
                                    let parent_node = capture.node.parent();
                                    if let Some(parent_node) = parent_node {
                                        if self
                                            .namespace_types
                                            .contains(&parent_node.kind().to_owned())
                                        {
                                            type_nodes.push(TypeInformation::new(
                                                Range::for_tree_node(&parent_node),
                                                "not_set_parent_node".to_owned(),
                                                capture_type,
                                            ));
                                            // to the range set we add the range of the current capture node
                                            range_set.insert(Range::for_tree_node(&capture.node));
                                            return;
                                        }
                                    }
                                }
                                type_nodes.push(TypeInformation::new(
                                    Range::for_tree_node(&capture.node),
                                    "not_set".to_owned(),
                                    capture_type,
                                ));
                                range_set.insert(Range::for_tree_node(&capture.node));
                            }
                        }
                    })
                })
        });

        // Now we iterate again and try to get the name of the types as well
        // and generate the final representation
        // the nodes are ordered in this way:
        // type_node
        // - identifier
        let mut index = 0;
        let mut compressed_types = vec![];
        while index < type_nodes.len() {
            let start_index = index;
            if type_nodes[start_index].get_type_type() != &TypeNodeType::TypeDeclaration {
                index += 1;
                continue;
            }
            compressed_types.push(type_nodes[start_index].clone());
            let mut end_index = start_index + 1;
            let mut type_identifier = None;
            while end_index < type_nodes.len()
                && type_nodes[end_index].get_type_type() != &TypeNodeType::TypeDeclaration
            {
                match type_nodes[end_index].get_type_type() {
                    TypeNodeType::Identifier => {
                        type_identifier = Some(get_string_from_bytes(
                            &source_code.to_vec(),
                            type_nodes[end_index].range().start_byte(),
                            type_nodes[end_index].range().end_byte(),
                        ));
                    }
                    _ => {}
                }
                end_index += 1;
            }

            match (compressed_types.last_mut(), type_identifier) {
                (Some(type_information), Some(type_name)) => {
                    type_information.set_name(type_name);
                }
                _ => {}
            }
            index = end_index;
        }
        let documentation_strings = self.capture_documentation_queries(source_code);
        TypeInformation::add_documentation_to_types(compressed_types, documentation_strings)
    }

    pub fn capture_class_data(&self, source_code: &[u8]) -> Vec<ClassInformation> {
        let class_queries = self.class_query.to_vec();

        let grammar = self.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let parsed_data = parser.parse(source_code, None).unwrap();
        let node = parsed_data.root_node();

        let mut class_nodes = vec![];
        let class_code_vec = source_code.to_vec();
        let mut range_set = HashSet::new();
        class_queries.into_iter().for_each(|class_query| {
            let query = tree_sitter::Query::new(grammar(), &class_query)
                .expect("class queries are well formed");
            let mut cursor = tree_sitter::QueryCursor::new();
            cursor
                .captures(&query, node, source_code)
                .into_iter()
                .for_each(|capture| {
                    capture.0.captures.into_iter().for_each(|capture| {
                        let capture_name = query
                            .capture_names()
                            .to_vec()
                            .remove(capture.index.try_into().unwrap());
                        let capture_type = ClassNodeType::from_str(&capture_name);
                        if !range_set.contains(&Range::for_tree_node(&capture.node)) {
                            if let Some(capture_type) = capture_type {
                                // if we have the type declaration here, we want to check if
                                // we should go to the parent of this node and check if its
                                // an export stament here, since if that's the case
                                // we want to handle that too
                                let parent_node = capture.node.parent();
                                if let Some(parent_node) = parent_node {
                                    if self
                                        .namespace_types
                                        .contains(&parent_node.kind().to_owned())
                                    {
                                        class_nodes.push(ClassInformation::new(
                                            Range::for_tree_node(&capture.node),
                                            "not_set_parent".to_owned(),
                                            capture_type,
                                        ));
                                        // to the range set we add the range of the current capture node
                                        range_set.insert(Range::for_tree_node(&capture.node));
                                        return;
                                    }
                                };
                                class_nodes.push(ClassInformation::new(
                                    Range::for_tree_node(&capture.node),
                                    "not_set".to_owned(),
                                    capture_type,
                                ));
                                range_set.insert(Range::for_tree_node(&capture.node));
                            }
                        }
                    })
                })
        });

        // Now we iterate again and try to get the name of the classes as well
        // and generate the final representation
        // the nodes are ordered in this way:
        // class
        // - identifier
        let mut index = 0;
        let mut compressed_classes = vec![];
        while index < class_nodes.len() {
            let start_index = index;
            if class_nodes[start_index].get_class_type() != &ClassNodeType::ClassDeclaration {
                index += 1;
                continue;
            }
            compressed_classes.push(class_nodes[start_index].clone());
            let mut end_index = start_index + 1;
            let mut class_identifier = None;
            while end_index < class_nodes.len()
                && class_nodes[end_index].get_class_type() != &ClassNodeType::ClassDeclaration
            {
                match class_nodes[end_index].get_class_type() {
                    ClassNodeType::Identifier => {
                        class_identifier = Some(get_string_from_bytes(
                            &class_code_vec,
                            class_nodes[end_index].range().start_byte(),
                            class_nodes[end_index].range().end_byte(),
                        ));
                    }
                    _ => {}
                }
                end_index += 1;
            }

            match (compressed_classes.last_mut(), class_identifier) {
                (Some(class_information), Some(class_name)) => {
                    class_information.set_name(class_name);
                }
                _ => {}
            }
            index = end_index;
        }
        let documentation_string_information: Vec<(Range, String)> =
            self.capture_documentation_queries(source_code);
        ClassInformation::add_documentation_to_classes(
            compressed_classes,
            documentation_string_information,
        )
    }

    pub fn capture_identifier_nodes(
        &self,
        source_code: &[u8],
        tree: &Tree,
    ) -> Vec<(String, Range)> {
        let identifier_nodes_query = self.variable_identifier_queries.to_vec();
        let grammar = self.grammar;
        let node = tree.root_node();
        let mut identifier_nodes = vec![];
        let mut range_set: HashSet<Range> = HashSet::new();
        let source_code_vec = source_code.to_vec();
        identifier_nodes_query
            .into_iter()
            .for_each(|identifier_query| {
                let query = tree_sitter::Query::new(grammar(), &identifier_query)
                    .expect("identifier queries to be well formed");
                let mut cursor = tree_sitter::QueryCursor::new();
                cursor
                    .captures(&query, node, source_code)
                    .into_iter()
                    .for_each(|capture| {
                        capture.0.captures.into_iter().for_each(|capture| {
                            let capture_range = Range::for_tree_node(&capture.node);
                            let node_name = get_string_from_bytes(
                                &source_code_vec,
                                capture_range.start_byte(),
                                capture_range.end_byte(),
                            );
                            if !range_set.contains(&capture_range) {
                                range_set.insert(capture_range.clone());
                                identifier_nodes.push((node_name, capture_range));
                            }
                        })
                    })
            });
        identifier_nodes
    }

    pub fn capture_function_data_with_tree(
        &self,
        source_code: &[u8],
        tree: &Tree,
        only_outline: bool,
    ) -> Vec<FunctionInformation> {
        let function_queries = self.function_query.to_vec();
        // We want to capture the function information here and then do a folding on top of
        // it, we just want to keep top level functions over here
        // Now we need to run the tree sitter query on this and get back the
        // answer
        let grammar = self.grammar;
        let node = tree.root_node();
        let mut function_nodes = vec![];
        let mut range_set = HashSet::new();
        function_queries.into_iter().for_each(|function_query| {
            let query = tree_sitter::Query::new(grammar(), &function_query)
                .expect("function queries are well formed");
            let mut cursor = tree_sitter::QueryCursor::new();
            cursor
                .captures(&query, node, source_code)
                .into_iter()
                .for_each(|capture| {
                    capture.0.captures.into_iter().for_each(|capture| {
                        let capture_name = query
                            .capture_names()
                            .to_vec()
                            .remove(capture.index.try_into().unwrap());
                        let capture_type = FunctionNodeType::from_str(&capture_name);
                        if !range_set.contains(&Range::for_tree_node(&capture.node)) {
                            if let Some(capture_type) = capture_type {
                                if capture_type == FunctionNodeType::Function {
                                    // if we have the type declaration here, we want to check if
                                    // we should go to the parent of this node and check if its
                                    // an export stament here, since if that's the case
                                    // we want to handle that too
                                    let parent_node = capture.node.parent();
                                    if let Some(parent_node) = parent_node {
                                        if self
                                            .namespace_types
                                            .contains(&parent_node.kind().to_owned())
                                        {
                                            function_nodes.push(FunctionInformation::new(
                                                Range::for_tree_node(&parent_node),
                                                capture_type,
                                            ));
                                            // to the range set we add the range of the current capture node
                                            range_set.insert(Range::for_tree_node(&capture.node));
                                            return;
                                        }
                                    }
                                }
                                function_nodes.push(FunctionInformation::new(
                                    Range::for_tree_node(&capture.node),
                                    capture_type,
                                ));
                                range_set.insert(Range::for_tree_node(&capture.node));
                            }
                        }
                    })
                });
        });

        // Now we know from the query, that we have to do the following:
        // function
        // - identifier
        // - body
        // - parameters
        // - return
        let mut index = 0;
        let source_code_vec = source_code.to_vec();
        let mut compressed_functions = vec![];
        while index < function_nodes.len() {
            let start_index = index;
            if function_nodes[start_index].r#type() != &FunctionNodeType::Function {
                index += 1;
                continue;
            }
            compressed_functions.push(function_nodes[start_index].clone());
            let mut end_index = start_index + 1;
            let mut function_node_information = FunctionNodeInformation::default();
            while end_index < function_nodes.len()
                && function_nodes[end_index].r#type() != &FunctionNodeType::Function
            {
                match function_nodes[end_index].r#type() {
                    &FunctionNodeType::Identifier => {
                        function_node_information.set_name(if !only_outline {
                            get_string_from_bytes(
                                &source_code_vec,
                                function_nodes[end_index].range().start_byte(),
                                function_nodes[end_index].range().end_byte(),
                            )
                        } else {
                            "".to_owned()
                        });
                    }
                    &FunctionNodeType::Body => {
                        function_node_information.set_body(if !only_outline {
                            get_string_from_bytes(
                                &source_code_vec,
                                function_nodes[end_index].range().start_byte(),
                                function_nodes[end_index].range().end_byte(),
                            )
                        } else {
                            "".to_owned()
                        });
                    }
                    &FunctionNodeType::Parameters => {
                        function_node_information.set_parameters(if !only_outline {
                            get_string_from_bytes(
                                &source_code_vec,
                                function_nodes[end_index].range().start_byte(),
                                function_nodes[end_index].range().end_byte(),
                            )
                        } else {
                            "".to_owned()
                        });
                    }
                    &FunctionNodeType::ReturnType => {
                        function_node_information.set_return_type(if !only_outline {
                            get_string_from_bytes(
                                &source_code_vec,
                                function_nodes[end_index].range().start_byte(),
                                function_nodes[end_index].range().end_byte(),
                            )
                        } else {
                            "".to_owned()
                        });
                    }
                    &FunctionNodeType::ClassName => {
                        function_node_information.set_class_name(get_string_from_bytes(
                            &source_code_vec,
                            function_nodes[end_index].range().start_byte(),
                            function_nodes[end_index].range().end_byte(),
                        ))
                    }
                    &FunctionNodeType::ParameterIdentifier => function_node_information
                        .add_parameter_identifier(
                            get_string_from_bytes(
                                &source_code_vec,
                                function_nodes[end_index].range().start_byte(),
                                function_nodes[end_index].range().end_byte(),
                            ),
                            function_nodes[end_index].range().clone(),
                        ),
                    _ => {}
                }
                end_index += 1;
            }

            match compressed_functions.last_mut() {
                Some(function_information) => {
                    function_information.set_node_information(function_node_information);
                }
                None => {}
            }
            index = end_index;
        }

        let documentation_string_information: Vec<(Range, String)> =
            self.capture_documentation_queries(source_code);
        let identifier_nodes = self.capture_identifier_nodes(source_code, tree);
        // Add the identifier nodes as well
        FunctionInformation::add_identifier_nodes(
            // Now we want to append the documentation string to the functions
            FunctionInformation::add_documentation_to_functions(
                FunctionInformation::fold_function_blocks(compressed_functions),
                documentation_string_information,
            ),
            identifier_nodes,
        )
    }

    pub fn capture_function_data(&self, source_code: &[u8]) -> Vec<FunctionInformation> {
        let grammar = self.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let parsed_data = parser.parse(source_code, None).unwrap();
        self.capture_function_data_with_tree(source_code, &parsed_data, false)
    }

    // TODO: get_tags cache

    // get tags for a given file
    pub async fn get_tags(
        &self,
        fname: &PathBuf,
        rel_fname: &PathBuf,
        file_content: Vec<u8>,
    ) -> Vec<Tag> {
        let tree = match self.get_tree_sitter_tree(file_content.as_slice()) {
            Some(tree) => tree,
            None => {
                eprintln!(
                    "Error: Failed to get tree-sitter tree for: {}",
                    fname.display()
                );
                return vec![];
            }
        };

        let root_node = tree.root_node();
        let grammar = self.grammar;
        let query = tree_sitter::Query::new(grammar(), &self.file_definitions_query)
            .expect("file definitions queries to be well formed");

        let mut cursor = tree_sitter::QueryCursor::new();

        let captures = cursor.captures(&query, root_node, file_content.as_slice());

        captures
            .filter_map(|(match_, capture_index)| {
                let capture = &match_.captures[capture_index]; // the specific capture we're interested in, as opposed to other captures in the match

                // A 'match' represents a successful pattern match from our query, potentially containing multiple captures
                let tag_name = &query.capture_names()[capture.index as usize];
                let node = capture.node;

                // todo - consider
                let line: usize = node.start_position().row + 1; // line numbers are 1-indexed
                match tag_name {
                    name if name.starts_with("name.definition.") => Some(Tag::new(
                        rel_fname.clone(),
                        fname.clone(),
                        line,
                        get_string_from_bytes(&file_content, node.start_byte(), node.end_byte()),
                        TagKind::Definition,
                    )),
                    name if name.starts_with("name.reference.") => Some(Tag::new(
                        rel_fname.clone(),
                        fname.clone(),
                        line,
                        get_string_from_bytes(&file_content, node.start_byte(), node.end_byte()),
                        TagKind::Reference,
                    )),
                    _ => None,
                }
            })
            .collect()
    }

    pub fn function_information_nodes(&self, source_code: &[u8]) -> Vec<FunctionInformation> {
        let function_queries = self.function_query.to_vec();

        // Now we need to run the tree sitter query on this and get back the
        // answer
        let grammar = self.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let parsed_data = parser.parse(source_code, None).unwrap();
        let node = parsed_data.root_node();
        let mut function_nodes = vec![];
        let mut unique_ranges: HashSet<tree_sitter::Range> = Default::default();
        function_queries.into_iter().for_each(|function_query| {
            let query = tree_sitter::Query::new(grammar(), &function_query)
                .expect("function queries are well formed");
            let mut cursor = tree_sitter::QueryCursor::new();
            cursor
                .captures(&query, node, source_code)
                .into_iter()
                .for_each(|capture| {
                    capture.0.captures.into_iter().for_each(|capture| {
                        let capture_name = query
                            .capture_names()
                            .to_vec()
                            .remove(capture.index.try_into().unwrap());
                        let capture_type = FunctionNodeType::from_str(&capture_name);
                        if let Some(capture_type) = capture_type {
                            function_nodes.push(FunctionInformation::new(
                                Range::for_tree_node(&capture.node),
                                capture_type,
                            ));
                        }
                    })
                });
        });
        function_nodes
            .into_iter()
            .filter_map(|function_node| {
                let range = function_node.range();
                if unique_ranges.contains(&range.to_tree_sitter_range()) {
                    return None;
                }
                unique_ranges.insert(range.to_tree_sitter_range());
                Some(function_node.clone())
            })
            .collect()
    }

    pub fn generate_object_qualifier(&self, source_code: &[u8]) -> Option<Range> {
        let grammar = self.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let object_qualifier_query = self.object_qualifier.to_owned();
        let tree = parser.parse(source_code, None).unwrap();

        let query = tree_sitter::Query::new(grammar(), &object_qualifier_query).expect("to work");
        let mut cursor = tree_sitter::QueryCursor::new();
        let query_captures = cursor.captures(&query, tree.root_node(), source_code);
        let mut object_qualifier = None;
        query_captures.into_iter().for_each(|capture| {
            capture.0.captures.into_iter().for_each(|capture| {
                let hover_range = Range::for_tree_node(&capture.node);
                if object_qualifier.is_none() {
                    object_qualifier = Some(hover_range);
                }
            })
        });
        object_qualifier
    }
}

#[derive(Clone)]
pub struct TSLanguageParsing {
    configs: Vec<TSLanguageConfig>,
}

impl TSLanguageParsing {
    pub fn init() -> Self {
        Self {
            configs: vec![
                javascript_language_config(),
                typescript_language_config(),
                rust_language_config(),
                python_language_config(),
                go_language_config(),
            ],
        }
    }

    pub fn for_lang(&self, language: &str) -> Option<&TSLanguageConfig> {
        self.configs
            .iter()
            .find(|config| config.language_ids.contains(&language))
    }

    pub fn for_file_path(&self, file_path: &str) -> Option<&TSLanguageConfig> {
        let file_path = PathBuf::from(file_path);
        let file_extension = file_path
            .extension()
            .map(|extension| extension.to_str())
            .map(|extension| extension.to_owned())
            .flatten();
        match file_extension {
            Some(extension) => self
                .configs
                .iter()
                .find(|config| config.file_extensions.contains(&extension)),
            None => None,
        }
    }

    /// We will use this to chunk the file to pieces which can be used for
    /// searching
    pub fn chunk_file(
        &self,
        _file_path: &str,
        buffer: &str,
        file_extension: Option<&str>,
        file_language_id: Option<&str>,
    ) -> Vec<Span> {
        if file_extension.is_none() && file_language_id.is_none() {
            // We use naive chunker here which just splits on the number
            // of lines
            return naive_chunker(buffer, 50, 30);
        }
        let mut language_config_maybe = None;
        if let Some(language_id) = file_language_id {
            language_config_maybe = self.for_lang(language_id);
        }
        if let Some(file_extension) = file_extension {
            language_config_maybe = self
                .configs
                .iter()
                .find(|config| config.file_extensions.contains(&file_extension));
        }
        if let Some(language_config) = language_config_maybe {
            // We use tree-sitter to parse the file and get the chunks
            // for the file
            let language = language_config.grammar;
            let mut parser = tree_sitter::Parser::new();
            parser.set_language(language()).unwrap();
            let tree = parser.parse(buffer.as_bytes(), None).unwrap();
            // we allow for 1500 characters and 100 character coalesce
            let chunks = chunk_tree(&tree, language_config, 2500, 100, &buffer);
            chunks
        } else {
            // use naive chunker here which just splits the file into parts
            return naive_chunker(buffer, 30, 15);
        }
    }

    pub fn parse_documentation(&self, code: &str, language: &str) -> Vec<String> {
        let language_config_maybe = self
            .configs
            .iter()
            .find(|config| config.language_ids.contains(&language));
        if let None = language_config_maybe {
            return Default::default();
        }
        let language_config = language_config_maybe.expect("if let None check above to hold");
        let grammar = language_config.grammar;
        let documentation_queries = language_config.documentation_query.to_vec();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let parsed_data = parser.parse(code, None).unwrap();
        let node = parsed_data.root_node();
        let mut nodes = vec![];
        documentation_queries
            .into_iter()
            .for_each(|documentation_query| {
                let query = tree_sitter::Query::new(grammar(), &documentation_query)
                    .expect("documentation queries are well formed");
                let mut cursor = tree_sitter::QueryCursor::new();
                cursor
                    .captures(&query, node, code.as_bytes())
                    .into_iter()
                    .for_each(|capture| {
                        capture.0.captures.into_iter().for_each(|capture| {
                            nodes.push(capture.node);
                        })
                    });
            });

        // Now we only want to keep the unique ranges which we have captured
        // from the nodes
        let mut node_ranges: HashSet<tree_sitter::Range> = Default::default();
        let nodes = nodes
            .into_iter()
            .filter(|capture| {
                let range = capture.range();
                if node_ranges.contains(&range) {
                    return false;
                }
                node_ranges.insert(range);
                true
            })
            .collect::<Vec<_>>();

        // Now that we have the nodes, we also want to merge them together,
        // for that we need to first order the nodes
        get_merged_documentation_nodes(nodes, code)
    }

    pub fn function_information_nodes(
        &self,
        source_code: &str,
        language: &str,
    ) -> Vec<FunctionInformation> {
        let language_config = self.for_lang(language);
        if let None = language_config {
            return Default::default();
        }
        let language_config = language_config.expect("if let None check above to hold");
        let function_queries = language_config.function_query.to_vec();

        // Now we need to run the tree sitter query on this and get back the
        // answer
        let grammar = language_config.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let parsed_data = parser.parse(source_code.as_bytes(), None).unwrap();
        let node = parsed_data.root_node();
        let mut function_nodes = vec![];
        let mut unique_ranges: HashSet<tree_sitter::Range> = Default::default();
        function_queries.into_iter().for_each(|function_query| {
            let query = tree_sitter::Query::new(grammar(), &function_query)
                .expect("function queries are well formed");
            let mut cursor = tree_sitter::QueryCursor::new();
            cursor
                .captures(&query, node, source_code.as_bytes())
                .into_iter()
                .for_each(|capture| {
                    capture.0.captures.into_iter().for_each(|capture| {
                        let capture_name = query
                            .capture_names()
                            .to_vec()
                            .remove(capture.index.try_into().unwrap());
                        let capture_type = FunctionNodeType::from_str(&capture_name);
                        if let Some(capture_type) = capture_type {
                            function_nodes.push(FunctionInformation::new(
                                Range::for_tree_node(&capture.node),
                                capture_type,
                            ));
                        }
                    })
                });
        });
        function_nodes
            .into_iter()
            .filter_map(|function_node| {
                let range = function_node.range();
                if unique_ranges.contains(&range.to_tree_sitter_range()) {
                    return None;
                }
                unique_ranges.insert(range.to_tree_sitter_range());
                Some(function_node.clone())
            })
            .collect()
    }

    pub fn get_fix_range<'a>(
        &'a self,
        source_code: &'a str,
        language: &'a str,
        range: &'a Range,
        extra_width: usize,
    ) -> Option<Range> {
        let language_config = self.for_lang(language);
        if let None = language_config {
            return None;
        }
        let language_config = language_config.expect("if let None check above to hold");
        let grammar = language_config.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let parsed_data = parser.parse(source_code.as_bytes(), None).unwrap();
        let node = parsed_data.root_node();
        let descendant_node_maybe =
            node.descendant_for_byte_range(range.start_byte(), range.end_byte());
        if let None = descendant_node_maybe {
            return None;
        }
        // we are going to now check if the descendant node is important enough
        // for us to consider and fits in the size range we expect it to
        let descendant_node = descendant_node_maybe.expect("if let None to hold");
        let found_range = iterate_over_nodes_within_range(
            language,
            descendant_node,
            extra_width,
            range,
            true,
            language_config,
        );
        let current_node_range = Range::for_tree_node(&descendant_node);
        if found_range.start_byte() == current_node_range.start_byte()
            && found_range.end_byte() == current_node_range.end_byte()
        {
            // here we try to iterate upwards if we can find a node
            Some(find_node_to_use(language, descendant_node, language_config))
        } else {
            Some(found_range)
        }
    }

    pub fn get_parent_range_for_selection(
        &self,
        source_code: &str,
        language: &str,
        range: &Range,
    ) -> Range {
        let language_config = self.for_lang(language);
        if let None = language_config {
            return range.clone();
        }
        let language_config = language_config.expect("if let None check above to hold");
        let grammar = language_config.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let parsed_data = parser.parse(source_code.as_bytes(), None).unwrap();
        let node = parsed_data.root_node();
        let query = language_config
            .construct_types
            .iter()
            .map(|construct_type| format!("({construct_type}) @scope"))
            .collect::<Vec<_>>()
            .join("\n");
        let query = tree_sitter::Query::new(grammar(), &query).expect("query to be well formed");
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut found_node = None;
        cursor
            .matches(&query, node, source_code.as_bytes())
            .into_iter()
            .for_each(|capture| {
                capture.captures.into_iter().for_each(|capture| {
                    let node = capture.node;
                    let node_range = Range::for_tree_node(&node);
                    if node_range.start_byte() <= range.start_byte()
                        && node_range.end_byte() >= range.end_byte()
                        && found_node.is_none()
                    {
                        found_node = Some(node);
                    }
                })
            });
        found_node
            .map(|node| Range::for_tree_node(&node))
            .unwrap_or(range.clone())
    }

    pub fn detect_lang(&self, path: &str) -> Option<String> {
        // Here we look at the path extension from path and use that for detecting
        // the language
        Path::new(path)
            .extension()
            .map(|extension| extension.to_str())
            .flatten()
            .map(|ext| ext.to_string())
    }
}

fn find_node_to_use(
    language: &str,
    node: tree_sitter::Node<'_>,
    language_config: &TSLanguageConfig,
) -> Range {
    let parent_node = node.parent();
    let current_range = Range::for_tree_node(&node);
    let construct_type = language_config
        .construct_types
        .contains(&node.kind().to_owned());
    if construct_type || parent_node.is_none() {
        return current_range;
    }
    let parent_node = parent_node.expect("check above to work");
    let filtered_ranges = keep_iterating(
        parent_node
            .children(&mut parent_node.walk())
            .into_iter()
            .collect::<Vec<_>>(),
        parent_node,
        language_config,
        false,
    );
    if filtered_ranges.is_none() {
        return current_range;
    }
    let filtered_ranges_with_interest_node = filtered_ranges.expect("if let is_none to work");
    let filtered_ranges = filtered_ranges_with_interest_node.filtered_nodes;
    let index_of_interest = filtered_ranges_with_interest_node.index_of_interest;
    let index_of_interest_i64 = <i64>::try_from(index_of_interest).expect("usize to i64 to work");
    if index_of_interest_i64 - 1 >= 0
        && index_of_interest_i64 <= <i64>::try_from(filtered_ranges.len()).unwrap() - 1
    {
        let before_node = filtered_ranges[index_of_interest - 1];
        let after_node = filtered_ranges[index_of_interest + 1];
        Range::new(
            Position::from_tree_sitter_point(
                &before_node.start_position(),
                before_node.start_byte(),
            ),
            Position::from_tree_sitter_point(&after_node.end_position(), after_node.end_byte()),
        )
    } else {
        find_node_to_use(language, parent_node, language_config)
    }
}

fn iterate_over_nodes_within_range(
    language: &str,
    node: tree_sitter::Node<'_>,
    line_limit: usize,
    _range: &Range,
    should_go_inside: bool,
    language_config: &TSLanguageConfig,
) -> Range {
    let children = node
        .children(&mut node.walk())
        .into_iter()
        .collect::<Vec<_>>();
    if node.range().end_point.row - node.range().start_point.row + 1 <= line_limit {
        let found_range = if language_config
            .construct_types
            .contains(&node.kind().to_owned())
        {
            // if we have a matching kind, then we should be probably looking at
            // this node which fits the bill and keep going
            return Range::for_tree_node(&node);
        } else {
            iterate_over_children(
                language,
                children,
                line_limit,
                node,
                language_config,
                should_go_inside,
            )
        };
        let parent_node = node.parent();
        if let None = parent_node {
            found_range
        } else {
            let parent = parent_node.expect("if let None to hold");
            // we iterate over the children of the parent
            iterate_over_nodes_within_range(
                language,
                parent,
                line_limit,
                &found_range,
                false,
                language_config,
            )
        }
    } else {
        iterate_over_children(
            language,
            children,
            line_limit,
            node,
            language_config,
            should_go_inside,
        )
    }
}

fn iterate_over_children(
    _language: &str,
    children: Vec<tree_sitter::Node<'_>>,
    line_limit: usize,
    some_other_node_to_name: tree_sitter::Node<'_>,
    language_config: &TSLanguageConfig,
    should_go_inside: bool,
) -> Range {
    if children.is_empty() {
        return Range::for_tree_node(&some_other_node_to_name);
    }
    let filtered_ranges_maybe = keep_iterating(
        children,
        some_other_node_to_name,
        language_config,
        should_go_inside,
    );

    if let None = filtered_ranges_maybe {
        return Range::for_tree_node(&some_other_node_to_name);
    }

    let filtered_range = filtered_ranges_maybe.expect("if let None");
    let interested_nodes = filtered_range.filtered_nodes;
    let index_of_interest = filtered_range.index_of_interest;

    let mut start_idx = 0;
    let mut end_idx = interested_nodes.len() - 1;
    let mut current_start_range = interested_nodes[start_idx];
    let mut current_end_range = interested_nodes[end_idx];
    while distance_between_nodes(&current_start_range, &current_end_range)
        > <i64>::try_from(line_limit).unwrap()
        && start_idx != end_idx
    {
        if index_of_interest - start_idx < end_idx - index_of_interest {
            end_idx = end_idx - 1;
            current_end_range = interested_nodes[end_idx];
        } else {
            start_idx = start_idx + 1;
            current_start_range = interested_nodes[start_idx];
        }
    }

    if distance_between_nodes(&current_start_range, &current_end_range)
        > <i64>::try_from(line_limit).unwrap()
    {
        Range::new(
            Position::from_tree_sitter_point(
                &current_start_range.start_position(),
                current_start_range.start_byte(),
            ),
            Position::from_tree_sitter_point(
                &current_end_range.end_position(),
                current_end_range.end_byte(),
            ),
        )
    } else {
        Range::for_tree_node(&some_other_node_to_name)
    }
}

fn distance_between_nodes(node: &tree_sitter::Node<'_>, other_node: &tree_sitter::Node<'_>) -> i64 {
    <i64>::try_from(other_node.end_position().row).unwrap()
        - <i64>::try_from(node.end_position().row).unwrap()
        + 1
}

fn keep_iterating<'a>(
    children: Vec<tree_sitter::Node<'a>>,
    current_node: tree_sitter::Node<'a>,
    language_config: &'a TSLanguageConfig,
    should_go_inside: bool,
) -> Option<FilteredRanges<'a>> {
    let mut filtered_children: Vec<tree_sitter::Node<'a>>;
    let index: Option<usize>;
    if should_go_inside {
        filtered_children = children
            .into_iter()
            .filter(|node| {
                language_config
                    .construct_types
                    .contains(&node.kind().to_owned())
                    || language_config
                        .expression_statements
                        .contains(&node.kind().to_owned())
            })
            .collect::<Vec<_>>();
        index = Some(binary_search(filtered_children.to_vec(), &current_node));
        filtered_children.insert(index.expect("binary search always returns"), current_node);
    } else {
        filtered_children = children
            .into_iter()
            .filter(|node| {
                language_config
                    .construct_types
                    .contains(&node.kind().to_owned())
                    || language_config
                        .expression_statements
                        .contains(&node.kind().to_owned())
                    || (node.start_byte() <= current_node.start_byte()
                        && node.end_byte() >= current_node.end_byte())
            })
            .collect::<Vec<_>>();
        index = filtered_children.to_vec().into_iter().position(|node| {
            node.start_byte() <= current_node.start_byte()
                && node.end_byte() >= current_node.end_byte()
        })
    }

    index.map(|index| FilteredRanges {
        filtered_nodes: filtered_children,
        index_of_interest: index,
    })
}

struct FilteredRanges<'a> {
    filtered_nodes: Vec<tree_sitter::Node<'a>>,
    index_of_interest: usize,
}

fn binary_search<'a>(
    nodes: Vec<tree_sitter::Node<'a>>,
    current_node: &tree_sitter::Node<'_>,
) -> usize {
    let mut start = 0;
    let mut end = nodes.len();

    while start < end {
        let mid = (start + end) / 2;
        if nodes[mid].range().start_byte < current_node.range().start_byte {
            start = mid + 1;
        } else {
            end = mid;
        }
    }
    start
}

fn get_merged_documentation_nodes(matches: Vec<tree_sitter::Node>, source: &str) -> Vec<String> {
    let mut comments = Vec::new();
    let mut current_index = 0;

    while current_index < matches.len() {
        let mut lines = Vec::new();
        lines.push(get_text_from_source(
            source,
            &matches[current_index].range(),
        ));

        while current_index + 1 < matches.len()
            && matches[current_index].range().end_point.row + 1
                == matches[current_index + 1].range().start_point.row
        {
            current_index += 1;
            lines.push(get_text_from_source(
                source,
                &matches[current_index].range(),
            ));
        }

        comments.push(lines.join("\n"));
        current_index += 1;
    }
    comments
}

fn get_text_from_source(source: &str, range: &tree_sitter::Range) -> String {
    source[range.start_byte..range.end_byte].to_owned()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub language: Option<String>,
    pub data: Option<String>,
}

impl Span {
    fn new(start: usize, end: usize, language: Option<String>, data: Option<String>) -> Self {
        Self {
            start,
            end,
            language,
            data,
        }
    }

    fn len(&self) -> usize {
        self.end - self.start
    }
}

fn chunk_node(node: tree_sitter::Node, language: &TSLanguageConfig, max_chars: usize) -> Vec<Span> {
    let mut chunks: Vec<Span> = vec![];
    let mut current_chunk = Span::new(
        node.start_byte(),
        node.start_byte(),
        language.get_language(),
        None,
    );
    let mut node_walker = node.walk();
    let current_node_children = node.children(&mut node_walker);
    for child in current_node_children {
        if child.end_byte() - child.start_byte() > max_chars {
            chunks.push(current_chunk.clone());
            current_chunk = Span::new(
                child.end_byte(),
                child.end_byte(),
                language.get_language(),
                None,
            );
            chunks.extend(chunk_node(child, language, max_chars));
        } else if child.end_byte() - child.start_byte() + current_chunk.len() > max_chars {
            chunks.push(current_chunk.clone());
            current_chunk = Span::new(
                child.start_byte(),
                child.end_byte(),
                language.get_language(),
                None,
            );
        } else {
            current_chunk.end = child.end_byte();
        }
    }
    chunks.push(current_chunk);
    chunks
}

/// We want to get back the non whitespace length of the string
fn non_whitespace_len(s: &str) -> usize {
    s.chars().filter(|c| !c.is_whitespace()).count()
}

fn get_line_number(byte_position: usize, split_lines: &[&str]) -> usize {
    let mut line_number = 0;
    let mut current_position = 0;
    for line in split_lines {
        if current_position + line.len() > byte_position {
            return line_number;
        }
        current_position += line.len();
        line_number += 1;
    }
    line_number
}

pub fn chunk_tree(
    tree: &tree_sitter::Tree,
    language: &TSLanguageConfig,
    max_characters_per_chunk: usize,
    coalesce: usize,
    buffer_content: &str,
) -> Vec<Span> {
    let root_node = tree.root_node();
    let split_lines = buffer_content.lines().collect::<Vec<_>>();
    let mut chunks = chunk_node(root_node, language, max_characters_per_chunk);

    if chunks.len() == 0 {
        return Default::default();
    }
    if chunks.len() < 2 {
        return vec![Span::new(
            0,
            get_line_number(chunks[0].end, split_lines.as_slice()),
            language.get_language(),
            Some(buffer_content.to_owned()),
        )];
    }
    for (prev, curr) in chunks.to_vec().iter_mut().zip(chunks.iter_mut().skip(1)) {
        prev.end = curr.start;
    }

    let mut new_chunks: Vec<Span> = Default::default();
    let mut current_chunk = Span::new(0, 0, language.get_language(), None);
    for chunk in chunks.iter() {
        current_chunk = Span::new(
            current_chunk.start,
            chunk.end,
            language.get_language(),
            None,
        );
        if non_whitespace_len(buffer_content[current_chunk.start..current_chunk.end].trim())
            > coalesce
        {
            new_chunks.push(current_chunk.clone());
            current_chunk = Span::new(chunk.end, chunk.end, language.get_language(), None);
        }
    }

    if current_chunk.len() > 0 {
        new_chunks.push(current_chunk.clone());
    }

    let mut line_chunks = new_chunks
        .iter()
        .map(|chunk| {
            let start_line = get_line_number(chunk.start, split_lines.as_slice());
            let end_line = get_line_number(chunk.end, split_lines.as_slice());
            Span::new(start_line, end_line, language.get_language(), None)
        })
        .filter(|span| span.len() > 0)
        .collect::<Vec<Span>>();

    if line_chunks.len() > 1 && line_chunks.last().unwrap().len() < coalesce {
        let chunks_len = line_chunks.len();
        let last_chunk = line_chunks.last().unwrap().clone();
        let prev_chunk = line_chunks.get_mut(chunks_len - 2).unwrap();
        prev_chunk.end = last_chunk.end;
        line_chunks.pop();
    }

    let split_buffer = buffer_content.split("\n").collect::<Vec<_>>();

    line_chunks
        .into_iter()
        .map(|line_chunk| {
            let data: String = split_buffer[line_chunk.start..line_chunk.end].join("\n");
            Span {
                start: line_chunk.start,
                end: line_chunk.end,
                language: line_chunk.language,
                data: Some(data),
            }
        })
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {

    use tree_sitter::Parser;
    use tree_sitter::TreeCursor;

    use crate::chunking::text_document::Position;
    use crate::chunking::text_document::Range;
    use crate::chunking::types::OutlineNodeType;

    use super::naive_chunker;
    use super::TSLanguageParsing;

    fn get_naive_chunking_test_string<'a>() -> &'a str {
        r#"
        # @axflow/models/azure-openai/chat

        Interface with [Azure-OpenAI's Chat Completions API](https://learn.microsoft.com/en-us/azure/ai-services/openai/reference) using this module.

        Note that this is very close to the vanilla openAI interface, with some subtle minor differences (the return types contain content filter results, see the `AzureOpenAIChatTypes.ContentFilterResults` type ).

        In addition, the streaming methods sometimes return objects with empty `choices` arrays. This is automatically handled if you use the `streamTokens()` method.

        ```ts
        import { AzureOpenAIChat } from '@axflow/models/azure-openai/chat';
        import type { AzureOpenAIChatTypes } from '@axflow/models/azure-openai/chat';
        ```

        ```ts
        declare class AzureOpenAIChat {
          static run: typeof run;
          static stream: typeof stream;
          static streamBytes: typeof streamBytes;
          static streamTokens: typeof streamTokens;
        }
        ```

        ## `run`

        ```ts
        /**
         * Run a chat completion against the Azure-openAI API.
         *
         * @see https://learn.microsoft.com/en-us/azure/ai-services/openai/reference#chat-completions
         *
         * @param request The request body sent to Azure. See Azure's documentation for all available parameters.
         * @param options
         * @param options.apiKey Azure API key.
         * @param options.resourceName Azure resource name.
         * @param options.deploymentId Azure deployment id.
         * @param options.apiUrl The url of the OpenAI (or compatible) API. If this is passed, resourceName and deploymentId are ignored.
         * @param options.fetch A custom implementation of fetch. Defaults to globalThis.fetch.
         * @param options.headers Optionally add additional HTTP headers to the request.
         * @param options.signal An AbortSignal that can be used to abort the fetch request.
         *
         * @returns an Azure OpenAI chat completion. See Azure's documentation for /chat/completions
         */
        declare function run(
          request: AzureOpenAIChatTypes.Request,
          options: AzureOpenAIChatTypes.RequestOptions
        ): Promise<AzureOpenAIChatTypes.Response>;
        ```

        ## `streamBytes`

        ```ts
        /**
         * Run a streaming chat completion against the Azure-openAI API. The resulting stream is the raw unmodified bytes from the API.
         *
         * @see https://learn.microsoft.com/en-us/azure/ai-services/openai/reference#chat-completions
         *
         * @param request The request body sent to Azure. See Azure's documentation for all available parameters.
         * @param options
         * @param options.apiKey Azure API key.
         * @param options.resourceName Azure resource name.
         * @param options.deploymentId Azure deployment id.
         * @param options.apiUrl The url of the OpenAI (or compatible) API. If this is passed, resourceName and deploymentId are ignored.
         * @param options.fetch A custom implementation of fetch. Defaults to globalThis.fetch.
         * @param options.headers Optionally add additional HTTP headers to the request.
         * @param options.signal An AbortSignal that can be used to abort the fetch request.
         *
         * @returns A stream of bytes directly from the API.
         */
        declare function streamBytes(
          request: AzureOpenAIChatTypes.Request,
          options: AzureOpenAIChatTypes.RequestOptions
        ): Promise<ReadableStream<Uint8Array>>;
        ```

        ## `stream`

        ```ts
        /**
         * Run a streaming chat completion against the Azure-openAI API. The resulting stream is the parsed stream data as JavaScript objects.
         *
         * @see https://learn.microsoft.com/en-us/azure/ai-services/openai/reference#chat-completions
         *
         * Example object:
         * {"id":"chatcmpl-864d71dHehdlb2Vjq7WP5nHz10LRO","object":"chat.completion.chunk","created":1696458457,"model":"gpt-4","choices":[{"index":0,"finish_reason":null,"delta":{"content":" me"}}],"usage":null}
         *
         * @param request The request body sent to Azure. See Azure's documentation for all available parameters.
         * @param options
         * @param options.apiKey Azure API key.
         * @param options.resourceName Azure resource name.
         * @param options.deploymentId Azure deployment id.
         * @param options.apiUrl The url of the OpenAI (or compatible) API. If this is passed, resourceName and deploymentId are ignored.
         * @param options.fetch A custom implementation of fetch. Defaults to globalThis.fetch.
         * @param options.headers Optionally add additional HTTP headers to the request.
         * @param options.signal An AbortSignal that can be used to abort the fetch request.
         *
         * @returns A stream of objects representing each chunk from the API.
         */
        declare function stream(
          request: AzureOpenAIChatTypes.Request,
          options: AzureOpenAIChatTypes.RequestOptions
        ): Promise<ReadableStream<AzureOpenAIChatTypes.Chunk>>;
        ```

        ## `streamTokens`

        ```ts
        /**
         * Run a streaming chat completion against the Azure-openAI API. The resulting stream emits only the string tokens.
         *
         * @see https://learn.microsoft.com/en-us/azure/ai-services/openai/reference#chat-completions
         *
         * @param request The request body sent to Azure. See Azure's documentation for all available parameters.
         * @param options
         * @param options.apiKey Azure API key.
         * @param options.resourceName Azure resource name.
         * @param options.deploymentId Azure deployment id.
         * @param options.apiUrl The url of the OpenAI (or compatible) API. If this is passed, resourceName and deploymentId are ignored.
         * @param options.fetch A custom implementation of fetch. Defaults to globalThis.fetch.
         * @param options.headers Optionally add additional HTTP headers to the request.
         * @param options.signal An AbortSignal that can be used to abort the fetch request.
         *
         * @returns A stream of tokens from the API.
         */
        declare function streamTokens(
          request: AzureOpenAIChatTypes.Request,
          options: AzureOpenAIChatTypes.RequestOptions
        ): Promise<ReadableStream<string>>;
        ```
        "#
    }

    #[test]
    fn test_naive_chunker() {
        // The test buffer has a total length of 128, with a chunk of size 30
        // and overlap of 15 we get 9 chunks, its easy maths. ceil(128/15) == 9
        let chunks = naive_chunker(get_naive_chunking_test_string(), 30, 15);
        assert_eq!(chunks.len(), 9);
    }

    #[test]
    fn test_documentation_parsing_rust() {
        let source_code = r#"
/// Some comment
/// Some other comment
fn blah_blah() {

}

/// something else
struct A {
    /// something over here
    pub a: string,
}
        "#;
        let tree_sitter_parsing = TSLanguageParsing::init();
        let documentation = tree_sitter_parsing.parse_documentation(source_code, "rust");
        assert_eq!(
            documentation,
            vec![
                "/// Some comment\n/// Some other comment",
                "/// something else",
                "/// something over here",
            ]
        );
    }

    #[test]
    fn test_documentation_parsing_rust_another() {
        let source_code = "/// Returns the default user ID as a `String`.\n///\n/// The default user ID is set to \"codestory\".\nfn default_user_id() -> String {\n    \"codestory\".to_owned()\n}";
        let tree_sitter_parsing = TSLanguageParsing::init();
        let documentation = tree_sitter_parsing.parse_documentation(source_code, "rust");
        assert_eq!(
            documentation,
            vec![
                "/// Returns the default user ID as a `String`.\n///\n/// The default user ID is set to \"codestory\".",
            ],
        );
    }

    #[test]
    fn test_documentation_parsing_typescript() {
        let source_code = r#"
        /**
         * Run a streaming chat completion against the Azure-openAI API. The resulting stream emits only the string tokens.
         *
         * @see https://learn.microsoft.com/en-us/azure/ai-services/openai/reference#chat-completions
         *
         * @param request The request body sent to Azure. See Azure's documentation for all available parameters.
         * @param options
         * @param options.apiKey Azure API key.
         * @param options.resourceName Azure resource name.
         * @param options.deploymentId Azure deployment id.
         * @param options.apiUrl The url of the OpenAI (or compatible) API. If this is passed, resourceName and deploymentId are ignored.
         * @param options.fetch A custom implementation of fetch. Defaults to globalThis.fetch.
         * @param options.headers Optionally add additional HTTP headers to the request.
         * @param options.signal An AbortSignal that can be used to abort the fetch request.
         *
         * @returns A stream of tokens from the API.
         */
        declare function streamTokens(
          request: AzureOpenAIChatTypes.Request,
          options: AzureOpenAIChatTypes.RequestOptions
        ): Promise<ReadableStream<string>>;
        "#;

        let tree_sitter_parsing = TSLanguageParsing::init();
        let documentation = tree_sitter_parsing.parse_documentation(source_code, "typescript");
        assert_eq!(
            documentation,
            vec![
    "/**\n         * Run a streaming chat completion against the Azure-openAI API. The resulting stream emits only the string tokens.\n         *\n         * @see https://learn.microsoft.com/en-us/azure/ai-services/openai/reference#chat-completions\n         *\n         * @param request The request body sent to Azure. See Azure's documentation for all available parameters.\n         * @param options\n         * @param options.apiKey Azure API key.\n         * @param options.resourceName Azure resource name.\n         * @param options.deploymentId Azure deployment id.\n         * @param options.apiUrl The url of the OpenAI (or compatible) API. If this is passed, resourceName and deploymentId are ignored.\n         * @param options.fetch A custom implementation of fetch. Defaults to globalThis.fetch.\n         * @param options.headers Optionally add additional HTTP headers to the request.\n         * @param options.signal An AbortSignal that can be used to abort the fetch request.\n         *\n         * @returns A stream of tokens from the API.\n         */",
            ],
        );
    }

    #[test]
    fn test_function_body_parsing_rust() {
        let source_code = r#"
/// Some comment
/// Some other comment
fn blah_blah() {

}

/// something else
struct A {
    /// something over here
    pub a: string,
}

impl A {
    fn something_else() -> Option<String> {
        None
    }
}
        "#;

        let tree_sitter_parsing = TSLanguageParsing::init();
        let function_nodes = tree_sitter_parsing.function_information_nodes(source_code, "rust");

        // we should get back 2 function nodes here and since we capture 3 pieces
        // of information for each function block, in total that is 6
        assert_eq!(function_nodes.len(), 6);
    }

    #[test]
    fn test_fix_range_for_typescript() {
        let source_code = "import { POST, HttpError } from '@axflow/models/shared';\nimport { headers } from './shared';\nimport type { SharedRequestOptions } from './shared';\n\nconst COHERE_API_URL = 'https://api.cohere.ai/v1/generate';\n\nexport namespace CohereGenerationTypes {\n  export type Request = {\n    prompt: string;\n    model?: string;\n    num_generations?: number;\n    max_tokens?: number;\n    truncate?: string;\n    temperature?: number;\n    preset?: string;\n    end_sequences?: string[];\n    stop_sequences?: string[];\n    k?: number;\n    p?: number;\n    frequency_penalty?: number;\n    presence_penalty?: number;\n    return_likelihoods?: string;\n    logit_bias?: Record<string, any>;\n  };\n\n  export type RequestOptions = SharedRequestOptions;\n\n  export type Generation = {\n    id: string;\n    text: string;\n    index?: number;\n    likelihood?: number;\n    token_likelihoods?: Array<{\n      token: string;\n      likelihood: number;\n    }>;\n  };\n\n  export type Response = {\n    id: string;\n    prompt?: string;\n    generations: Generation[];\n    meta: {\n      api_version: {\n        version: string;\n        is_deprecated?: boolean;\n        is_experimental?: boolean;\n      };\n      warnings?: string[];\n    };\n  };\n\n  export type Chunk = {\n    text?: string;\n    is_finished: boolean;\n    finished_reason?: 'COMPLETE' | 'MAX_TOKENS' | 'ERROR' | 'ERROR_TOXIC';\n    response?: {\n      id: string;\n      prompt?: string;\n      generations: Generation[];\n    };\n  };\n}\n\n/**\n * Run a generation against the Cohere API.\n *\n * @see https://docs.cohere.com/reference/generate\n *\n * @param request The request body sent to Cohere. See Cohere's documentation for /v1/generate for supported parameters.\n * @param options\n * @param options.apiKey Cohere API key.\n * @param options.apiUrl The url of the Cohere (or compatible) API. Defaults to https://api.cohere.ai/v1/generate.\n * @param options.fetch A custom implementation of fetch. Defaults to globalThis.fetch.\n * @param options.headers Optionally add additional HTTP headers to the request.\n * @param options.signal An AbortSignal that can be used to abort the fetch request.\n * @returns Cohere completion. See Cohere's documentation for /v1/generate.\n */\nasync function run(\n  request: CohereGenerationTypes.Request,\n  options: CohereGenerationTypes.RequestOptions,\n): Promise<CohereGenerationTypes.Response> {\n  const url = options.apiUrl || COHERE_API_URL;\n\n  const response = await POST(url, {\n    headers: headers(options.apiKey, options.headers),\n    body: JSON.stringify({ ...request, stream: false }),\n    fetch: options.fetch,\n    signal: options.signal,\n  });\n\n  return response.json();\n}\n\n/**\n * Run a streaming generation against the Cohere API. The resulting stream is the raw unmodified bytes from the API.\n *\n * @see https://docs.cohere.com/reference/generate\n *\n * @param request The request body sent to Cohere. See Cohere's documentation for /v1/generate for supported parameters.\n * @param options\n * @param options.apiKey Cohere API key.\n * @param options.apiUrl The url of the Cohere (or compatible) API. Defaults to https://api.cohere.ai/v1/generate.\n * @param options.fetch A custom implementation of fetch. Defaults to globalThis.fetch.\n * @param options.headers Optionally add additional HTTP headers to the request.\n * @param options.signal An AbortSignal that can be used to abort the fetch request.\n * @returns A stream of bytes directly from the API.\n */\nasync function streamBytes(\n  request: CohereGenerationTypes.Request,\n  options: CohereGenerationTypes.RequestOptions,\n): Promise<ReadableStream<Uint8Array>> {\n  const url = options.apiUrl || COHERE_API_URL;\n\n  const response = await POST(url, {\n    headers: headers(options.apiKey, options.headers),\n    body: JSON.stringify({ ...request, stream: true }),\n    fetch: options.fetch,\n    signal: options.signal,\n  });\n\n  if (!response.body) {\n    throw new HttpError('Expected response body to be a ReadableStream', response);\n  }\n\n  return response.body;\n}\n\nfunction noop(chunk: CohereGenerationTypes.Chunk) {\n  return chunk;\n}\n\n/**\n * Run a streaming generation against the Cohere API. The resulting stream is the parsed stream data as JavaScript objects.\n *\n * @see https://docs.cohere.com/reference/generate\n *\n * @param request The request body sent to Cohere. See Cohere's documentation for /v1/generate for supported parameters.\n * @param options\n * @param options.apiKey Cohere API key.\n * @param options.apiUrl The url of the Cohere (or compatible) API. Defaults to https://api.cohere.ai/v1/generate.\n * @param options.fetch A custom implementation of fetch. Defaults to globalThis.fetch.\n * @param options.headers Optionally add additional HTTP headers to the request.\n * @param options.signal An AbortSignal that can be used to abort the fetch request.\n * @returns A stream of objects representing each chunk from the API.\n */\nasync function stream(\n  request: CohereGenerationTypes.Request,\n  options: CohereGenerationTypes.RequestOptions,\n): Promise<ReadableStream<CohereGenerationTypes.Chunk>> {\n  const byteStream = await streamBytes(request, options);\n  return byteStream.pipeThrough(new CohereGenerationDecoderStream(noop));\n}\n\nfunction chunkToToken(chunk: CohereGenerationTypes.Chunk) {\n  return chunk.text || '';\n}\n\n/**\n * Run a streaming generation against the Cohere API. The resulting stream emits only the string tokens.\n *\n * @see https://docs.cohere.com/reference/generate\n *\n * @param request The request body sent to Cohere. See Cohere's documentation for /v1/generate for supported parameters.\n * @param options\n * @param options.apiKey Cohere API key.\n * @param options.apiUrl The url of the Cohere (or compatible) API. Defaults to https://api.cohere.ai/v1/generate.\n * @param options.fetch A custom implementation of fetch. Defaults to globalThis.fetch.\n * @param options.headers Optionally add additional HTTP headers to the request.\n * @param options.signal An AbortSignal that can be used to abort the fetch request.\n * @returns A stream of tokens from the API.\n */\nasync function streamTokens(\n  request: CohereGenerationTypes.Request,\n  options: CohereGenerationTypes.RequestOptions,\n): Promise<ReadableStream<string>> {\n  const byteStream = await streamBytes(request, options);\n  return byteStream.pipeThrough(new CohereGenerationDecoderStream(chunkToToken));\n}\n\n/**\n * An object that encapsulates methods for calling the Cohere Generate API.\n */\nexport class CohereGeneration {\n  static run = run;\n  static stream = stream;\n  static streamBytes = streamBytes;\n  static streamTokens = streamTokens;\n}\n\nclass CohereGenerationDecoderStream<T> extends TransformStream<Uint8Array, T> {\n  private static parse(line: string): CohereGenerationTypes.Chunk | null {\n    line = line.trim();\n\n    // Empty lines are ignored\n    if (line.length === 0) {\n      return null;\n    }\n\n    try {\n      return JSON.parse(line);\n    } catch (error) {\n      throw new Error(\n        `Invalid event: expected well-formed event lines but got ${JSON.stringify(line)}`,\n      );\n    }\n  }\n\n  private static transformer<T>(map: (chunk: CohereGenerationTypes.Chunk) => T) {\n    let buffer: string[] = [];\n    const decoder = new TextDecoder();\n\n    return (bytes: Uint8Array, controller: TransformStreamDefaultController<T>) => {\n      const chunk = decoder.decode(bytes);\n\n      for (let i = 0, len = chunk.length; i < len; ++i) {\n        // Cohere separates events with '\\n'\n        const isEventSeparator = chunk[\"something\"] === '\\n';\n\n        // Keep buffering unless we've hit the end of an event\n        if (!isEventSeparator) {\n          buffer.push(chunk[i]);\n          continue;\n        }\n\n        const event = CohereGenerationDecoderStream.parse(buffer.join(''));\n\n        if (event) {\n          controller.enqueue(map(event));\n        }\n\n        buffer = [];\n      }\n    };\n  }\n\n  constructor(map: (chunk: CohereGenerationTypes.Chunk) => T) {\n    super({ transform: CohereGenerationDecoderStream.transformer(map) });\n  }\n}\n";
        let language = "typescript";
        let range = Range::new(Position::new(217, 45, 7441), Position::new(217, 45, 7441));
        let extra_width = 15;
        let tree_sitter_parsing = TSLanguageParsing::init();
        let fix_range =
            tree_sitter_parsing.get_fix_range(source_code, language, &range, extra_width);
        assert!(fix_range.is_some());
        let fix_range = fix_range.expect("is_some to work");
        let generated_range = source_code[fix_range.start_byte()..fix_range.end_byte()].to_owned();
        assert_eq!(generated_range, "{\n        // Cohere separates events with '\\n'\n        const isEventSeparator = chunk[\"something\"] === '\\n';\n\n        // Keep buffering unless we've hit the end of an event\n        if (!isEventSeparator) {\n          buffer.push(chunk[i]);\n          continue;\n        }\n\n        const event = CohereGenerationDecoderStream.parse(buffer.join(''));\n\n        if (event) {\n          controller.enqueue(map(event));\n        }\n\n        buffer = [];\n      }");
    }

    #[test]
    fn test_function_nodes_for_typescript() {
        let source_code = "import { POST, HttpError } from '@axflow/models/shared';\nimport { headers } from './shared';\nimport type { SharedRequestOptions } from './shared';\n\nconst COHERE_API_URL = 'https://api.cohere.ai/v1/generate';\n\nexport namespace CohereGenerationTypes {\n  export type Request = {\n    prompt: string;\n    model?: string;\n    num_generations?: number;\n    max_tokens?: number;\n    truncate?: string;\n    temperature?: number;\n    preset?: string;\n    end_sequences?: string[];\n    stop_sequences?: string[];\n    k?: number;\n    p?: number;\n    frequency_penalty?: number;\n    presence_penalty?: number;\n    return_likelihoods?: string;\n    logit_bias?: Record<string, any>;\n  };\n\n  export type RequestOptions = SharedRequestOptions;\n\n  export type Generation = {\n    id: string;\n    text: string;\n    index?: number;\n    likelihood?: number;\n    token_likelihoods?: Array<{\n      token: string;\n      likelihood: number;\n    }>;\n  };\n\n  export type Response = {\n    id: string;\n    prompt?: string;\n    generations: Generation[];\n    meta: {\n      api_version: {\n        version: string;\n        is_deprecated?: boolean;\n        is_experimental?: boolean;\n      };\n      warnings?: string[];\n    };\n  };\n\n  export type Chunk = {\n    text?: string;\n    is_finished: boolean;\n    finished_reason?: 'COMPLETE' | 'MAX_TOKENS' | 'ERROR' | 'ERROR_TOXIC';\n    response?: {\n      id: string;\n      prompt?: string;\n      generations: Generation[];\n    };\n  };\n}\n\n/**\n * Run a generation against the Cohere API.\n *\n * @see https://docs.cohere.com/reference/generate\n *\n * @param request The request body sent to Cohere. See Cohere's documentation for /v1/generate for supported parameters.\n * @param options\n * @param options.apiKey Cohere API key.\n * @param options.apiUrl The url of the Cohere (or compatible) API. Defaults to https://api.cohere.ai/v1/generate.\n * @param options.fetch A custom implementation of fetch. Defaults to globalThis.fetch.\n * @param options.headers Optionally add additional HTTP headers to the request.\n * @param options.signal An AbortSignal that can be used to abort the fetch request.\n * @returns Cohere completion. See Cohere's documentation for /v1/generate.\n */\nasync function run(\n  request: CohereGenerationTypes.Request,\n  options: CohereGenerationTypes.RequestOptions,\n): Promise<CohereGenerationTypes.Response> {\n  const url = options.apiUrl || COHERE_API_URL;\n\n  const response = await POST(url, {\n    headers: headers(options.apiKey, options.headers),\n    body: JSON.stringify({ ...request, stream: false }),\n    fetch: options.fetch,\n    signal: options.signal,\n  });\n\n  return response.json();\n}\n\n/**\n * Run a streaming generation against the Cohere API. The resulting stream is the raw unmodified bytes from the API.\n *\n * @see https://docs.cohere.com/reference/generate\n *\n * @param request The request body sent to Cohere. See Cohere's documentation for /v1/generate for supported parameters.\n * @param options\n * @param options.apiKey Cohere API key.\n * @param options.apiUrl The url of the Cohere (or compatible) API. Defaults to https://api.cohere.ai/v1/generate.\n * @param options.fetch A custom implementation of fetch. Defaults to globalThis.fetch.\n * @param options.headers Optionally add additional HTTP headers to the request.\n * @param options.signal An AbortSignal that can be used to abort the fetch request.\n * @returns A stream of bytes directly from the API.\n */\nasync function streamBytes(\n  request: CohereGenerationTypes.Request,\n  options: CohereGenerationTypes.RequestOptions,\n): Promise<ReadableStream<Uint8Array>> {\n  const url = options.apiUrl || COHERE_API_URL;\n\n  const response = await POST(url, {\n    headers: headers(options.apiKey, options.headers),\n    body: JSON.stringify({ ...request, stream: true }),\n    fetch: options.fetch,\n    signal: options.signal,\n  });\n\n  if (!response.body) {\n    throw new HttpError('Expected response body to be a ReadableStream', response);\n  }\n\n  return response.body;\n}\n\nfunction noop(chunk: CohereGenerationTypes.Chunk) {\n  return chunk;\n}\n\n/**\n * Run a streaming generation against the Cohere API. The resulting stream is the parsed stream data as JavaScript objects.\n *\n * @see https://docs.cohere.com/reference/generate\n *\n * @param request The request body sent to Cohere. See Cohere's documentation for /v1/generate for supported parameters.\n * @param options\n * @param options.apiKey Cohere API key.\n * @param options.apiUrl The url of the Cohere (or compatible) API. Defaults to https://api.cohere.ai/v1/generate.\n * @param options.fetch A custom implementation of fetch. Defaults to globalThis.fetch.\n * @param options.headers Optionally add additional HTTP headers to the request.\n * @param options.signal An AbortSignal that can be used to abort the fetch request.\n * @returns A stream of objects representing each chunk from the API.\n */\nasync function stream(\n  request: CohereGenerationTypes.Request,\n  options: CohereGenerationTypes.RequestOptions,\n): Promise<ReadableStream<CohereGenerationTypes.Chunk>> {\n  const byteStream = await streamBytes(request, options);\n  return byteStream.pipeThrough(new CohereGenerationDecoderStream(noop));\n}\n\nfunction chunkToToken(chunk: CohereGenerationTypes.Chunk) {\n  return chunk.text || '';\n}\n\n/**\n * Run a streaming generation against the Cohere API. The resulting stream emits only the string tokens.\n *\n * @see https://docs.cohere.com/reference/generate\n *\n * @param request The request body sent to Cohere. See Cohere's documentation for /v1/generate for supported parameters.\n * @param options\n * @param options.apiKey Cohere API key.\n * @param options.apiUrl The url of the Cohere (or compatible) API. Defaults to https://api.cohere.ai/v1/generate.\n * @param options.fetch A custom implementation of fetch. Defaults to globalThis.fetch.\n * @param options.headers Optionally add additional HTTP headers to the request.\n * @param options.signal An AbortSignal that can be used to abort the fetch request.\n * @returns A stream of tokens from the API.\n */\nasync function streamTokens(\n  request: CohereGenerationTypes.Request,\n  options: CohereGenerationTypes.RequestOptions,\n): Promise<ReadableStream<string>> {\n  const byteStream = await streamBytes(request, options);\n  return byteStream.pipeThrough(new CohereGenerationDecoderStream(chunkToToken));\n}\n\n/**\n * An object that encapsulates methods for calling the Cohere Generate API.\n */\nexport class CohereGeneration {\n  static run = run;\n  static stream = stream;\n  static streamBytes = streamBytes;\n  static streamTokens = streamTokens;\n}\n\nclass CohereGenerationDecoderStream<T> extends TransformStream<Uint8Array, T> {\n  private static parse(line: string): CohereGenerationTypes.Chunk | null {\n    line = line.trim();\n\n    // Empty lines are ignored\n    if (line.length === 0) {\n      return null;\n    }\n\n    try {\n      return JSON.parse(line);\n    } catch (error) {\n      throw new Error(\n        `Invalid event: expected well-formed event lines but got ${JSON.stringify(line)}`,\n      );\n    }\n  }\n\n  private static transformer<T>(map: (chunk: CohereGenerationTypes.Chunk) => T) {\n    let buffer: string[] = [];\n    const decoder = new TextDecoder();\n\n    return (bytes: Uint8Array, controller: TransformStreamDefaultController<T>) => {\n      const chunk = decoder.decode(bytes);\n\n      for (let i = 0, len = chunk.length; i < len; ++i) {\n        // Cohere separates events with '\\n'\n        const isEventSeparator = chunk[\"something\"] === '\\n';\n\n        // Keep buffering unless we've hit the end of an event\n        if (!isEventSeparator) {\n          buffer.push(chunk[i]);\n          continue;\n        }\n\n        const event = CohereGenerationDecoderStream.parse(buffer.join(''));\n\n        if (event) {\n          controller.enqueue(map(event));\n        }\n\n        buffer = [];\n      }\n    };\n  }\n\n  constructor(map: (chunk: CohereGenerationTypes.Chunk) => T) {\n    super({ transform: CohereGenerationDecoderStream.transformer(map) });\n  }\n}\n";
        let language = "typescript";
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang(language)
            .expect("test to work");
        let function_data = ts_language_config.capture_function_data(source_code.as_bytes());
        assert!(true);
    }

    #[test]
    fn test_type_nodes_for_typescript() {
        let source_code = r#"
// Some random comment over here
type SometingElse = {
    a: string,
    b: number,
};

// something else over here as comment
export type SomethingInterface = {
    a: string,
    b: number,
};

namespace SomeNamespace {
    export type Something = {
        a: string,
        b: number,
    };
}
        "#;
        let language = "typescript";
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang(language)
            .expect("test to work");
        let type_information = ts_language_config.capture_type_data(source_code.as_bytes());
        assert_eq!(type_information.len(), 3);
        assert_eq!(type_information[0].name, "SometingElse");
        assert_eq!(
            type_information[0].documentation,
            Some("// Some random comment over here".to_owned())
        );
        assert_eq!(type_information[1].name, "SomethingInterface");
    }

    #[test]
    fn test_function_nodes_documentation_for_typescript() {
        let source_code = r#"
// Updates an existing chat agent with new metadata
function updateAgent(id: string, updateMetadata: ICSChatAgentMetadata): void {
    // ...
}

// Returns the default chat agent
function getDefaultAgent(): IChatAgent | undefined {
    // ...
}

// Returns the secondary chat agent
function getSecondaryAgent(): IChatAgent | undefined {
    // ...
}

// Returns all registered chat agents
function getAgents(): Array<IChatAgent> {
    // ...
}

// Checks if a chat agent with the given id exists
function hasAgent(id: string): boolean {
    // ...
}

// Returns a chat agent with the given id
function getAgent(id: string): IChatAgent | undefined {
    // ...
}

// Invokes a chat agent with the given id and request
async function invokeAgent(id: string, request: ICSChatAgentRequest, progress: (part: ICSChatProgress) => void, history: ICSChatMessage[], token: CancellationToken): Promise<ICSChatAgentResult> {
    // ...
}

// Returns followups for a chat agent with the given id and session id
async getFollowups(id: string, sessionId: string, token: CancellationToken): Promise<ICSChatFollowup[]> {
    // ...
}

// Returns edits for a chat agent with the given context
async function getEdits(context: ICSChatAgentEditRequest, progress: (part: ICSChatAgentEditRepsonse) => void, token: CancellationToken): Promise<ICSChatAgentEditRepsonse | undefined> {
    // ...
}"#;
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing.for_lang("typescript").expect("to work");
        let fn_info = ts_language_config.capture_function_data(source_code.as_bytes());
        assert!(true);
    }

    #[test]
    fn test_outline_for_typescript() {
        let source_code = "import { POST, HttpError } from '@axflow/models/shared';\nimport { headers } from './shared';\nimport type { SharedRequestOptions } from './shared';\n\nconst COHERE_API_URL = 'https://api.cohere.ai/v1/generate';\n\nexport namespace CohereGenerationTypes {\n  export type Request = {\n    prompt: string;\n    model?: string;\n    num_generations?: number;\n    max_tokens?: number;\n    truncate?: string;\n    temperature?: number;\n    preset?: string;\n    end_sequences?: string[];\n    stop_sequences?: string[];\n    k?: number;\n    p?: number;\n    frequency_penalty?: number;\n    presence_penalty?: number;\n    return_likelihoods?: string;\n    logit_bias?: Record<string, any>;\n  };\n\n  export type RequestOptions = SharedRequestOptions;\n\n  export type Generation = {\n    id: string;\n    text: string;\n    index?: number;\n    likelihood?: number;\n    token_likelihoods?: Array<{\n      token: string;\n      likelihood: number;\n    }>;\n  };\n\n  export type Response = {\n    id: string;\n    prompt?: string;\n    generations: Generation[];\n    meta: {\n      api_version: {\n        version: string;\n        is_deprecated?: boolean;\n        is_experimental?: boolean;\n      };\n      warnings?: string[];\n    };\n  };\n\n  export type Chunk = {\n    text?: string;\n    is_finished: boolean;\n    finished_reason?: 'COMPLETE' | 'MAX_TOKENS' | 'ERROR' | 'ERROR_TOXIC';\n    response?: {\n      id: string;\n      prompt?: string;\n      generations: Generation[];\n    };\n  };\n}\n\n/**\n * Run a generation against the Cohere API.\n *\n * @see https://docs.cohere.com/reference/generate\n *\n * @param request The request body sent to Cohere. See Cohere's documentation for /v1/generate for supported parameters.\n * @param options\n * @param options.apiKey Cohere API key.\n * @param options.apiUrl The url of the Cohere (or compatible) API. Defaults to https://api.cohere.ai/v1/generate.\n * @param options.fetch A custom implementation of fetch. Defaults to globalThis.fetch.\n * @param options.headers Optionally add additional HTTP headers to the request.\n * @param options.signal An AbortSignal that can be used to abort the fetch request.\n * @returns Cohere completion. See Cohere's documentation for /v1/generate.\n */\nasync function run(\n  request: CohereGenerationTypes.Request,\n  options: CohereGenerationTypes.RequestOptions,\n): Promise<CohereGenerationTypes.Response> {\n  const url = options.apiUrl || COHERE_API_URL;\n\n  const response = await POST(url, {\n    headers: headers(options.apiKey, options.headers),\n    body: JSON.stringify({ ...request, stream: false }),\n    fetch: options.fetch,\n    signal: options.signal,\n  });\n\n  return response.json();\n}\n\n/**\n * Run a streaming generation against the Cohere API. The resulting stream is the raw unmodified bytes from the API.\n *\n * @see https://docs.cohere.com/reference/generate\n *\n * @param request The request body sent to Cohere. See Cohere's documentation for /v1/generate for supported parameters.\n * @param options\n * @param options.apiKey Cohere API key.\n * @param options.apiUrl The url of the Cohere (or compatible) API. Defaults to https://api.cohere.ai/v1/generate.\n * @param options.fetch A custom implementation of fetch. Defaults to globalThis.fetch.\n * @param options.headers Optionally add additional HTTP headers to the request.\n * @param options.signal An AbortSignal that can be used to abort the fetch request.\n * @returns A stream of bytes directly from the API.\n */\nasync function streamBytes(\n  request: CohereGenerationTypes.Request,\n  options: CohereGenerationTypes.RequestOptions,\n): Promise<ReadableStream<Uint8Array>> {\n  const url = options.apiUrl || COHERE_API_URL;\n\n  const response = await POST(url, {\n    headers: headers(options.apiKey, options.headers),\n    body: JSON.stringify({ ...request, stream: true }),\n    fetch: options.fetch,\n    signal: options.signal,\n  });\n\n  if (!response.body) {\n    throw new HttpError('Expected response body to be a ReadableStream', response);\n  }\n\n  return response.body;\n}\n\nfunction noop(chunk: CohereGenerationTypes.Chunk) {\n  return chunk;\n}\n\n/**\n * Run a streaming generation against the Cohere API. The resulting stream is the parsed stream data as JavaScript objects.\n *\n * @see https://docs.cohere.com/reference/generate\n *\n * @param request The request body sent to Cohere. See Cohere's documentation for /v1/generate for supported parameters.\n * @param options\n * @param options.apiKey Cohere API key.\n * @param options.apiUrl The url of the Cohere (or compatible) API. Defaults to https://api.cohere.ai/v1/generate.\n * @param options.fetch A custom implementation of fetch. Defaults to globalThis.fetch.\n * @param options.headers Optionally add additional HTTP headers to the request.\n * @param options.signal An AbortSignal that can be used to abort the fetch request.\n * @returns A stream of objects representing each chunk from the API.\n */\nasync function stream(\n  request: CohereGenerationTypes.Request,\n  options: CohereGenerationTypes.RequestOptions,\n): Promise<ReadableStream<CohereGenerationTypes.Chunk>> {\n  const byteStream = await streamBytes(request, options);\n  return byteStream.pipeThrough(new CohereGenerationDecoderStream(noop));\n}\n\nfunction chunkToToken(chunk: CohereGenerationTypes.Chunk) {\n  return chunk.text || '';\n}\n\n/**\n * Run a streaming generation against the Cohere API. The resulting stream emits only the string tokens.\n *\n * @see https://docs.cohere.com/reference/generate\n *\n * @param request The request body sent to Cohere. See Cohere's documentation for /v1/generate for supported parameters.\n * @param options\n * @param options.apiKey Cohere API key.\n * @param options.apiUrl The url of the Cohere (or compatible) API. Defaults to https://api.cohere.ai/v1/generate.\n * @param options.fetch A custom implementation of fetch. Defaults to globalThis.fetch.\n * @param options.headers Optionally add additional HTTP headers to the request.\n * @param options.signal An AbortSignal that can be used to abort the fetch request.\n * @returns A stream of tokens from the API.\n */\nasync function streamTokens(\n  request: CohereGenerationTypes.Request,\n  options: CohereGenerationTypes.RequestOptions,\n): Promise<ReadableStream<string>> {\n  const byteStream = await streamBytes(request, options);\n  return byteStream.pipeThrough(new CohereGenerationDecoderStream(chunkToToken));\n}\n\n/**\n * An object that encapsulates methods for calling the Cohere Generate API.\n */\nexport class CohereGeneration {\n  static run = run;\n  static stream = stream;\n  static streamBytes = streamBytes;\n  static streamTokens = streamTokens;\n}\n\nclass CohereGenerationDecoderStream<T> extends TransformStream<Uint8Array, T> {\n  private static parse(line: string): CohereGenerationTypes.Chunk | null {\n    line = line.trim();\n\n    // Empty lines are ignored\n    if (line.length === 0) {\n      return null;\n    }\n\n    try {\n      return JSON.parse(line);\n    } catch (error) {\n      throw new Error(\n        `Invalid event: expected well-formed event lines but got ${JSON.stringify(line)}`,\n      );\n    }\n  }\n\n  private static transformer<T>(map: (chunk: CohereGenerationTypes.Chunk) => T) {\n    let buffer: string[] = [];\n    const decoder = new TextDecoder();\n\n    return (bytes: Uint8Array, controller: TransformStreamDefaultController<T>) => {\n      const chunk = decoder.decode(bytes);\n\n      for (let i = 0, len = chunk.length; i < len; ++i) {\n        // Cohere separates events with '\\n'\n        const isEventSeparator = chunk[\"something\"] === '\\n';\n\n        // Keep buffering unless we've hit the end of an event\n        if (!isEventSeparator) {\n          buffer.push(chunk[i]);\n          continue;\n        }\n\n        const event = CohereGenerationDecoderStream.parse(buffer.join(''));\n\n        if (event) {\n          controller.enqueue(map(event));\n        }\n\n        buffer = [];\n      }\n    };\n  }\n\n  constructor(map: (chunk: CohereGenerationTypes.Chunk) => T) {\n    super({ transform: CohereGenerationDecoderStream.transformer(map) });\n  }\n}\n";
        let language = "typescript";
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang(language)
            .expect("test to work");
        ts_language_config.capture_class_data(source_code.as_bytes());
        let outline = ts_language_config.generate_file_outline_str(source_code.as_bytes());
        assert_eq!(outline, "```typescript\n\nClass CohereGeneration\n\nClass CohereGenerationDecoderStream\n\n    function parse((line: string)): : CohereGenerationTypes.Chunk | null\n    function transformer((map: (chunk: CohereGenerationTypes.Chunk) => T)): \n    function constructor((map: (chunk: CohereGenerationTypes.Chunk) => T)): \nfunction run((\n  request: CohereGenerationTypes.Request,\n  options: CohereGenerationTypes.RequestOptions,\n)): : Promise<CohereGenerationTypes.Response>\n\nfunction streamBytes((\n  request: CohereGenerationTypes.Request,\n  options: CohereGenerationTypes.RequestOptions,\n)): : Promise<ReadableStream<Uint8Array>>\n\nfunction noop((chunk: CohereGenerationTypes.Chunk)): \n\nfunction stream((\n  request: CohereGenerationTypes.Request,\n  options: CohereGenerationTypes.RequestOptions,\n)): : Promise<ReadableStream<CohereGenerationTypes.Chunk>>\n\nfunction chunkToToken((chunk: CohereGenerationTypes.Chunk)): \n\nfunction streamTokens((\n  request: CohereGenerationTypes.Request,\n  options: CohereGenerationTypes.RequestOptions,\n)): : Promise<ReadableStream<string>>\n\n```");
    }

    fn walk(cursor: &mut TreeCursor, indent: usize) {
        loop {
            let node = cursor.node();
            let start_byte = node.start_byte();
            let end_byte = node.end_byte();
            println!(
                "{}{:?}({}:{}): error:{} missing:{}",
                " ".repeat(indent),
                node.kind(),
                start_byte,
                end_byte,
                node.is_error(),
                // TODO(skcd): Found it! We can use this to determine if there are
                // any linter errors and then truncate using this, until we do not introduce
                // any more errors
                node.is_missing(),
            );

            if cursor.goto_first_child() {
                walk(cursor, indent + 2);
                cursor.goto_parent();
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    #[test]
    fn test_typescript_error_parsing() {
        let source_code = r#"
function add(a: number, b: number): number {
    !!!!!!!
    return a + b;
}
"#;
        let language = "typescript";
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang(language)
            .expect("test to work");
        let grammar = ts_language_config.grammar;
        let mut parser = Parser::new();
        parser.set_language(grammar()).unwrap();
        let tree = parser.parse(source_code.as_bytes(), None).unwrap();
        let mut visitors = tree.walk();
        walk(&mut visitors, 0);
        assert!(false);
    }

    fn walk_tree_for_no_errors(
        cursor: &mut TreeCursor,
        inserted_range: &Range,
        indent: usize,
    ) -> bool {
        let mut answer = true;
        loop {
            let node = cursor.node();
            let start_byte = node.start_byte();
            let end_byte = node.end_byte();

            fn check_if_inside_range(
                start_byte: usize,
                end_byte: usize,
                inserted_byte: usize,
            ) -> bool {
                start_byte <= inserted_byte && inserted_byte <= end_byte
            }

            // TODO(skcd): Check this condition here tomorrow so we can handle cases
            // where the missing shows up at the end of the node, because that ends up
            // happening quite often
            fn check_if_intersects_range(
                start_byte: usize,
                end_byte: usize,
                inserted_range: &Range,
            ) -> bool {
                check_if_inside_range(start_byte, end_byte, inserted_range.start_byte())
                    || check_if_inside_range(start_byte, end_byte, inserted_range.end_byte())
            }

            println!(
                "{}{:?}({}:{}): error:{} missing:{} does_intersect({}:{}): {}",
                " ".repeat(indent),
                node.kind(),
                start_byte,
                end_byte,
                node.is_error(),
                // TODO(skcd): Found it! We can use this to determine if there are
                // any linter errors and then truncate using this, until we do not introduce
                // any more errors
                node.is_missing(),
                inserted_range.start_byte(),
                inserted_range.end_byte(),
                check_if_intersects_range(start_byte, end_byte, inserted_range),
            );

            // First check if the node is in the range or
            // the range of the node intersects with the inserted range
            if check_if_intersects_range(
                node.range().start_byte,
                node.range().end_byte,
                inserted_range,
            ) {
                if node.is_error() || node.is_missing() {
                    answer = false;
                    return answer;
                }
            }

            if cursor.goto_first_child() {
                answer = answer && walk_tree_for_no_errors(cursor, inserted_range, indent + 1);
                if !answer {
                    return answer;
                }
                cursor.goto_parent();
            }

            if !cursor.goto_next_sibling() {
                return answer;
            }
        }
    }

    #[test]
    fn test_rust_error_checking() {
        let source_code = r#"use sidecar::{embedder::embedder::Embedder, embedder::embedder::LocalEmbedder};
use std::env;

#[tokio::main]
async fn main() {
    println!("Hello, world! skcd");
    init_ort_dylib();

    // Now we try to create the embedder and see if thats working
    let current_path = env::current_dir().unwrap();
    // Checking that the embedding logic is also working
    let embedder = LocalEmbedder::new(&current_path.join("models/all-MiniLM-L6-v2/")).unwrap();
    let result = embedder.embed("hello world!").unwrap();
    dbg!(result.len());
    dbg!(result);
}

fn add(left:)

fn init_ort_dylib() {
    #[cfg(not(windows))]
    {
        #[cfg(target_os = "linux")]
        let lib_path = "libonnxruntime.so";
        #[cfg(target_os = "macos")]
        let lib_path =
            "/Users/skcd/Downloads/onnxruntime-osx-arm64-1.16.0/lib/libonnxruntime.dylib";

        // let ort_dylib_path = dylib_dir.as_ref().join(lib_name);

        if env::var("ORT_DYLIB_PATH").is_err() {
            env::set_var("ORT_DYLIB_PATH", lib_path);
        }
    }
}"#;
        let language = "rust";
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang(language)
            .expect("test to work");
        let grammar = ts_language_config.grammar;
        let mut parser = Parser::new();
        parser.set_language(grammar()).unwrap();
        // the range we are checking is this:
        // let range = Range {
        //     start_position: Position {
        //         line: 17,
        //         character: 7,
        //         byte_offset: 568,
        //     },
        //     end_position: Position {
        //         line: 17,
        //         character: 13,
        //         byte_offset: 574,
        //     },
        // };
        let range = Range::new(Position::new(17, 7, 568), Position::new(17, 7, 574));
        let tree = parser.parse(source_code.as_bytes(), None).unwrap();
        let mut visitors = tree.walk();
        // walk(&mut visitors, 0);
        walk_tree_for_no_errors(&mut visitors, &range, 0);
        assert!(false);
    }

    #[test]
    fn test_typescript_error_checking() {
        let source_code = r#"class A {
public somethingelse() {}
public something() {
}"#;
        let language = "typescript";
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang(language)
            .expect("test to work");
        let grammar = ts_language_config.grammar;
        let mut parser = Parser::new();
        parser.set_language(grammar()).unwrap();
        let tree = parser.parse(source_code.as_bytes(), None).unwrap();
        let mut visitors = tree.walk();
        walk(&mut visitors, 0);
        assert!(false);
    }

    #[test]
    fn test_outline_parsing() {
        let source_code = r#"
impl SomethingClassImplementation {
    pub fn something(&self, blah: string) {
    }

    pub fn something(&self, blah: string) {
        struct Something {
        }
    }
}

struct SomethingElseStruct {
    pub something: String,
    something_else: String,
}

#[derive(Debug, Clone, serde::Serialize)]
enum SomethingEnum {
}

fn something_else_function() {

}

fn something_else_over_here_wtf() {
    let a = "";
    let b = "";
    let c = "";
}

type SomethingType = string;

trait SomethingTrait {
    fn something_else(&self);
}"#;
        let language = "rust";
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang(language)
            .expect("test to work");
        let grammar = ts_language_config.grammar;
        let mut parser = Parser::new();
        parser.set_language(grammar()).unwrap();
        let tree = parser.parse(source_code.as_bytes(), None).unwrap();
        let outline = ts_language_config.generate_outline(
            source_code.as_bytes(),
            &tree,
            "/tmp/something".to_owned(),
        );
        println!("{:?}", outline);
        assert_eq!(outline.len(), 6);
    }

    #[test]
    fn test_class_with_functions_parsing() {
        let source_code = r#"
        use std::path::PathBuf;

        use regex::Regex;
        use tracing::debug;

        use crate::repo::types::RepoRef;

        use super::{
            javascript::javascript_language_config,
            languages::TSLanguageConfig,
            python::python_language_config,
            rust::rust_language_config,
            text_document::{DocumentSymbol, Position, Range, TextDocument},
            types::FunctionInformation,
            typescript::typescript_language_config,
        };

        /// Here we will parse the document we get from the editor using symbol level
        /// information, as its very fast

        #[derive(Debug, Clone)]
        pub struct EditorParsing {
            configs: Vec<TSLanguageConfig>,
        }

        impl Default for EditorParsing {
            fn default() -> Self {
                Self {
                    configs: vec![
                        rust_language_config(),
                        javascript_language_config(),
                        typescript_language_config(),
                        python_language_config(),
                    ],
                }
            }
        }

        impl EditorParsing {
            pub fn ts_language_config(&self, language: &str) -> Option<&TSLanguageConfig> {
                self.configs
                    .iter()
                    .find(|config| config.language_ids.contains(&language))
            }

            pub fn for_file_path(&self, file_path: &str) -> Option<&TSLanguageConfig> {
                let file_path = PathBuf::from(file_path);
                let file_extension = file_path
                    .extension()
                    .map(|extension| extension.to_str())
                    .map(|extension| extension.to_owned())
                    .flatten();
                match file_extension {
                    Some(extension) => self
                        .configs
                        .iter()
                        .find(|config| config.file_extensions.contains(&extension)),
                    None => None,
                }
            }

            fn is_node_identifier(
                &self,
                node: &tree_sitter::Node,
                language_config: &TSLanguageConfig,
            ) -> bool {
                match language_config
                    .language_ids
                    .first()
                    .expect("language_id to be present")
                    .to_lowercase()
                    .as_ref()
                {
                    "typescript" | "typescriptreact" | "javascript" | "javascriptreact" => {
                        Regex::new(r"(definition|declaration|declarator|export_statement)")
                            .unwrap()
                            .is_match(node.kind())
                    }
                    "golang" => Regex::new(r"(definition|declaration|declarator|var_spec)")
                        .unwrap()
                        .is_match(node.kind()),
                    "cpp" => Regex::new(r"(definition|declaration|declarator|class_specifier)")
                        .unwrap()
                        .is_match(node.kind()),
                    "ruby" => Regex::new(r"(module|class|method|assignment)")
                        .unwrap()
                        .is_match(node.kind()),
                    "rust" => Regex::new(r"(item)").unwrap().is_match(node.kind()),
                    _ => Regex::new(r"(definition|declaration|declarator)")
                        .unwrap()
                        .is_match(node.kind()),
                }
            }

            /**
             * This function aims to process nodes from a tree sitter parsed structure
             * based on their intersection with a given range and identify nodes that
             * represent declarations or definitions specific to a programming language.
             *
             * @param {Object} t - The tree sitter node.
             * @param {Object} e - The range (or point structure) with which intersections are checked.
             * @param {string} r - The programming language (e.g., "typescript", "golang").
             *
             * @return {Object|undefined} - Returns the most relevant node or undefined.
             */
            // function KX(t, e, r) {
            // // Initial setup with the root node and an empty list for potential matches
            // let n = [t.rootNode], i = [];

            // while (true) {
            //     // For each node in 'n', calculate its intersection size with 'e'
            //     let o = n.map(s => [s, rs.intersectionSize(s, e)])
            //              .filter(([s, a]) => a > 0)
            //              .sort(([s, a], [l, c]) => c - a);  // sort in decreasing order of intersection size

            //     // If there are no intersections, either return undefined or the most relevant node from 'i'
            //     if (o.length === 0) return i.length === 0 ? void 0 : tX(i, ([s, a], [l, c]) => a - c)[0];

            //     // For the nodes in 'o', calculate a relevance score and filter the ones that are declarations or definitions for language 'r'
            //     let s = o.map(([a, l]) => {
            //         let c = rs.len(a),  // Length of the node
            //             u = Math.abs(rs.len(e) - l),  // Difference between length of 'e' and its intersection size
            //             p = (l - u) / c;  // Relevance score
            //         return [a, p];
            //     });

            //     // Filter nodes based on the ZL function and push to 'i'
            //     i.push(...s.filter(([a, l]) => ZL(a, r)));

            //     // Prepare for the next iteration by setting 'n' to the children of the nodes in 'o'
            //     n = [];
            //     n.push(...s.flatMap(([a, l]) => a.children));
            // }
            // }
            fn get_identifier_node_fully_contained<'a>(
                &'a self,
                tree_sitter_node: tree_sitter::Node<'a>,
                range: &'a Range,
                language_config: &'a TSLanguageConfig,
                source_str: &str,
            ) -> Option<tree_sitter::Node<'a>> {
                let mut nodes = vec![tree_sitter_node];
                let mut identifier_nodes: Vec<(tree_sitter::Node, f64)> = vec![];
                loop {
                    // Here we take the nodes in [nodes] which have an intersection
                    // with the range we are interested in
                    let mut intersecting_nodes = nodes
                        .into_iter()
                        .map(|tree_sitter_node| {
                            (
                                tree_sitter_node,
                                Range::for_tree_node(&tree_sitter_node).intersection_size(range) as f64,
                            )
                        })
                        .filter(|(_, intersection_size)| intersection_size > &0.0)
                        .collect::<Vec<_>>();
                    // we sort the nodes by their intersection size
                    // we want to keep the biggest size here on the top
                    intersecting_nodes.sort_by(|a, b| b.1.partial_cmp(&a.1).expect("partial_cmp to work"));

                    // if there are no nodes, then we return none or the most relevant nodes
                    // from i, which is the biggest node here
                    if intersecting_nodes.is_empty() {
                        return if identifier_nodes.is_empty() {
                            None
                        } else {
                            Some({
                                let mut current_node = identifier_nodes[0];
                                for identifier_node in &identifier_nodes[1..] {
                                    if identifier_node.1 - current_node.1 > 0.0 {
                                        current_node = identifier_node.clone();
                                    }
                                }
                                current_node.0
                            })
                        };
                    }

                    // For the nodes in intersecting_nodes, calculate a relevance score and filter the ones that are declarations or definitions for language 'language_config'
                    let identifier_nodes_sorted = intersecting_nodes
                        .iter()
                        .map(|(tree_sitter_node, intersection_size)| {
                            let len = Range::for_tree_node(&tree_sitter_node).len();
                            let diff = ((range.len() as f64 - intersection_size) as f64).abs();
                            let relevance_score = (intersection_size - diff) as f64 / len as f64;
                            (tree_sitter_node.clone(), relevance_score)
                        })
                        .collect::<Vec<_>>();

                    // now we filter out the nodes which are here based on the identifier function and set it to identifier nodes
                    // which we want to find for documentation
                    identifier_nodes.extend(
                        identifier_nodes_sorted
                            .into_iter()
                            .filter(|(tree_sitter_node, _)| {
                                self.is_node_identifier(tree_sitter_node, language_config)
                            })
                            .map(|(tree_sitter_node, score)| (tree_sitter_node, score))
                            .collect::<Vec<_>>(),
                    );

                    // Now we prepare for the next iteration by setting nodes to the children of the nodes
                    // in intersecting_nodes
                    nodes = intersecting_nodes
                        .into_iter()
                        .map(|(tree_sitter_node, _)| {
                            let mut cursor = tree_sitter_node.walk();
                            tree_sitter_node.children(&mut cursor).collect::<Vec<_>>()
                        })
                        .flatten()
                        .collect::<Vec<_>>();
                }
            }

            fn get_identifier_node_by_expanding<'a>(
                &'a self,
                tree_sitter_node: tree_sitter::Node<'a>,
                range: &Range,
                language_config: &TSLanguageConfig,
            ) -> Option<tree_sitter::Node<'a>> {
                let tree_sitter_range = range.to_tree_sitter_range();
                let mut expanding_node = tree_sitter_node
                    .descendant_for_byte_range(tree_sitter_range.start_byte, tree_sitter_range.end_byte);
                loop {
                    // Here we expand this node until we hit a identifier node, this is
                    // a very easy way to get to the best node we are interested in by
                    // bubbling up
                    if expanding_node.is_none() {
                        return None;
                    }
                    match expanding_node {
                        Some(expanding_node_val) => {
                            // if this is not a identifier and the parent is there, we keep
                            // going up
                            if !self.is_node_identifier(&expanding_node_val, &language_config)
                                && expanding_node_val.parent().is_some()
                            {
                                expanding_node = expanding_node_val.parent()
                            // if we have a node identifier, return right here!
                            } else if self.is_node_identifier(&expanding_node_val, &language_config) {
                                return Some(expanding_node_val.clone());
                            } else {
                                // so we don't have a node identifier and neither a parent, so
                                // just return None
                                return None;
                            }
                        }
                        None => {
                            return None;
                        }
                    }
                }
            }

            pub fn get_documentation_node(
                &self,
                text_document: &TextDocument,
                language_config: &TSLanguageConfig,
                range: Range,
            ) -> Vec<DocumentSymbol> {
                let language = language_config.grammar;
                let source = text_document.get_content_buffer();
                let mut parser = tree_sitter::Parser::new();
                parser.set_language(language()).unwrap();
                let tree = parser
                    .parse(text_document.get_content_buffer().as_bytes(), None)
                    .unwrap();
                if let Some(identifier_node) = self.get_identifier_node_fully_contained(
                    tree.root_node(),
                    &range,
                    &language_config,
                    source,
                ) {
                    // we have a identifier node right here, so lets get the document symbol
                    // for this and return it back
                    return DocumentSymbol::from_tree_node(
                        &identifier_node,
                        language_config,
                        text_document.get_content_buffer(),
                    )
                    .into_iter()
                    .collect();
                }
                // or else we try to expand the node out so we can get a symbol back
                if let Some(expanded_node) =
                    self.get_identifier_node_by_expanding(tree.root_node(), &range, &language_config)
                {
                    // we get the expanded node here again
                    return DocumentSymbol::from_tree_node(
                        &expanded_node,
                        language_config,
                        text_document.get_content_buffer(),
                    )
                    .into_iter()
                    .collect();
                }
                // or else we return nothing here
                vec![]
            }

            pub fn get_documentation_node_for_range(
                &self,
                source_str: &str,
                language: &str,
                relative_path: &str,
                fs_file_path: &str,
                start_position: &Position,
                end_position: &Position,
                repo_ref: &RepoRef,
            ) -> Vec<DocumentSymbol> {
                // First we need to find the language config which matches up with
                // the language we are interested in
                let language_config = self.ts_language_config(&language);
                if let None = language_config {
                    return vec![];
                }
                // okay now we have a language config, lets parse it
                self.get_documentation_node(
                    &TextDocument::new(
                        source_str.to_owned(),
                        repo_ref.clone(),
                        fs_file_path.to_owned(),
                        relative_path.to_owned(),
                    ),
                    &language_config.expect("if let None check above to work"),
                    Range::new(start_position.clone(), end_position.clone()),
                )
            }

            pub fn function_information_nodes(
                &self,
                source_code: &[u8],
                language: &str,
            ) -> Vec<FunctionInformation> {
                let language_config = self.ts_language_config(&language);
                if let None = language_config {
                    return vec![];
                }
                language_config
                    .expect("if let None check above")
                    .function_information_nodes(source_code)
            }
        }

        #[cfg(test)]
        mod tests {
            use crate::{
                chunking::{
                    languages::TSLanguageParsing,
                    text_document::{Position, Range, TextDocument},
                },
                repo::types::RepoRef,
            };

            use super::EditorParsing;

            #[test]
            fn rust_selection_parsing() {
                let editor_parsing = EditorParsing::default();
                // This is from the configuration file
                let source_str = "use std::{\n    num::NonZeroUsize,\n    path::{Path, PathBuf},\n};\n\nuse clap::Parser;\nuse serde::{Deserialize, Serialize};\n\nuse crate::repo::state::StateSource;\n\n#[derive(Serialize, Deserialize, Parser, Debug, Clone)]\n#[clap(author, version, about, long_about = None)]\npub struct Configuration {\n    #[clap(short, long, default_value_os_t = default_index_dir())]\n    #[serde(default = \"default_index_dir\")]\n    /// Directory to store all persistent state\n    pub index_dir: PathBuf,\n\n    #[clap(long, default_value_t = default_port())]\n    #[serde(default = \"default_port\")]\n    /// Bind the webserver to `<host>`\n    pub port: u16,\n\n    #[clap(long)]\n    /// Path to the embedding model directory\n    pub model_dir: PathBuf,\n\n    #[clap(long, default_value_t = default_host())]\n    #[serde(default = \"default_host\")]\n    /// Bind the webserver to `<port>`\n    pub host: String,\n\n    #[clap(flatten)]\n    #[serde(default)]\n    pub state_source: StateSource,\n\n    #[clap(short, long, default_value_t = default_parallelism())]\n    #[serde(default = \"default_parallelism\")]\n    /// Maximum number of parallel background threads\n    pub max_threads: usize,\n\n    #[clap(short, long, default_value_t = default_buffer_size())]\n    #[serde(default = \"default_buffer_size\")]\n    /// Size of memory to use for file indexes\n    pub buffer_size: usize,\n\n    /// Qdrant url here can be mentioned if we are running it remotely or have\n    /// it running on its own process\n    #[clap(long)]\n    #[serde(default = \"default_qdrant_url\")]\n    pub qdrant_url: String,\n\n    /// The folder where the qdrant binary is present so we can start the server\n    /// and power the qdrant client\n    #[clap(short, long)]\n    pub qdrant_binary_directory: Option<PathBuf>,\n\n    /// The location for the dylib directory where we have the runtime binaries\n    /// required for ort\n    #[clap(short, long)]\n    pub dylib_directory: PathBuf,\n\n    /// Qdrant allows us to create collections and we need to provide it a default\n    /// value to start with\n    #[clap(short, long, default_value_t = default_collection_name())]\n    #[serde(default = \"default_collection_name\")]\n    pub collection_name: String,\n\n    #[clap(long, default_value_t = interactive_batch_size())]\n    #[serde(default = \"interactive_batch_size\")]\n    /// Batch size for batched embeddings\n    pub embedding_batch_len: NonZeroUsize,\n\n    #[clap(long, default_value_t = default_user_id())]\n    #[serde(default = \"default_user_id\")]\n    user_id: String,\n\n    /// If we should poll the local repo for updates auto-magically. Disabled\n    /// by default, until we figure out the delta sync method where we only\n    /// reindex the files which have changed\n    #[clap(long)]\n    pub enable_background_polling: bool,\n}\n\nimpl Configuration {\n    /// Directory where logs are written to\n    pub fn log_dir(&self) -> PathBuf {\n        self.index_dir.join(\"logs\")\n    }\n\n    pub fn index_path(&self, name: impl AsRef<Path>) -> impl AsRef<Path> {\n        self.index_dir.join(name)\n    }\n\n    pub fn qdrant_storage(&self) -> PathBuf {\n        self.index_dir.join(\"qdrant_storage\")\n    }\n}\n\nfn default_index_dir() -> PathBuf {\n    match directories::ProjectDirs::from(\"ai\", \"codestory\", \"sidecar\") {\n        Some(dirs) => dirs.data_dir().to_owned(),\n        None => \"codestory_sidecar\".into(),\n    }\n}\n\nfn default_port() -> u16 {\n    42424\n}\n\nfn default_host() -> String {\n    \"127.0.0.1\".to_owned()\n}\n\npub fn default_parallelism() -> usize {\n    std::thread::available_parallelism().unwrap().get()\n}\n\nconst fn default_buffer_size() -> usize {\n    100_000_000\n}\n\nfn default_collection_name() -> String {\n    \"codestory\".to_owned()\n}\n\nfn interactive_batch_size() -> NonZeroUsize {\n    NonZeroUsize::new(1).unwrap()\n}\n\nfn default_qdrant_url() -> String {\n    \"http://127.0.0.1:6334\".to_owned()\n}\n\nfn default_user_id() -> String {\n    \"codestory\".to_owned()\n}\n";
                let range = Range::new(Position::new(134, 7, 3823), Position::new(137, 0, 3878));
                let ts_lang_parsing = TSLanguageParsing::init();
                let rust_config = ts_lang_parsing.for_lang("rust");
                let mut documentation_nodes = editor_parsing.get_documentation_node(
                    &TextDocument::new(
                        source_str.to_owned(),
                        RepoRef::local("/Users/skcd/testing/").expect("test to work"),
                        "".to_owned(),
                        "".to_owned(),
                    ),
                    &rust_config.expect("rust config to be present"),
                    range,
                );
                assert!(!documentation_nodes.is_empty());
                let first_entry = documentation_nodes.remove(0);
                assert_eq!(first_entry.start_position, Position::new(134, 0, 3816));
                assert_eq!(first_entry.end_position, Position::new(136, 1, 3877));
            }
        }
        "#;
        let language = "rust";
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang(language)
            .expect("test to work");
        let file_symbols = ts_language_config.generate_file_symbols(source_code.as_bytes());
        dbg!(&file_symbols);
        assert!(false);
    }

    #[test]
    fn test_parsing_go_code_for_outline() {
        let source_code = r#"
        type Person struct {
            Name string
            Age  int
        }

        func createPerson(name string, age int) Person {
            return Person{Name: name, Age: age}
        }

        func (c *Person) AreaSomething() float64 {
            return math.Pi * c.Radius * c.Radius
        }
        "#;
        let language = "go";
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang(language)
            .expect("to be present");
        let mut parser = Parser::new();
        let grammar = ts_language_config.grammar;
        parser.set_language(grammar()).unwrap();
        let tree = parser.parse(source_code.as_bytes(), None).unwrap();
        let outlines = ts_language_config.generate_outline(
            source_code.as_bytes(),
            &tree,
            "/tmp/something".to_owned(),
        );
        // we have 1 function in the type

        assert_eq!(outlines[0].children_len(), 1);
        assert_eq!(outlines.len(), 2);
    }

    #[test]
    fn test_parsing_python_code_for_outline_nodes() {
        let source_code = r#"
class Something:
    def __init__():
        pass

    def something_else(self, blah, blah2):
        print(blah)
        print(blah2)
        pass

def something_else_function(self, a, b, c) -> sss:
    print(a)
    print(b)
    pass
        "#;
        let language = "python";
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang(language)
            .expect("to be present");
        let mut parser = Parser::new();
        let grammar = ts_language_config.grammar;
        parser.set_language(grammar()).unwrap();
        let tree = parser.parse(source_code.as_bytes(), None).unwrap();
        let outlines = ts_language_config.generate_outline(
            source_code.as_bytes(),
            &tree,
            "/tmp/something.py".to_owned(),
        );
        println!("{:?}", &outlines);
        assert!(false);
    }

    #[test]
    fn test_parsing_python_functions_with_decorators() {
        let source_code = r#"
class Something:
    def __init__():
        a = b
        c = d
        pass

    @classmethod
    def something_else(cls, blah, blah2):
        print(blah)
        print(blah2)
        e = f
        g = h
        pass

def something_else_function(self, a, b, c) -> sss:
    print(a)
    print(b)
    blah = blah2
    pass

something_else = interesting
a.b.c = {
    a: c,
    d: e,
    f: g,
}
        "#;
        let language = "python";
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang(language)
            .expect("to be present");
        let mut parser = Parser::new();
        let grammar = ts_language_config.grammar;
        parser.set_language(grammar()).unwrap();
        let tree = parser.parse(source_code.as_bytes(), None).unwrap();
        let outlines = ts_language_config.generate_outline(
            source_code.as_bytes(),
            &tree,
            "/tmp/something.py".to_owned(),
        );
        // we are also including the identifier node over here
        assert_eq!(outlines.len(), 4);
    }

    #[test]
    fn test_captures_rust_class_attribute_items() {
        let source_code = r#"
#[derive(Debug, Clone)]
pub struct Something {{
    a: String,
}}

#[derive(
    Debug,
    Clone,
)]
pub struct SomethingSplitLines {{
    a: String,
}}

pub struct NormalBoringStruct {{
    a: String,
}}"#;
        let language = "rust";
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang(language)
            .expect("to be present");
        let mut parser = Parser::new();
        let grammar = ts_language_config.grammar;
        parser.set_language(grammar()).unwrap();
        let tree = parser.parse(source_code.as_bytes(), None).unwrap();
        let outlines = ts_language_config.generate_outline(
            source_code.as_bytes(),
            &tree,
            "/tmp/something.rs".to_owned(),
        );
        assert_eq!(outlines.len(), 3);
        // the outline for this class starts at the #[derive(...)] position
        assert_eq!(outlines[0].range().start_line(), 1);
    }

    #[test]
    fn test_parse_struct_inside_function_rust() {
        let source_code = r#"
impl Something {{
    fn new() {{
        struct SomethingElse {{
        }}
    }}
}}

pub struct SomethingWorking {{

}}
        "#;
        let language = "rust";
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang(language)
            .expect("to be present");
        let mut parser = Parser::new();
        let grammar = ts_language_config.grammar;
        parser.set_language(grammar()).unwrap();
        let tree = parser.parse(source_code.as_bytes(), None).unwrap();
        let outlines = ts_language_config.generate_outline(
            source_code.as_bytes(),
            &tree,
            "/tmp/something.rs".to_owned(),
        );
        assert_eq!(outlines.len(), 2);
        assert_eq!(outlines[0].name(), "Something");
    }

    #[test]
    fn test_formatting_outline_nodes() {
        let source_code = r#"
impl Something {
    fn new() {
        // random content over here
        // more random content over here
        // something else
    }

    #[derive(Debug, blah)]
    fn something_interesting() {
        // random content over here
    }
}

#[derive(Debug, Clone)]
struct SomethingInteresting {
    a: String,
    b: String,
    c: String,
}
        "#;
        let language = "rust";
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang(&language)
            .expect("to be present");
        let mut parser = Parser::new();
        let grammar = ts_language_config.grammar;
        parser.set_language(grammar()).unwrap();
        let tree = parser.parse(source_code.as_bytes(), None).unwrap();
        let outlines = ts_language_config.generate_outline(
            source_code.as_bytes(),
            &tree,
            "/tmp/something.rs".to_owned(),
        );
        outlines.into_iter().for_each(|outline| {
            let content_for_prompt = outline.get_outline_for_prompt();
            // check that none of the content is empty over here
            assert!(!content_for_prompt.is_empty());
        });
    }

    #[test]
    fn test_hover_query_rust() {
        let source_code = r#"
fn agent_router() -> Router {
    use axum::routing::*;
    Router::new()
        .route(
            "/search_agent",
            get(sidecar::webserver::agent::search_agent),
        )
        .route(
            "/hybrid_search",
            get(sidecar::webserver::agent::hybrid_search),
        )
        .route("/explain", get(sidecar::webserver::agent::explain))
        .route(
            "/followup_chat",
            post(sidecar::webserver::agent::followup_chat),
        )
}
        "#;
        let language = "rust";
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang(&language)
            .expect("to be present");
        let mut parser = Parser::new();
        let grammar = ts_language_config.grammar;
        parser.set_language(grammar()).unwrap();
        let hoverable_ranges = ts_language_config.hoverable_nodes(source_code.as_bytes());
        assert!(!hoverable_ranges.is_empty());
    }

    #[test]
    fn test_object_qualifier() {
        let cases = vec![
            (
                "rust",
                r#"
            Self::go()
            "#,
                "Self",
            ),
            (
                "javascript",
                r#"
            hotel.method()
            "#,
                "hotel",
            ),
            (
                "typescript",
                r#"
                hotel.method()
            "#,
                "hotel",
            ),
            (
                "python",
                r#"
            hotel.method()
            "#,
                "hotel",
            ),
            (
                "go",
                r#"
            hotel.method()
            "#,
                "hotel",
            ),
        ];
        for (language, source_code, expected_qualifier) in cases {
            let tree_sitter_parsing = TSLanguageParsing::init();
            let ts_language_config = tree_sitter_parsing
                .for_lang(language)
                .expect("language config to be present");
            let object_qualifier =
                ts_language_config.generate_object_qualifier(source_code.as_bytes());
            assert!(object_qualifier.is_some());
            let range = object_qualifier.unwrap();
            let extracted_text = &source_code[range.start_byte()..range.end_byte()];
            assert_eq!(extracted_text, expected_qualifier);
        }
    }

    #[test]
    fn test_trait_implementation_tracking() {
        let source_code = r#"
impl Something::interesting for SomethingElse {

}
impl TraitSomething for Something {
}

struct SomethingElse {
}

struct Something {
}

enum SomethingElse {
}
        "#;
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang("rust")
            .expect("language config to be present");
        let grammar = ts_language_config.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let tree = parser.parse(source_code, None).unwrap();
        let outline_nodes = ts_language_config.generate_outline(
            source_code.as_bytes(),
            &tree,
            "/tmp/something.rs".to_owned(),
        );
        assert_eq!(outline_nodes.len(), 5);
        assert_eq!(
            outline_nodes[0].content().has_trait_implementation(),
            Some("Something::interesting".to_owned())
        );
        assert_eq!(
            outline_nodes[1].content().has_trait_implementation(),
            Some("TraitSomething".to_owned())
        );
        assert_eq!(outline_nodes[2].content().has_trait_implementation(), None,);
        assert_eq!(outline_nodes[3].content().has_trait_implementation(), None,);
        assert_eq!(outline_nodes[4].content().has_trait_implementation(), None,);
    }

    /// This is a failing test case, we want to track this in the code
    #[test]
    fn failing_object_qualifier_test_case() {
        let cases = vec![("rust", "Self::go_do_something", "Self")];
        for (language, source_code, _expected_qualifier) in cases.into_iter() {
            let tree_sitter_parsing = TSLanguageParsing::init();
            let ts_language_config = tree_sitter_parsing
                .for_lang(language)
                .expect("language config to be present");
            let object_qualifier =
                ts_language_config.generate_object_qualifier(source_code.as_bytes());
            assert!(object_qualifier.is_none());
        }
    }

    #[test]
    fn test_class_declaration_and_definition() {
        let source_code = r#"
struct Something {
}

impl Something {
}

enum Something {
}"#;
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang("rust")
            .expect("language config to be present");
        let grammar = ts_language_config.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let tree = parser.parse(source_code, None).unwrap();
        let outline_nodes = ts_language_config.generate_outline(
            source_code.as_bytes(),
            &tree,
            "/tmp/something.rs".to_owned(),
        );
        assert_eq!(outline_nodes.len(), 3);
        assert_eq!(
            outline_nodes[0].outline_node_type(),
            &OutlineNodeType::ClassDefinition,
        );
        assert_eq!(
            outline_nodes[1].outline_node_type(),
            &OutlineNodeType::Class,
        );
        assert_eq!(
            outline_nodes[2].outline_node_type(),
            &OutlineNodeType::ClassDefinition,
        );
    }

    #[test]
    fn test_parsing_outline_nodes_for_typescript_error() {
        let source_code = r#"
private isDefaultConfigurationAllowed(configuration: IConfigurationNode): boolean {
    return configuration.disallowConfigurationDefault !== true;
}
        "#;
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang("typescript")
            .expect("language config to be present");
        let grammar = ts_language_config.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let tree = parser.parse(source_code, None).unwrap();
        let outline_nodes = ts_language_config.generate_outline(
            source_code.as_bytes(),
            &tree,
            "/tmp/something.rs".to_owned(),
        );
        assert!(outline_nodes.is_empty());
    }

    #[test]
    fn test_parsing_outline_nodes_compressed() {
        let source_code = r#"
fn something() {
    // something over here
}

impl Blah for SomethingElse {
    fn something_important() {
        // something over here
    }

    fn something_important_over_here() {
        // something important over here
    }
}

struct SomethingExtremelyImportant {
    important_field: String,
}
        "#;
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang("rust")
            .expect("language config to be present");
        let grammar = ts_language_config.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let tree = parser.parse(source_code, None).unwrap();
        let outline_nodes = ts_language_config.generate_outline(
            source_code.as_bytes(),
            &tree,
            "/tmp/something.rs".to_owned(),
        );
        assert!(!outline_nodes.is_empty());
        assert_eq!(outline_nodes.len(), 3);
        let output = outline_nodes[0]
            .get_outline_node_compressed()
            .expect("to work");
        assert_eq!(
            output,
            r#"FILEPATH: /tmp/something.rs
fn something() {"#
        );
        let output = outline_nodes[1]
            .get_outline_node_compressed()
            .expect("to work");
        assert_eq!(
            output,
            r#"FILEPATH: /tmp/something.rs
impl Blah for SomethingElse {

    fn something_important() {
    fn something_important_over_here() {
}"#
        );
        let output = outline_nodes[2]
            .get_outline_node_compressed()
            .expect("to work");
        assert_eq!(
            output,
            r#"FILEPATH: /tmp/something.rs
struct SomethingExtremelyImportant {
    important_field: String,
}"#
        );
    }

    #[test]
    fn test_outline_nodes_for_rust() {
        let source_code = r#"#[derive(Debug, Clone)]
enum Something {
    entry1,
    entry2,
}"#;
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang("rust")
            .expect("language config to be present");
        let grammar = ts_language_config.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let tree = parser.parse(source_code, None).unwrap();
        let outline_nodes = ts_language_config.generate_outline(
            source_code.as_bytes(),
            &tree,
            "/tmp/something.rs".to_owned(),
        );
        assert_eq!(outline_nodes.len(), 1);
        assert!(outline_nodes[0].is_class_definition());
        let compressed_node = outline_nodes[0].get_outline_node_compressed();
        assert!(compressed_node.is_some());
        let compressed_node = compressed_node.expect("is_some to hold");
        println!("{}", &compressed_node);
        assert!(false);
    }

    #[test]
    fn test_outline_nodes_class_definitions_for_typescript() {
        let source_code = r#"interface Something {
    a: string;
    b: string;
}

export type IAideProbeProgress =
    | IAideChatMarkdownContent
    | IAideProbeBreakdownContent
    | IAideProbeGoToDefinition
    | IAideProbeTextEdit
    | IAideProbeOpenFile
    | IAideProbeRepoMapGeneration
    | IAideProbeLongContextSearch
    | IAideProbeInitialSymbols"#;
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang("typescript")
            .expect("language config to be present");
        let grammar = ts_language_config.grammar;
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(grammar()).unwrap();
        let tree = parser.parse(source_code, None).unwrap();
        let outline_nodes = ts_language_config.generate_outline(
            source_code.as_bytes(),
            &tree,
            "/tmp/something.ts".to_owned(),
        );
        assert_eq!(outline_nodes.len(), 2);
        assert!(outline_nodes[0].is_class_definition());
        assert!(outline_nodes[1].is_class_definition());
    }

    #[test]
    fn test_function_required_parameters() {
        let source_code = r#"fn something(a: A, b: B) -> Result<C, E> {
            // something
        }"#;
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang("rust")
            .expect("language config to be present");
        let function_clickable_nodes =
            ts_language_config.generate_function_insights(source_code.as_bytes());
        assert_eq!(
            function_clickable_nodes
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
            vec!["A", "B", "Result", "C", "E"]
        );
    }

    #[test]
    fn test_outline_nodes_parsing_rust_with_decorators() {
        let source_code = r#"#[derive(Debug, Clone)]
struct Something {
    a: String,
}"#;
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang("rust")
            .expect("language config to be present");
        let outline_nodes =
            ts_language_config.generate_outline_fresh(source_code.as_bytes(), "/tmp/something.rs");
        println!("{:?}", &outline_nodes);
        assert!(false);
    }

    #[test]
    fn test_function_call_paths_capturing() {
        let source_code = r#"
fn something() {
    let a = b.c.d.e.f(sdfsdfsdf);
    let b = A::b::c(sdfsfsdfsdf);
}
"#;
        let tree_sitter_parsing = TSLanguageParsing::init();
        let ts_language_config = tree_sitter_parsing
            .for_lang("rust")
            .expect("language config to be present");
        let function_call_paths =
            ts_language_config.generate_function_call_paths(source_code.as_bytes());
        assert!(function_call_paths.is_some());
        let function_call_paths = function_call_paths.expect("assert! to hold");
        assert_eq!(
            vec!["b.c.d.e.f", "A::b::c"],
            function_call_paths
                .into_iter()
                .map(|(symbol, _)| symbol)
                .collect::<Vec<_>>()
        );
    }
}
