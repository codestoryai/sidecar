use std::collections::HashMap;

use super::text_document::Range;

/// Some common types which can be reused across calls

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct FunctionNodeInformation {
    name: String,
    parameters: String,
    body: String,
    return_type: String,
    documentation: Option<String>,
    variables: Vec<(String, Range)>,
    class_name: Option<String>,
    parameter_identifiers: Vec<(String, Range)>,
}

impl FunctionNodeInformation {
    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    pub fn set_parameters(&mut self, parameters: String) {
        self.parameters = parameters;
    }

    pub fn set_body(&mut self, body: String) {
        self.body = body;
    }

    pub fn set_return_type(&mut self, return_type: String) {
        self.return_type = return_type;
    }

    pub fn set_variable_name(&mut self, variable_name: String, variable_range: Range) {
        self.variables.push((variable_name, variable_range));
    }

    pub fn set_class_name(&mut self, class_name: String) {
        self.class_name = Some(class_name);
    }

    pub fn add_parameter_identifier(&mut self, parameter_identifier: String, range: Range) {
        self.parameter_identifiers
            .push((parameter_identifier, range));
    }

    pub fn set_documentation(&mut self, documentation: String) {
        self.documentation = Some(documentation);
    }

    pub fn get_class_name(&self) -> Option<&str> {
        self.class_name.as_deref()
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_parameters(&self) -> &str {
        &self.parameters
    }

    pub fn get_return_type(&self) -> &str {
        &self.return_type
    }

    pub fn get_documentation(&self) -> Option<&str> {
        self.documentation.as_deref()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Hash)]
pub enum OutlineNodeType {
    // the trait of the class if present, this represents the trait implementation which
    // might be part of the symbol, its not necessarily always present in every language
    // but it is a part of rust
    ClassTrait,
    // the defintion of the class if the language supports it (like rust, golang) struct A {...}
    // otherwise its inside the class struct (in languages like js, ts) class A {something: string; something_else: string}
    ClassDefinition,
    // The identifier for the complete class body
    Class,
    // the name of the class
    ClassName,
    // the identifier for the complete function body
    Function,
    // the name of the funciton
    FunctionName,
    // the body of the function
    FunctionBody,
    // function class name
    FunctionClassName,
    // The function parameter identifier
    FunctionParameterIdentifier,
    // The decorators which are present on top of functions/classes
    Decorator,
    // Assignment definition for all the constants etc which are present globally
    // but are relevant to the symbol
    DefinitionAssignment,
    // The identifier for the definition or the constant which we are interested in
    DefinitionIdentifier,
    // Represents a file in the outline
    File,
}

impl OutlineNodeType {
    pub fn is_function(&self) -> bool {
        matches!(self, OutlineNodeType::Function)
            || matches!(self, OutlineNodeType::FunctionName)
            || matches!(self, OutlineNodeType::FunctionBody)
            || matches!(self, OutlineNodeType::FunctionClassName)
            || matches!(self, OutlineNodeType::FunctionParameterIdentifier)
    }

    pub fn is_definition_assignment(&self) -> bool {
        matches!(self, OutlineNodeType::DefinitionAssignment)
    }

    pub fn is_class_definition(&self) -> bool {
        matches!(self, OutlineNodeType::ClassDefinition)
    }

    pub fn is_class_implementation(&self) -> bool {
        matches!(self, OutlineNodeType::Class)
    }
}

impl OutlineNodeType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "definition.class.trait" => Some(Self::ClassTrait),
            "definition.class.declaration" => Some(Self::ClassDefinition),
            "definition.class" => Some(Self::Class),
            "definition.class.name" => Some(Self::ClassName),
            "definition.function" | "definition.method" => Some(Self::Function),
            "function.name" => Some(Self::FunctionName),
            "function.body" => Some(Self::FunctionBody),
            "class.function.name" => Some(Self::FunctionClassName),
            "parameter.identifier" => Some(Self::FunctionParameterIdentifier),
            "decorator" => Some(Self::Decorator),
            "definition.assignment" => Some(Self::DefinitionAssignment),
            "definition.identifier" => Some(Self::DefinitionIdentifier),
            "file" => Some(Self::File),
            _ => None,
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Self::ClassTrait => "definition.class.trait".to_owned(),
            Self::ClassDefinition => "definition.class.declaration".to_owned(),
            Self::Class => "definition.class".to_owned(),
            Self::ClassName => "definition.class.name".to_owned(),
            Self::Function => "definition.function".to_owned(),
            Self::FunctionName => "function.name".to_owned(),
            Self::FunctionBody => "function.body".to_owned(),
            Self::FunctionClassName => "class.function.name".to_owned(),
            Self::FunctionParameterIdentifier => "parameter.identifier".to_owned(),
            Self::Decorator => "decorator".to_owned(),
            Self::DefinitionAssignment => "definition.assignment".to_owned(),
            Self::DefinitionIdentifier => "definition.identifier".to_owned(),
            Self::File => "file".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, std::hash::Hash)]
