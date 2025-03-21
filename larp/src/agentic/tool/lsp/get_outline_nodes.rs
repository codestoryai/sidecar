//! The editor provides DocumentSymbols which we can use to map back
//! to the outline nodes which we need over here
//! The clutch is that its not perfect and there are language specific
//! tricks which we need to pull off properly, but before we start doing
//! that we should see how well it works for the languages we are interested in

use async_trait::async_trait;
use logging::new_client;

use crate::{
    agentic::tool::{
        errors::ToolError,
        input::ToolInput,
        output::ToolOutput,
        r#type::{Tool, ToolRewardScale},
    },
    chunking::{
        text_document::{Position, Range},
        types::{OutlineNode, OutlineNodeContent, OutlineNodeType},
    },
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OutlineNodesUsingEditorRequest {
    fs_file_path: String,
    editor_url: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DocumentSymbolPosition {
    line: usize,
    character: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DocumentSymbolRange {
    start: DocumentSymbolPosition,
    end: DocumentSymbolPosition,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DocumentSymbol {
    /// The name of this symbol.
    pub name: String,

    /// More detail for this symbol, e.g. the signature of a function.
    pub detail: Option<String>,

    /// The kind of this symbol.
    pub kind: usize,

    /// The range enclosing this symbol not including leading/trailing whitespace
    /// but everything else, e.g. comments and code.
    pub range: [DocumentSymbolPosition; 2],

    /// The range that should be selected and reveal when this symbol is being picked,
    /// e.g. the name of a function. Must be contained by the `range`.
    #[serde(rename = "selectionRange")]
    pub selection_range: [DocumentSymbolPosition; 2],

    /// Children of this symbol, e.g. properties of a class.
    #[serde(default)]
    pub children: Vec<DocumentSymbol>,
}

impl DocumentSymbol {
    fn range(&self) -> Range {
        Range::new(
            Position::new(self.range[0].line, self.range[0].character, 0),
            Position::new(self.range[1].line, self.range[1].character, 0),
        )
    }

    // This is similar to the display name of the symbol, not the real symbol
    // name which is used by the OutlineNode
    // we should add something in OutlineNode to capture this information
    pub fn decription_name(&self) -> &str {
        &self.name
    }

    fn identifier_range(&self) -> Range {
        Range::new(
            Position::new(
                self.selection_range[0].line,
                self.selection_range[0].character,
                0,
            ),
            Position::new(
                self.selection_range[1].line,
                self.selection_range[1].character,
                0,
            ),
        )
    }
}

pub enum SymbolKind {
    File,
    Module,
    Namespace,
    Package,
    Class,
    Method,
    Property,
    Field,
    Constructor,
    Enum,
    Interface,
    Function,
    Variable,
    Constant,
    String,
    Number,
    Boolean,
    Array,
    Object,
    Key,
    Null,
    EnumMember,
    Struct,
    Event,
    Operator,
    TypeParameter,
}

impl SymbolKind {
    /// Convert a usize to SymbolKind
    pub fn from_usize(value: usize) -> Option<Self> {
        match value {
            0 => Some(Self::File),
            // in case of module we want to keep going deeper
            // as this is not a top level symbol which we consider
            1 => Some(Self::Module),
            // for namespace as well, we want to keep going deeper
            2 => Some(Self::Namespace),
            // same for package as well, we want to keep going deeper
            3 => Some(Self::Package),
            // class here is the Struct in rust land so we do want to classify
            // this as class declaration
            4 => Some(Self::Class),
            // method can be a function inside class which we want to track
            5 => Some(Self::Method),
            // we can ignore this safely for now
            6 => Some(Self::Property),
            // similarly for this we can ignore this safely
            7 => Some(Self::Field),
            // special but not really, ends up being a function infact in most languages
            8 => Some(Self::Constructor),
            // this gets mapped to the class declaration
            9 => Some(Self::Enum),
            // this should also get mapped to class declaration
            10 => Some(Self::Interface),
            // this can be a global function or a method in a class
            11 => Some(Self::Function),
            // only track if this is global and belongs to a file or a module
            12 => Some(Self::Variable),
            // we want to track this for rust like languages only if this is global
            13 => Some(Self::Constant),
            // ignore for now
            14 => Some(Self::String),
            // ignore for now
            15 => Some(Self::Number),
            // ignore for now
            16 => Some(Self::Boolean),
            // ignore for now
            17 => Some(Self::Array),
            // ignore for now
            18 => Some(Self::Object),
            // ignore for now
            19 => Some(Self::Key),
            // ignore for now
            20 => Some(Self::Null),
            // ignore for now
            21 => Some(Self::EnumMember),
            // this is the impl block in most cases so we can classify this as
            // class instead (and in python/js land, this will be the class itself)
            22 => Some(Self::Struct),
            // ignore for now
            23 => Some(Self::Event),
            // ignore for now
            24 => Some(Self::Operator),
            // ignore for now
            25 => Some(Self::TypeParameter),
            _ => None,
        }
    }
}

/// We expect the LSP to not mess up anything for us
/// And the outline node we are interested in is fetched from the IDE
fn name_from_selection_range(file_lines: &[&str], range: [DocumentSymbolPosition; 2]) -> String {
    let start_line = range[0].line;
    let file_line_content = file_lines[start_line];
    let character_range = file_line_content.chars().into_iter().collect::<Vec<_>>();
    character_range[range[0].character..range[1].character]
        .into_iter()
        .collect()
}

/// now we want to convert this back to the OutlineNode types we are interested in
/// from the documentSymbol
/// to go about this the right way we have to go through all the documentSymbols and figure
/// out which one we are talking about and why and translate them properly
pub fn document_symbols_to_outline_nodes(
    language: String,
    fs_file_path: String,
    file_content: &str,
    document_symbols: Vec<DocumentSymbol>,
) -> Vec<OutlineNode> {
    let file_lines = file_content.lines().into_iter().collect::<Vec<_>>();
    document_symbols
        .into_iter()
        .filter_map(|document_symbol| {
            let symbol_type = SymbolKind::from_usize(document_symbol.kind);
            symbol_type.map(|symbol_type| (document_symbol, symbol_type))
        })
        .filter_map(|(document_symbol, symbol_type)| {
            match symbol_type {
                // cursed code, comparing 25 types down to OutlineNodeType 12 types
                // is a bit of a challenge, its still okay tho so all good
                SymbolKind::Class
                | SymbolKind::Enum
                | SymbolKind::Struct
                | SymbolKind::TypeParameter => {
                    let class_node = OutlineNodeContent::class_definition_symbol(
                        name_from_selection_range(
                            file_lines.as_slice(),
                            document_symbol.selection_range.clone(),
                        ),
                        document_symbol.range(),
                        file_lines[document_symbol.range().start_line()
                            ..=document_symbol.range().end_line()]
                            .to_vec()
                            .join("\n"),
                        fs_file_path.to_owned(),
                        document_symbol.identifier_range(),
                        language.to_owned(),
                    );
                    let children = document_symbols_to_outline_nodes(
                        language.to_owned(),
                        fs_file_path.to_owned(),
                        file_content,
                        document_symbol.children,
                    );
                    Some(vec![OutlineNode::new(
                        class_node,
                        children
                            .into_iter()
                            .map(|child| child.consume_content())
                            .collect::<Vec<_>>(),
                        language.to_owned(),
                    )])
                }
                SymbolKind::Function | SymbolKind::Method => {
                    let function_node = OutlineNodeContent::function_symbol(
                        name_from_selection_range(
                            file_lines.as_slice(),
                            document_symbol.selection_range.clone(),
                        ),
                        document_symbol.range(),
                        file_lines[document_symbol.range().start_line()
                            ..=document_symbol.range().end_line()]
                            .to_vec()
                            .join("\n"),
                        fs_file_path.to_owned(),
                        document_symbol.identifier_range(),
                        language.to_owned(),
                    );
                    Some(vec![OutlineNode::new(
                        function_node,
                        vec![],
                        language.to_owned(),
                    )])
                }
                SymbolKind::Object => {
                    let class_node = OutlineNodeContent::class_implementation_symbol(
                        name_from_selection_range(
                            file_lines.as_slice(),
                            document_symbol.selection_range.clone(),
                        ),
                        document_symbol.range(),
                        file_lines[document_symbol.range().start_line()
                            ..=document_symbol.range().end_line()]
                            .to_vec()
                            .join("\n"),
                        fs_file_path.to_owned(),
                        document_symbol.identifier_range(),
                        language.to_owned(),
                    );
                    let children = document_symbols_to_outline_nodes(
                        language.to_owned(),
                        fs_file_path.to_owned(),
                        file_content,
                        document_symbol.children,
                    );
                    Some(vec![OutlineNode::new(
                        class_node,
                        children
                            .into_iter()
                            .map(|child| child.consume_content())
                            .collect::<Vec<_>>(),
                        language.to_owned(),
                    )])
                }
                SymbolKind::Module => Some(document_symbols_to_outline_nodes(
                    language.to_owned(),
                    fs_file_path.to_owned(),
                    file_content,
                    document_symbol.children,
                )),
                // If we don't support the symbolkind, then just make it a class
                _ => {
                    let class_node = OutlineNodeContent::class_implementation_symbol(
                        name_from_selection_range(
                            file_lines.as_slice(),
                            document_symbol.selection_range.clone(),
                        ),
                        document_symbol.range(),
                        file_lines[document_symbol.range().start_line()
                            ..=document_symbol.range().end_line()]
                            .to_vec()
                            .join("\n"),
                        fs_file_path.to_owned(),
                        document_symbol.identifier_range(),
                        language.to_owned(),
                    );
                    Some(vec![OutlineNode::new(
                        class_node,
                        vec![],
                        language.to_owned(),
                    )])
                }
            }
        })
        .flatten()
        .collect::<Vec<_>>()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OutlineNodesUsingEditorResponse {
    file_content: String,
    // we have to create the outline nodes over here
    outline_nodes: Vec<DocumentSymbol>,
    language: String,
}

impl OutlineNodesUsingEditorResponse {
    pub fn to_outline_nodes(self, fs_file_path: String) -> Vec<OutlineNode> {
        let mut results = document_symbols_to_outline_nodes(
            self.language.to_owned(),
            fs_file_path.to_owned(),
            &self.file_content,
            self.outline_nodes,
        );
        // send over the file symbol here none the less if required, this can't
        // break anything which works on top of symbol level but it will work
        // wonders when we want to do file level edits
        let file_content = self.file_content;
        let cloned_file_path = fs_file_path.to_owned();
        let final_file_content = format!(
            r#"{cloned_file_path}
{file_content}"#
        );
        let file_content_lines = final_file_content
            .lines()
            .into_iter()
            .collect::<Vec<_>>()
            .len();
        let full_file_content = Range::new(
            Position::new(0, 0, 0),
            Position::new(file_content_lines, 0, 0),
        );
        let file_path_length = fs_file_path.chars().collect::<Vec<_>>().len();
        let identifier_range = Range::new(
            Position::new(0, 0, 0),
            Position::new(0, file_path_length, 0),
        );
        let body_range = Range::new(
            Position::new(1, 0, 0),
            // this is wrong btw we are taking a shortcut, we should calculate the length
            // of the last line in the file
            Position::new(file_content_lines - 1, 0, 0),
        );
        results.push(OutlineNode::new(
            OutlineNodeContent::new(
                fs_file_path.to_owned(),
                full_file_content,
                OutlineNodeType::File,
                final_file_content,
                fs_file_path,
                identifier_range,
                body_range,
                self.language.to_owned(),
                None,
            ),
            vec![],
            self.language,
        ));
        results
    }
}

impl OutlineNodesUsingEditorRequest {
    pub fn new(fs_file_path: String, editor_url: String) -> Self {
        Self {
            fs_file_path,
            editor_url,
        }
    }
}

pub struct OutlineNodesUsingEditorClient {
    client: reqwest_middleware::ClientWithMiddleware,
}

impl OutlineNodesUsingEditorClient {
    pub fn new() -> Self {
        Self {
            client: new_client(),
        }
    }
}

#[async_trait]
impl Tool for OutlineNodesUsingEditorClient {
    async fn invoke(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let context = input.should_outline_nodes_using_editor()?;
        let editor_endpoint = context.editor_url.to_owned() + "/get_outline_nodes";
        let response = self
            .client
            .post(editor_endpoint)
            .body(serde_json::to_string(&context).map_err(|_e| ToolError::SerdeConversionFailed)?)
            .send()
            .await
            .map_err(|_e| ToolError::ErrorCommunicatingWithEditor)?;
        let response: OutlineNodesUsingEditorResponse = response.json().await.map_err(|e| {
            eprintln!("{:?}", e);
            ToolError::SerdeConversionFailed
        })?;

        Ok(ToolOutput::outline_nodes_using_editor(response))
    }

    fn tool_description(&self) -> String {
        "".to_owned()
    }

    fn tool_input_format(&self) -> String {
        "".to_owned()
    }

    fn get_evaluation_criteria(&self, _trajectory_length: usize) -> Vec<String> {
        vec![]
    }

    fn get_reward_scale(&self, _trajectory_length: usize) -> Vec<ToolRewardScale> {
        vec![]
    }
}

#[cfg(test)]
mod tests {

    use super::{document_symbols_to_outline_nodes, OutlineNodesUsingEditorResponse};

    #[test]
    fn test_parsing_response_from_editor() {
        let response = r#"{
  "outline_nodes": [
    {
      "name": "main",
      "detail": "fn()",
      "kind": 11,
      "range": [
        {
          "line": 4,
          "character": 0
        },
        {
          "line": 27,
          "character": 1
        }
      ],
      "selectionRange": [
        {
          "line": 5,
          "character": 9
        },
        {
          "line": 5,
          "character": 13
        }
      ],
      "children": []
    }
  ],
  "file_content": "testing"
}"#;
        let parsed_response = serde_json::from_str::<OutlineNodesUsingEditorResponse>(response);
        println!("{:?}", &parsed_response);
        assert!(parsed_response.is_ok());
    }

    #[test]
    fn test_outline_node_generation() {
        // peak clowntown 🤡
        let outline_nodes_from_editor = r#"{
  "file_content": "fn something() {\n}\n\nstruct Something {\n\n}\n\nimpl Something {\n\n}\n\nimpl Blah for Something {\n\n}\n\nenum Something {\n\n}\n\nmod something {\n    mod somethingElse {\n        fn something_inner() {\n            \n        }\n    }\n}",
  "outline_nodes": [
    {
      "name": "something",
      "detail": "fn()",
      "kind": 11,
      "range": [
        {
          "line": 0,
          "character": 0
        },
        {
          "line": 1,
          "character": 1
        }
      ],
      "selectionRange": [
        {
          "line": 0,
          "character": 3
        },
        {
          "line": 0,
          "character": 12
        }
      ],
      "children": []
    },
    {
      "name": "Something",
      "detail": "",
      "kind": 22,
      "range": [
        {
          "line": 3,
          "character": 0
        },
        {
          "line": 5,
          "character": 1
        }
      ],
      "selectionRange": [
        {
          "line": 3,
          "character": 7
        },
        {
          "line": 3,
          "character": 16
        }
      ],
      "children": []
    },
    {
      "name": "impl Something",
      "detail": "",
      "kind": 18,
      "range": [
        {
          "line": 7,
          "character": 0
        },
        {
          "line": 9,
          "character": 1
        }
      ],
      "selectionRange": [
        {
          "line": 7,
          "character": 5
        },
        {
          "line": 7,
          "character": 14
        }
      ],
      "children": []
    },
    {
      "name": "impl Blah for Something",
      "detail": "",
      "kind": 18,
      "range": [
        {
          "line": 11,
          "character": 0
        },
        {
          "line": 13,
          "character": 1
        }
      ],
      "selectionRange": [
        {
          "line": 11,
          "character": 14
        },
        {
          "line": 11,
          "character": 23
        }
      ],
      "children": []
    },
    {
      "name": "Something",
      "detail": "",
      "kind": 9,
      "range": [
        {
          "line": 15,
          "character": 0
        },
        {
          "line": 17,
          "character": 1
        }
      ],
      "selectionRange": [
        {
          "line": 15,
          "character": 5
        },
        {
          "line": 15,
          "character": 14
        }
      ],
      "children": []
    },
    {
      "name": "something",
      "detail": "",
      "kind": 1,
      "range": [
        {
          "line": 19,
          "character": 0
        },
        {
          "line": 25,
          "character": 1
        }
      ],
      "selectionRange": [
        {
          "line": 19,
          "character": 4
        },
        {
          "line": 19,
          "character": 13
        }
      ],
      "children": [
        {
          "name": "somethingElse",
          "detail": "",
          "kind": 1,
          "range": [
            {
              "line": 20,
              "character": 4
            },
            {
              "line": 24,
              "character": 5
            }
          ],
          "selectionRange": [
            {
              "line": 20,
              "character": 8
            },
            {
              "line": 20,
              "character": 21
            }
          ],
          "children": [
            {
              "name": "something_inner",
              "detail": "fn()",
              "kind": 11,
              "range": [
                {
                  "line": 21,
                  "character": 8
                },
                {
                  "line": 23,
                  "character": 9
                }
              ],
              "selectionRange": [
                {
                  "line": 21,
                  "character": 11
                },
                {
                  "line": 21,
                  "character": 26
                }
              ],
              "children": []
            }
          ]
        }
      ]
    }
  ],
  "language": "rust"
}"#;
        let file_content = (0..=700)
            .into_iter()
            .map(|_| "a".to_owned())
            .collect::<Vec<_>>()
            .join("\n");
        let file_lines = file_content
            .lines()
            .into_iter()
            .map(|s| s.to_owned())
            .collect::<Vec<_>>()
            .join("\n");
        let document_symbols =
            serde_json::from_str::<OutlineNodesUsingEditorResponse>(&outline_nodes_from_editor)
                .expect("to work");
        let outline_nodes = document_symbols_to_outline_nodes(
            "rust".to_owned(),
            "something/testing.rs".to_owned(),
            &file_lines,
            document_symbols.outline_nodes,
        );
        assert!(!outline_nodes.is_empty());
        assert_eq!(outline_nodes.len(), 6);
    }
}
