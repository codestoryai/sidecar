//! Contains all the different kind of events which we get while getting a context
//! recording from the user
//! This helps the user interact with the editor in a very natural way and for the agent
//! to understand the different steps the user has taken to get a task done

use std::collections::HashSet;

use crate::chunking::{
    text_document::{Position, Range},
    types::OutlineNode,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenFileContextEvent {
    fs_file_path: String,
}

impl OpenFileContextEvent {
    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct LSPContextEventDestination {
    fs_file_path: String,
    position: Position,
    line_content: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LSPContextEvent {
    fs_file_path: String,
    position: Position,
    source_word: Option<String>,
    source_line: String,
    event_type: String,
    destination: Option<LSPContextEventDestination>,
}

impl LSPContextEvent {
    pub fn source_fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    /// The source position where the lsp event happened
    pub fn source_position(&self) -> &Position {
        &self.position
    }

    /// The destination position on the file where we land
    pub fn destination_position(&self) -> Option<Position> {
        self.destination
            .as_ref()
            .map(|destination| destination.position.clone())
    }

    pub fn destination_maybe(&self) -> Option<String> {
        self.destination
            .as_ref()
            .map(|destination| destination.fs_file_path.to_owned())
    }

    /// Converst the lsp context event to a prompt
    pub fn lsp_context_event_to_prompt(
        &self,
        source_outline_nodes: Vec<OutlineNode>,
        destination_outline_nodes: Vec<OutlineNode>,
    ) -> Option<String> {
        println!("tool_box::lsp_context_event_to_prompt");
        // source outline node can be blank if we are clicking on a position which
        // is part of the imports
        let source_outline_node = source_outline_nodes
            .into_iter()
            .filter(|outline_node| !outline_node.is_file())
            .find(|outline_node| outline_node.range().contains_line(self.position.line()));
        let destination_outline_node = match self.destination.clone() {
            Some(destination) => destination_outline_nodes
                .into_iter()
                .filter(|outline_node| !outline_node.is_file())
                .find(|outline_node| {
                    outline_node
                        .range()
                        .contains_line(destination.position.line())
                }),
            None => None,
        };

        let source_word = self.source_word.to_owned().unwrap_or_default();
        // if we are unable to grab the outline node we should grab the line at the
        // very least
        let source_prompt = match source_outline_node {
            Some(source_outline_node) => {
                let file_path = source_outline_node.fs_file_path();
                let start_line = source_outline_node.range().start_line();
                let end_line = source_outline_node.range().end_line();
                let content = source_outline_node.content().content();
                let language_id = source_outline_node.content().language();
                format!(
                    r#"FILEPATH: {file_path}-{start_line}:{end_line}
```{language_id}
{content}
```"#
                )
            }
            None => {
                let file_path = &self.fs_file_path;
                let source_line = &self.source_line;
                let start_line = self.position.line();
                format!(
                    r#"FILEPATH: {file_path}-{start_line}:{start_line}
```
{source_line}
```"#
                )
            }
        };
        let destination_prompt = match destination_outline_node {
            Some(destination_outline_node) => {
                let file_path = destination_outline_node.fs_file_path();
                let start_line = destination_outline_node.range().start_line();
                let end_line = destination_outline_node.range().end_line();
                let content = destination_outline_node.content().content();
                let language_id = destination_outline_node.content().language();
                Some(format!(
                    r#"I ended up here
FILEPATH: {file_path}-{start_line}:{end_line}
```{language_id}
{content}
```"#
                ))
            }
            None => match self.destination.as_ref() {
                Some(destination) => {
                    let file_path = &destination.fs_file_path;
                    let start_line = destination.position.line();
                    let line_content = &destination.line_content;
                    Some(format!(
                        r#"I ended up here
FILEPATH: {file_path}-{start_line}:{start_line}
```
{line_content}
```"#
                    ))
                }
                None => None,
            },
        };
        let event = self.event_type.to_owned();

        // now we have the source outline node and the source word where the user clicked
        // should be easy to create a prompt out of this
        let lsp_start_prompt = format!(
            r#"I performed {event} on {source_word} located in
{source_prompt}"#
        );
        let final_prompt = match destination_prompt {
            Some(destination_prompt) => {
                format!(
                    r#"{lsp_start_prompt}
{destination_prompt}"#
                )
            }
            None => lsp_start_prompt,
        };
        Some(final_prompt)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SelectionContextEvent {
    fs_file_path: String,
    range: Range,
}

impl SelectionContextEvent {
    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn selection_range(&self) -> &Range {
        &self.range
    }

    /// Converts subsequent file open response to a single prompt
    pub fn to_prompt(selection_events: Vec<Self>, outline_nodes: Vec<OutlineNode>) -> String {
        let line_numbers_interested_in = selection_events
            .iter()
            .map(|selection_event| {
                (selection_event.range.start_line()..=selection_event.range.end_line())
                    .collect::<Vec<_>>()
            })
            .flatten()
            .collect::<HashSet<_>>();

        // now we want to for sure include all the outline nodes which are interesecting with these lines
        // and for lines which are not included in any outline nodes, we ignore them for now
        let interested_outline_nodes = outline_nodes
            .into_iter()
            .filter(|outline_node| {
                let outline_node_range = outline_node.range();
                line_numbers_interested_in
                    .iter()
                    .any(|line_number| outline_node_range.contains_line(*line_number))
            })
            .collect::<Vec<_>>();

        // Now we create a prompt out of this
        let outline_nodes_prompt = interested_outline_nodes
            .into_iter()
            .map(|outline_node| {
                let file_path = outline_node.fs_file_path();
                let start_line = outline_node.range().start_line();
                let end_line = outline_node.range().end_line();
                let content = outline_node.content().content();
                let language_id = outline_node.content().language();
                format!(
                    r#"FILEPATH: {file_path}-{start_line}:{end_line}
```{language_id}
{content}
```"#
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "I am highlighting this section for you:
{outline_nodes_prompt}"
        )
        // Figure out what we can show to the user
    }
}

/// All the context-driven events which can happen in the editor that are useful
/// and done by the user in a quest to provide additional context to the agent.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ContextGatheringEvent {
    OpenFile(OpenFileContextEvent),
    LSPContextEvent(LSPContextEvent),
    Selection(SelectionContextEvent),
}

impl ContextGatheringEvent {
    pub fn is_selection(&self) -> bool {
        matches!(self, Self::Selection(_))
    }
}