pub struct OutlineNodeContent {
    range: Range,
    name: String,
    r#type: OutlineNodeType,
    // The content here gives the outline of the node which we are interested in
    content: String,
    fs_file_path: String,
    identifier_range: Range,
    body_range: Range,
    language: String,
    trait_implementation: Option<String>,
}

impl OutlineNodeContent {
    pub fn new(
        name: String,
        range: Range,
        r#type: OutlineNodeType,
        content: String,
        fs_file_path: String,
        identifier_range: Range,
        body_range: Range,
        language: String,
        trait_implementation: Option<String>,
    ) -> Self {
        Self {
            range,
            name,
            r#type,
            content,
            fs_file_path,
            identifier_range,
            body_range,
            language,
            trait_implementation,
        }
    }

    pub fn file_symbol(
        name: String,
        range: Range,
        content: String,
        fs_file_path: String,
        language: String,
    ) -> Self {
        Self {
            range: range.clone(),
            name,
            r#type: OutlineNodeType::File,
            content,
            fs_file_path,
            identifier_range: range.clone(),
            body_range: range,
            language,
            trait_implementation: None,
        }
    }

    pub fn class_implementation_symbol(
        name: String,
        range: Range,
        content: String,
        fs_file_path: String,
        identifier_range: Range,
        language: String,
    ) -> Self {
        Self {
            range: range.clone(),
            name,
            r#type: OutlineNodeType::Class,
            content,
            fs_file_path,
            identifier_range,
            body_range: range,
            language,
            trait_implementation: None,
        }
    }

    pub fn class_definition_symbol(
        name: String,
        range: Range,
        content: String,
        fs_file_path: String,
        identifier_range: Range,
        language: String,
    ) -> Self {
        Self {
            range: range.clone(),
            name,
            r#type: OutlineNodeType::ClassDefinition,
            content,
            fs_file_path,
            identifier_range,
            body_range: range,
            language,
            trait_implementation: None,
        }
    }

    pub fn function_symbol(
        name: String,
        range: Range,
        content: String,
        fs_file_path: String,
        identifier_range: Range,
        language: String,
    ) -> Self {
        Self {
            range: range.clone(),
            name,
            r#type: OutlineNodeType::Function,
            content,
            fs_file_path,
            identifier_range,
            body_range: range,
            language,
            trait_implementation: None,
        }
    }

    /// Overrides the range for the outline node content
    pub fn set_range(mut self, range: Range) -> Self {
        self.range = range;
        self
    }

    /// Overrides the content for the outline node content
    pub fn set_content(mut self, content: String) -> Self {
        self.content = content;
        self
    }

    pub fn is_class_declaration(&self) -> bool {
        matches!(self.outline_node_type(), OutlineNodeType::ClassDefinition)
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    pub fn to_xml(&self) -> String {
        let name = &self.name;
        let file_path = &self.fs_file_path;
        let start_line = self.range.start_line();
        let end_line = self.range.end_line();
        let content = &self.content;
        let language = &self.language;
        format!(
            r#"<name>
{name}
</name>
<file_path>
{file_path}:{start_line}-{end_line}
</file_path>
<content>
```{language}
{content}
```
</content>"#
        )
        .to_owned()
    }

    // we try to get the non overlapping lines from our content
    pub fn get_non_overlapping_content(&self, range: &[&Range]) -> Option<(String, Range)> {
        let lines = self
            .content
            .lines()
            .into_iter()
            .enumerate()
            .map(|(idx, line)| (idx + self.range().start_line(), line.to_owned()))
            .filter(|(idx, _)| !range.into_iter().any(|range| range.contains_line(*idx)))
            .filter(|(_, line)| {
                // we want to filter out the lines which are not part of
                if line == "}" || line.is_empty() {
                    false
                } else {
                    true
                }
            })
            .map(|(_, line)| line)
            .collect::<Vec<String>>();
        let mut start_positions = range
            .into_iter()
            .map(|range| range.start_position())
            .collect::<Vec<_>>();
        start_positions.sort_by_key(|position| position.line());
        if lines.is_empty() {
            None
        } else {
            if start_positions.is_empty() {
                Some((lines.join("\n"), self.range.clone()))
            } else {
                Some((
                    lines.join("\n"),
                    Range::new(
                        self.range.start_position(),
                        start_positions.remove(0).move_to_previous_line(),
                    ),
                ))
            }
        }
    }

    pub fn range(&self) -> &Range {
        &self.range
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn outline_node_type(&self) -> &OutlineNodeType {
        &self.r#type
    }

    pub fn is_class_definition(&self) -> bool {
        self.outline_node_type().is_class_definition()
    }

    pub fn is_class_type(&self) -> bool {
        self.outline_node_type().is_class_definition()
            || self.outline_node_type().is_class_implementation()
    }

    pub fn is_function_type(&self) -> bool {
        self.outline_node_type().is_function()
    }

    pub fn identifier_range(&self) -> &Range {
        &self.identifier_range
    }

    pub fn has_trait_implementation(&self) -> Option<String> {
        self.trait_implementation.clone()
    }
}

#[derive(Debug, Clone, PartialEq, std::hash::Hash, Eq, serde::Serialize)]
pub struct OutlineNode {
    content: OutlineNodeContent,
    children: Vec<OutlineNodeContent>,
    language: String,
}

impl OutlineNode {
    pub fn new(
        content: OutlineNodeContent,
        children: Vec<OutlineNodeContent>,
        language: String,
    ) -> Self {
        Self {
            content,
            children,
            language,
        }
    }

    /// Gives back a unique string representation of the outline node which can be used
    /// to de-duplicate outline nodes
    pub fn unique_identifier(&self) -> String {
        format!(
            "{}-{}-{}-{}",
            self.content.name(),
            self.content.fs_file_path(),
            self.identifier_range().start_line(),
            self.identifier_range().end_line()
        )
    }

    pub fn identifier_range(&self) -> &Range {
        self.content.identifier_range()
    }

    pub fn fs_file_path(&self) -> &str {
        self.content.fs_file_path()
    }

    pub fn outline_node_type(&self) -> &OutlineNodeType {
        self.content.outline_node_type()
    }

    pub fn new_from_child(outline_node_content: OutlineNodeContent, language: String) -> Self {
        Self {
            content: outline_node_content,
            children: vec![],
            language,
        }
    }

    pub fn consume_content(self) -> OutlineNodeContent {
        self.content
    }

    pub fn content(&self) -> &OutlineNodeContent {
        &self.content
    }

    pub fn consume_all_outlines(self) -> Vec<OutlineNodeContent> {
        vec![self.content]
            .into_iter()
            .chain(self.children)
            .collect()
    }

    pub fn children(&self) -> &[OutlineNodeContent] {
        self.children.as_slice()
    }

    pub fn children_len(&self) -> usize {
        self.children.len()
    }

    pub fn add_children(&mut self, mut children: Vec<OutlineNodeContent>) {
        self.children.append(&mut children);
    }

    pub fn range(&self) -> &Range {
        &self.content.range
    }

    pub fn name(&self) -> &str {
        &self.content.name
    }

    pub fn is_class_definition(&self) -> bool {
        matches!(self.content.r#type, OutlineNodeType::ClassDefinition)
    }

    pub fn is_class(&self) -> bool {
        matches!(self.content.r#type, OutlineNodeType::Class)
    }

    pub fn is_file(&self) -> bool {
        matches!(self.content.r#type, OutlineNodeType::File)
    }

    pub fn is_function(&self) -> bool {
        matches!(self.content.r#type, OutlineNodeType::Function)
    }

    /// Grabs the outline of this node similar to how we are showing things
    /// in the repo map
    /// extremely useful for just giving an overview to the AI to start selecting
    /// symbols to start with
    pub fn get_outline_for_prompt(&self) -> String {
        match &self.content.r#type {
            OutlineNodeType::Class | OutlineNodeType::ClassDefinition => {
                let start_line = self.range().start_line();
                let end_line = self.range().end_line();
                let mut line_numbers_included = (start_line..=end_line)
                    .map(|line_number| (line_number, true))
                    .collect::<HashMap<usize, bool>>();
                // mark all the lines covered by functions as not included so we can
                // generate a better outline
                self.children.iter().for_each(|children| {
                    let child_range = children.range();
                    let start_line = child_range.start_line();
                    let end_line = child_range.end_line();
                    (start_line..=end_line).into_iter().for_each(|line_number| {
                        if let Some(line_content) = line_numbers_included.get_mut(&line_number) {
                            *line_content = false;
                        }
                    });
                });
                self.children.iter().for_each(|children| {
                    // println!("{:?}", children.name());
                    // println!("{:?}", &children.body_range);
                    // println!("{:?}", &children.range);
                    // we have the body range for the children
                    // but we are missing the range for the

                    // the body range is from where the body of the function starts
                    // and the complete range also includes the prefix with the comments
                    // or any decorator which is present on the function
                    // so we start at the start of the complete range but stop at the
                    // start line of the body range
                    // body range is always included in the compelete range
                    let body_range = children.body_range;
                    let complete_range = children.range();
                    let start_line = complete_range.start_line();
                    // this is not exactly correct but I think it should work out
                    let end_line = body_range.start_line();
                    (start_line..=end_line).into_iter().for_each(|line_number| {
                        if let Some(line_content) = line_numbers_included.get_mut(&line_number) {
                            *line_content = true;
                        }
                    })
                });

                // Now just grab the lines which have true
                let node_start_line = self.range().start_line();
                let content_lines = self
                    .content()
                    .content()
                    .lines()
                    .enumerate()
                    .into_iter()
                    .map(|(idx, line)| (idx + node_start_line, line.to_string()))
                    .filter_map(|(idx, line)| {
                        if let Some(include_line) = line_numbers_included.get(&idx) {
                            if *include_line {
                                Some(line)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                content_lines
            }
            OutlineNodeType::Function => {
                let complete_range = self.content.range();
                let body_range = &self.content.body_range;
                let start_line = complete_range.start_line();
                let end_line = body_range.start_line();
                let mut line_numbers_included = (complete_range.start_line()
                    ..=complete_range.end_line())
                    .into_iter()
                    .map(|line_number| (line_number, true))
                    .collect::<HashMap<usize, bool>>();
                (start_line..=end_line).into_iter().for_each(|line_number| {
                    if let Some(line_content) = line_numbers_included.get_mut(&line_number) {
                        *line_content = true;
                    }
                });
                let content_lines = self
                    .content()
                    .content()
                    .lines()
                    .enumerate()
                    .into_iter()
                    .map(|(idx, line)| (idx + complete_range.start_line(), line.to_string()))
                    .filter_map(|(idx, line)| {
                        if let Some(include_line) = line_numbers_included.get(&idx) {
                            if *include_line {
                                Some(line)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                content_lines
            }
            _ => "".to_owned(),
        }
    }

    pub fn get_outline_short(&self) -> String {
        // we have to carefully construct this over here, but for now we just return
        // the content
        match &self.content.r#type {
            OutlineNodeType::Class | OutlineNodeType::ClassDefinition => {
                let start_line = self.range().start_line();
                let end_line = self.range().end_line();
                let mut line_numbers_included = (start_line..=end_line)
                    .map(|line_number| (line_number, true))
                    .collect::<HashMap<usize, bool>>();
                self.children.iter().for_each(|children| {
                    let child_range = children.range();
                    let start_line = child_range.start_line();
                    let end_line = child_range.end_line();
                    (start_line..=end_line).into_iter().for_each(|line_number| {
                        if let Some(line_content) = line_numbers_included.get_mut(&line_number) {
                            *line_content = false;
                        }
                    });
                });
                let _ = self
                    .content
                    .content()
                    .lines()
                    .into_iter()
                    .enumerate()
                    .map(|(idx, line)| (idx + start_line, line.to_owned()))
                    .collect::<Vec<_>>();

                // we only keep the lines which are not covered by any of the functions over here
                // since these are important for the outline as well

                // now we have the line numbers here which should not be included
                // for the functions which we have we want to get their outline as well, so we can keep going
                // TODO(skcd): Pick this up from here and complete getting the outline for the functions
                // as well and constructing a correct outline for the symbol
            }
            OutlineNodeType::Function => {
                // if this is a function we want to get the outline over here and just keep that
                // the outline here involves getting the function defintion and the return type mostly
            }
            _ => {}
        }
        self.content.content.to_owned()
    }

    fn outline_node_compressed_function(&self, function: &OutlineNodeContent) -> String {
        let function_range = function.range();
        let start_line = function_range.start_line();
        let function_outline = function
            .content()
            .lines()
            .enumerate()
            .into_iter()
            .map(|(idx, content)| (idx + start_line, content.to_owned()))
            .filter_map(|(line_number, content)| {
                if line_number <= function.body_range.start_line() {
                    Some(content)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        function_outline
    }

    pub fn get_outline_node_compressed(&self) -> Option<String> {
        match &self.content.r#type {
            OutlineNodeType::Class | OutlineNodeType::ClassDefinition => {
                if &self.language == "rust"
                    && self.content.r#type == OutlineNodeType::ClassDefinition
                {
                    let file_path = self.fs_file_path();
                    let content = self.content().content().to_owned();
                    Some(format!(
                        r#"FILEPATH: {file_path}
{content}"#
                    ))
                } else {
                    // Now we have an impl block over here for which we want to get the outline
                    let children_compressed = self
                        .children()
                        .into_iter()
                        .map(|children_node| self.outline_node_compressed_function(children_node))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let start_line = self.range().start_line();
                    // now we need to get the lines which are not part of any functions
                    let symbol_content_without_children = self
                        .content()
                        .content()
                        .lines()
                        .enumerate()
                        .map(|(idx, content)| (idx + start_line, content.to_owned()))
                        .filter_map(|(line_number, content)| {
                            if self.children().into_iter().any(|children_node| {
                                children_node.range().contains_line(line_number)
                            }) {
                                None
                            } else {
                                Some(content)
                            }
                        })
                        .filter(|content| {
                            // only keep lines which are not empty over here
                            !content.is_empty()
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let symbol_content_without_children_lines =
                        symbol_content_without_children.lines().collect::<Vec<_>>();
                    if symbol_content_without_children_lines.len() == 1 {
                        let file_path = self.fs_file_path();
                        let formatted_lines = format!(
                            r#"FILEPATH: {file_path}
{symbol_content_without_children}
{children_compressed}"#
                        );
                        Some(formatted_lines)
                    } else {
                        // take the last line and insert the children compressed
                        // represenatation inside the range over here
                        if let Some((last, remaining)) =
                            symbol_content_without_children_lines.split_last()
                        {
                            let remaining = remaining
                                .into_iter()
                                .map(|str_value| str_value.to_owned())
                                .collect::<Vec<_>>()
                                .join("\n");
                            let file_path = self.fs_file_path();
                            Some(format!(
                                r#"FILEPATH: {file_path}
{remaining}
{children_compressed}
{last}"#
                            ))
                        } else {
                            let file_path = self.fs_file_path();
                            Some(format!(
                                r#"FILEPATH: {file_path}
{symbol_content_without_children}
{children_compressed}"#
                            ))
                        }
                    }
                }
            }
            OutlineNodeType::Function => {
                let file_path = self.fs_file_path();
                let outline_compressed = self.outline_node_compressed_function(self.content());
                Some(format!(
                    r#"FILEPATH: {file_path}
{outline_compressed}"#
                ))
            }
            _ => None,
        }
    }

    pub fn get_outline(&self) -> Option<String> {
        // we want to generate the outline for the node here, we have to do some
        // language specific gating here but thats fine
        match &self.content.r#type {
            OutlineNodeType::Class => {
                if self.children.is_empty() {
                    Some(self.content.content.to_owned())
                } else {
                    // for rust we have a special case here as we might have functions
                    // inside which we want to show but its part of the implementation
                    if &self.language == "rust" {
                        // this is 100% a implementation unless over here, so lets use
                        // it as such
                        let implementation_name = self.content.name.to_owned();
                        let children_content = self
                            .children
                            .iter()
                            .map(|children| children.content.to_owned())
                            .collect::<Vec<_>>()
                            .join("\n");
                        Some(format!(
                            "impl {implementation_name} {{\n{children_content}\n}}"
                        ))
                    } else {
                        // TODO(skcd): We will figure out support for other languages
                        None
                    }
                }
            }
            OutlineNodeType::Function => None,
            _ => None,
        }
    }

    pub fn check_smallest_member_in_range(&self, range: &Range) -> Option<OutlineNode> {
        match &self.content.r#type {
            OutlineNodeType::Class => {
                if self.content.range().contains_check_line_column(&range) {
                    // cool we have some hits probably now, so there can be 2 cases
                    // here, one of them is that the hit is in the struct defintion
                    // and the other is that its in the implementations, in some language
                    // they are the one and same but we can still try our best right now
                    let possible_hit = self
                        .children
                        .iter()
                        .filter(|children| children.range().contains_check_line_column(&range))
                        .next();
                    match possible_hit {
                        Some(hit) => {
                            // return the function which we are getting a hit with
                            Some(OutlineNode::new(
                                hit.clone(),
                                vec![],
                                self.language.to_owned(),
                            ))
                        }
                        None => {
                            // return the whole class itself
                            Some(OutlineNode::new(
                                self.content.clone(),
                                vec![],
                                self.language.to_owned(),
                            ))
                        }
                    }
                } else {
                    None
                }
            }
            OutlineNodeType::Function => {
                if self.content.range().contains_check_line_column(&range) {
                    Some(self.clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionNodeType {
    // The identifier for the function
    Identifier,
    // The body of the function without the identifier
    Body,
    // The full function with its name and the body
    Function,
    // The parameters of the function
    Parameters,
    // The return type of the function
    ReturnType,
    // Class name
    ClassName,
    // Add parameter identifier nodes
    ParameterIdentifier,
}

impl FunctionNodeType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "identifier" => Some(Self::Identifier),
            "body" => Some(Self::Body),
            "function" => Some(Self::Function),
            "parameters" => Some(Self::Parameters),
            "return_type" => Some(Self::ReturnType),
            "class.function.name" => Some(Self::ClassName),
            "parameter.identifier" => Some(Self::ParameterIdentifier),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FunctionInformation {
    range: Range,
    r#type: FunctionNodeType,
    node_information: Option<FunctionNodeInformation>,
}

impl FunctionInformation {
    pub fn new(range: Range, r#type: FunctionNodeType) -> Self {
        Self {
            range,
            r#type,
            node_information: None,
        }
    }

    pub fn name(&self) -> Option<&str> {
        self.node_information.as_ref().map(|info| info.get_name())
    }

    pub fn class_name(&self) -> Option<&str> {
        self.node_information
            .as_ref()
            .map(|info| info.get_class_name())
            .flatten()
    }

    pub fn get_node_information(&self) -> Option<&FunctionNodeInformation> {
        self.node_information.as_ref()
    }

    pub fn set_node_information(&mut self, node_information: FunctionNodeInformation) {
        self.node_information = Some(node_information);
    }

    pub fn set_documentation(&mut self, documentation: String) {
        if let Some(node_information) = &mut self.node_information {
            node_information.set_documentation(documentation);
        }
    }

    pub fn insert_identifier_node(&mut self, identiifer_name: String, identifier_range: Range) {
        if let Some(node_information) = &mut self.node_information {
            node_information.set_variable_name(identiifer_name, identifier_range);
        }
    }

    pub fn get_identifier_nodes(&self) -> Option<&Vec<(String, Range)>> {
        self.node_information.as_ref().map(|info| &info.variables)
    }

    pub fn get_function_parameters(&self) -> Option<&Vec<(String, Range)>> {
        self.node_information
            .as_ref()
            .map(|info| &info.parameter_identifiers)
    }

    pub fn range(&self) -> &Range {
        &self.range
    }

    pub fn r#type(&self) -> &FunctionNodeType {
        &self.r#type
    }

    pub fn content(&self, file_content: &str) -> String {
        file_content[self.range().start_byte()..self.range().end_byte()].to_owned()
    }

    pub fn find_function_in_byte_offset<'a>(
        function_blocks: &'a [&'a Self],
        byte_offset: usize,
    ) -> Option<&'a Self> {
        let mut possible_function_block = None;
        for function_block in function_blocks.into_iter() {
            // if the end byte for this block is greater than the current byte
            // position and the start byte is greater than the current bytes
            // position as well, we have our function block
            if !(function_block.range().end_byte() < byte_offset) {
                if function_block.range().start_byte() > byte_offset {
                    break;
                }
                possible_function_block = Some(function_block);
            }
        }
        possible_function_block.copied()
    }

    pub fn get_expanded_selection_range(
        function_bodies: &[&FunctionInformation],
        selection_range: &Range,
    ) -> Range {
        let mut start_position = selection_range.start_position();
        let mut end_position = selection_range.end_position();
        let selection_start_fn_body =
            Self::find_function_in_byte_offset(function_bodies, selection_range.start_byte());
        let selection_end_fn_body =
            Self::find_function_in_byte_offset(function_bodies, selection_range.end_byte());

        // What we are trying to do here is expand our selection to cover the whole
        // function if we have to
        if let Some(selection_start_function) = selection_start_fn_body {
            // check if we can expand the range a bit here
            if start_position.to_byte_offset() > selection_start_function.range().start_byte() {
                start_position = selection_start_function.range().start_position();
            }
            // check if the function block ends after our current selection
            if selection_start_function.range().end_byte() > end_position.to_byte_offset() {
                end_position = selection_start_function.range().end_position();
            }
        }
        if let Some(selection_end_function) = selection_end_fn_body {
            // check if we can expand the start position byte here a bit
            if selection_end_function.range().start_byte() < start_position.to_byte_offset() {
                start_position = selection_end_function.range().start_position();
            }
            if selection_end_function.range().end_byte() > end_position.to_byte_offset() {
                end_position = selection_end_function.range().end_position();
            }
        }
        Range::new(start_position, end_position)
    }

    pub fn fold_function_blocks(mut function_blocks: Vec<Self>) -> Vec<Self> {
        // First we sort the function blocks(which are bodies) based on the start
        // index or the end index
        function_blocks.sort_by(|a, b| {
            a.range()
                .start_byte()
                .cmp(&b.range().start_byte())
                .then_with(|| b.range().end_byte().cmp(&a.range().end_byte()))
        });

        // Now that these are sorted we only keep the ones which are not overlapping
        // or fully contained in the other one
        let mut filtered_function_blocks = Vec::new();
        let mut index = 0;

        while index < function_blocks.len() {
            filtered_function_blocks.push(function_blocks[index].clone());
            let mut iterate_index = index + 1;
            while iterate_index < function_blocks.len()
                && function_blocks[index]
                    .range()
                    .is_contained(&function_blocks[iterate_index].range())
            {
                iterate_index += 1;
            }
            index = iterate_index;
        }

        filtered_function_blocks
    }

    pub fn add_documentation_to_functions(
        mut function_blocks: Vec<Self>,
        documentation_entries: Vec<(Range, String)>,
    ) -> Vec<Self> {
        // First we sort the function blocks based on the start index or the end index
        function_blocks.sort_by(|a, b| {
            a.range()
                .start_byte()
                .cmp(&b.range().start_byte())
                .then_with(|| b.range().end_byte().cmp(&a.range().end_byte()))
        });
        let documentation_entires = concat_documentation_string(documentation_entries);
        // now we want to concat the functions to the documentation strings
        // we will use a 2 pointer approach here and keep track of what the current function is and what the current documentation string is
        function_blocks
            .into_iter()
            .map(|mut function_block| {
                documentation_entires
                    .iter()
                    .for_each(|documentation_entry| {
                        if function_block.range().start_line() != 0
                            && documentation_entry.0.end_line()
                                == function_block.range().start_line() - 1
                        {
                            // we have a documentation entry which is right above the function block
                            // we will add this to the function block
                            function_block.set_documentation(documentation_entry.1.to_owned());
                            // we will also update the function block range to include the documentation entry
                            function_block
                                .range
                                .set_start_position(documentation_entry.0.start_position());
                        }
                    });
                // Here we will look for the documentation entries which are just one line above the function range and add that to the function
                // context and update the function block range
                function_block
            })
            .collect()
    }

    pub fn add_identifier_nodes(
        mut function_blocks: Vec<Self>,
        identifier_nodes: Vec<(String, Range)>,
    ) -> Vec<Self> {
        // First we sort the function blocks based on the start index or the end index
        function_blocks.sort_by(|a, b| {
            a.range()
                .start_byte()
                .cmp(&b.range().start_byte())
                .then_with(|| b.range().end_byte().cmp(&a.range().end_byte()))
        });
        function_blocks
            .into_iter()
            .map(|mut function_block| {
                identifier_nodes.iter().for_each(|identifier_node| {
                    let name = &identifier_node.0;
                    let range = identifier_node.1;
                    if function_block.range().contains(&range) {
                        function_block.insert_identifier_node(name.to_owned(), range);
                    }
                });
                function_block
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClassNodeType {
    Identifier,
    ClassDeclaration,
}

impl ClassNodeType {
    pub fn from_str(s: &str) -> Option<ClassNodeType> {
        match s {
            "identifier" => Some(Self::Identifier),
            "class_declaration" => Some(Self::ClassDeclaration),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClassInformation {
    range: Range,
    name: String,
    class_node_type: ClassNodeType,
    documentation: Option<String>,
}

impl ClassInformation {
    pub fn new(range: Range, name: String, class_node_type: ClassNodeType) -> Self {
        Self {
            range,
            name,
            class_node_type,
            documentation: None,
        }
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    pub fn get_class_type(&self) -> &ClassNodeType {
        &self.class_node_type
    }

    pub fn range(&self) -> &Range {
        &self.range
    }

    pub fn set_documentation(&mut self, documentation: String) {
        self.documentation = Some(documentation);
    }

    pub fn content(&self, content: &str) -> String {
        content[self.range().start_byte()..self.range().end_byte()].to_string()
    }

    pub fn fold_class_information(mut classes: Vec<Self>) -> Vec<Self> {
        // First we sort the function blocks(which are bodies) based on the start
        // index or the end index
        classes.sort_by(|a, b| {
            a.range()
                .start_byte()
                .cmp(&b.range().start_byte())
                .then_with(|| b.range().end_byte().cmp(&a.range().end_byte()))
        });

        // Now that these are sorted we only keep the ones which are not overlapping
        // or fully contained in the other one
        let mut filtered_classes = Vec::new();
        let mut index = 0;

        while index < classes.len() {
            filtered_classes.push(classes[index].clone());
            let mut iterate_index = index + 1;
            while iterate_index < classes.len()
                && classes[index]
                    .range()
                    .is_contained(&classes[iterate_index].range())
            {
                iterate_index += 1;
            }
            index = iterate_index;
        }

        filtered_classes
    }

    pub fn add_documentation_to_classes(
        mut class_blocks: Vec<Self>,
        documentation_entries: Vec<(Range, String)>,
    ) -> Vec<Self> {
        // First we sort the function blocks based on the start index or the end index
        class_blocks.sort_by(|a, b| {
            a.range()
                .start_byte()
                .cmp(&b.range().start_byte())
                .then_with(|| b.range().end_byte().cmp(&a.range().end_byte()))
        });
        let documentation_entires = concat_documentation_string(documentation_entries);
        // now we want to concat the functions to the documentation strings
        // we will use a 2 pointer approach here and keep track of what the current function is and what the current documentation string is
        class_blocks
            .into_iter()
            .map(|mut class_block| {
                documentation_entires
                    .iter()
                    .for_each(|documentation_entry| {
                        if class_block.range().start_line() != 0
                            && documentation_entry.0.end_line()
                                == class_block.range().start_line() - 1
                        {
                            // we have a documentation entry which is right above the function block
                            // we will add this to the function block
                            class_block.set_documentation(documentation_entry.1.to_owned());
                            // we will also update the function block range to include the documentation entry
                            class_block
                                .range
                                .set_start_position(documentation_entry.0.start_position());
                        }
                    });
                // Here we will look for the documentation entries which are just one line above the function range and add that to the function
                // context and update the function block range
                class_block
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct ClassWithFunctions {
    pub class_information: Option<ClassInformation>,
    pub function_information: Vec<FunctionInformation>,
}

impl ClassWithFunctions {
    pub fn class_functions(
        class_information: ClassInformation,
        function_information: Vec<FunctionInformation>,
    ) -> Self {
        Self {
            class_information: Some(class_information),
            function_information,
        }
    }

    pub fn functions(function_information: Vec<FunctionInformation>) -> Self {
        Self {
            class_information: None,
            function_information,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeNodeType {
    Identifier,
    TypeDeclaration,
}

#[derive(Debug, Clone)]
pub struct TypeInformation {
    pub range: Range,
    pub name: String,
    pub node_type: TypeNodeType,
    pub documentation: Option<String>,
}

impl TypeNodeType {
    pub fn from_str(s: &str) -> Option<TypeNodeType> {
        match s {
            "identifier" => Some(Self::Identifier),
            "type_declaration" => Some(Self::TypeDeclaration),
            _ => None,
        }
    }
}

impl TypeInformation {
    pub fn new(range: Range, name: String, type_node_type: TypeNodeType) -> Self {
        Self {
            range,
            name,
            node_type: type_node_type,
            documentation: None,
        }
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    pub fn set_documentation(&mut self, documentation: String) {
        self.documentation = Some(documentation);
    }

    pub fn get_type_type(&self) -> &TypeNodeType {
        &self.node_type
    }

    pub fn range(&self) -> &Range {
        &self.range
    }

    pub fn content(&self, content: &str) -> String {
        content[self.range().start_byte()..self.range().end_byte()].to_string()
    }

    pub fn fold_type_information(mut types: Vec<Self>) -> Vec<Self> {
        // First we sort the function blocks(which are bodies) based on the start
        // index or the end index
        types.sort_by(|a, b| {
            a.range()
                .start_byte()
                .cmp(&b.range().start_byte())
                .then_with(|| b.range().end_byte().cmp(&a.range().end_byte()))
        });

        // Now that these are sorted we only keep the ones which are not overlapping
        // or fully contained in the other one
        let mut filtered_types = Vec::new();
        let mut index = 0;

        while index < types.len() {
            filtered_types.push(types[index].clone());
            let mut iterate_index = index + 1;
            while iterate_index < types.len()
                && types[index]
                    .range()
                    .is_contained(&types[iterate_index].range())
            {
                iterate_index += 1;
            }
            index = iterate_index;
        }

        filtered_types
    }

    pub fn add_documentation_to_types(
        mut type_blocks: Vec<Self>,
        documentation_entries: Vec<(Range, String)>,
    ) -> Vec<Self> {
        // First we sort the function blocks based on the start index or the end index
        type_blocks.sort_by(|a, b| {
            a.range()
                .start_byte()
                .cmp(&b.range().start_byte())
                .then_with(|| b.range().end_byte().cmp(&a.range().end_byte()))
        });
        let documentation_entires = concat_documentation_string(documentation_entries);
        // now we want to concat the functions to the documentation strings
        // we will use a 2 pointer approach here and keep track of what the current function is and what the current documentation string is
        type_blocks
            .into_iter()
            .map(|mut type_block| {
                documentation_entires
                    .iter()
                    .for_each(|documentation_entry| {
                        if type_block.range().start_line() != 0
                            && documentation_entry.0.end_line()
                                == type_block.range().start_line() - 1
                        {
                            // we have a documentation entry which is right above the function block
                            // we will add this to the function block
                            type_block.set_documentation(documentation_entry.1.to_owned());
                            // we will also update the function block range to include the documentation entry
                            type_block
                                .range
                                .set_start_position(documentation_entry.0.start_position());
                        }
                    });
                // Here we will look for the documentation entries which are just one line above the function range and add that to the function
                // context and update the function block range
                type_block
            })
            .collect()
    }
}

pub fn concat_documentation_string(
    mut documentation_entries: Vec<(Range, String)>,
) -> Vec<(Range, String)> {
    // we also sort the doucmentation entries based on the start index or the end index
    documentation_entries.sort_by(|a, b| {
        a.0.start_byte()
            .cmp(&b.0.start_byte())
            .then_with(|| b.0.end_byte().cmp(&a.0.end_byte()))
    });
    // We also want to concat the documentation entires if they are right after one another for example:
    // // This is a comment
    // // This is another comment
    // fn foo() {}
    // We want to make sure that we concat the comments into one
    let mut documentation_index = 0;
    let mut concatenated_documentation_queries: Vec<(Range, String)> = Vec::new();
    while documentation_index < documentation_entries.len() {
        let mut iterate_index = documentation_index + 1;
        let mut current_index_end_line = documentation_entries[documentation_index].0.end_line();
        let mut documentation_str = documentation_entries[documentation_index].1.to_owned();
        let mut documentation_range = documentation_entries[documentation_index].0.clone();

        // iterate over consecutive entries in the comments
        while iterate_index < documentation_entries.len()
            && current_index_end_line + 1 == documentation_entries[iterate_index].0.start_line()
        {
            current_index_end_line = documentation_entries[iterate_index].0.end_line();
            documentation_str = documentation_str + "\n" + &documentation_entries[iterate_index].1;
            documentation_range
                .set_end_position(documentation_entries[iterate_index].0.end_position());
            iterate_index += 1;
        }
        concatenated_documentation_queries.push((documentation_range, documentation_str));
        documentation_index = iterate_index;
        // either we hit the end of we have a bunch of documentation entries which are consecutive
        // we know what the comment should be and we can add a new entry
    }
    concatenated_documentation_queries
}

#[cfg(test)]
mod tests {
    use crate::chunking::text_document::Position;
    use crate::chunking::text_document::Range;

    use super::concat_documentation_string;

    #[test]
    fn test_documentation_string_concatenation() {
        let documentation_strings = vec![
            (
                Range::new(Position::new(0, 0, 0), Position::new(0, 0, 0)),
                "first_comment".to_owned(),
            ),
            (
                Range::new(Position::new(1, 0, 0), Position::new(1, 0, 0)),
                "second_comment".to_owned(),
            ),
            (
                Range::new(Position::new(4, 0, 0), Position::new(6, 0, 0)),
                "third_multi_line_comment".to_owned(),
            ),
            (
                Range::new(Position::new(7, 0, 0), Position::new(7, 0, 0)),
                "fourth_comment".to_owned(),
            ),
        ];
        let final_documentation_strings = concat_documentation_string(documentation_strings);
        assert_eq!(final_documentation_strings.len(), 2);
    }
}
