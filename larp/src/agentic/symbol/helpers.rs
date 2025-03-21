use std::collections::HashMap;

use crate::{
    agentic::tool::{
        code_symbol::models::anthropic::AskQuestionSymbolHint,
        lsp::inlay_hints::{InlayHintsResponse, InlayHintsResponseParts},
    },
    chunking::text_document::Range,
};

use super::{
    events::edit::SymbolToEdit,
    identifier::{Snippet, SymbolIdentifier},
};

/// Keeps track of the symbols on which we want to perform followups
#[derive(Clone)]
pub struct SymbolFollowupBFS {
    symbol_edited: SymbolToEdit,
    // this is the parent symbol where the edit was happening
    // we could be editing a sub-symbol inside the parent
    symbol_identifier: SymbolIdentifier,
    original_code: String,
    edited_code: String,
}

impl SymbolFollowupBFS {
    pub fn new(
        symbol_edited: SymbolToEdit,
        symbol_identifier: SymbolIdentifier,
        original_code: String,
        edited_code: String,
    ) -> Self {
        Self {
            symbol_edited,
            symbol_identifier,
            original_code,
            edited_code,
        }
    }

    pub fn symbol_identifier(&self) -> &SymbolIdentifier {
        &self.symbol_identifier
    }

    pub fn symbol_edited(&self) -> &SymbolToEdit {
        &self.symbol_edited
    }

    pub fn original_code(&self) -> &str {
        &self.original_code
    }

    pub fn edited_code(&self) -> &str {
        &self.edited_code
    }
}

/// Grab the file contents above, below and in the selection
pub fn split_file_content_into_parts(
    file_content: &str,
    selection_range: &Range,
) -> (Option<String>, Option<String>, String) {
    let lines = file_content
        .lines()
        .enumerate()
        .into_iter()
        .map(|(idx, line)| (idx as i64, line.to_owned()))
        .collect::<Vec<_>>();

    let start_line = selection_range.start_line() as i64;
    let end_line = selection_range.end_line() as i64;
    let above: Option<String>;
    if start_line == 0 {
        above = None;
    } else {
        let above_lines = lines
            .iter()
            .take_while(|(idx, _line)| idx < &start_line)
            .map(|(_, line)| line.to_owned())
            .collect::<Vec<_>>()
            .join("\n");
        above = Some(above_lines.to_owned());
    }

    // now we generate the section in the selection
    let selection_range = lines
        .iter()
        .skip_while(|(idx, _)| idx < &start_line)
        .take_while(|(idx, _)| idx <= &end_line)
        .map(|(_, line)| line.to_owned())
        .collect::<Vec<_>>()
        .join("\n");

    let below: Option<String>;
    if end_line >= lines.len() as i64 {
        below = None;
    } else {
        let below_lines = lines
            .iter()
            .skip_while(|(idx, _)| idx <= &end_line)
            .map(|(_, line)| line.to_owned())
            .collect::<Vec<_>>()
            .join("\n");
        below = Some(below_lines)
    }

    (above, below, selection_range)
}

fn search_haystack<T: PartialEq>(needle: &[T], haystack: &[T]) -> Option<usize> {
    if needle.is_empty() {
        // special case: `haystack.windows(0)` will panic, so this case
        // needs to be handled separately in whatever way you feel is
        // appropriate
        return Some(0);
    }

    haystack
        .windows(needle.len())
        .rposition(|subslice| subslice == needle)
}

/// Find the symbol in the line now
/// our home fed needle in haystack which works on character level instead
/// of byte level
/// This returns the last character position where the needle is contained in
/// the haystack
pub fn find_needle_position(haystack: &str, needle: &str) -> Option<usize> {
    search_haystack(
        needle.chars().into_iter().collect::<Vec<_>>().as_slice(),
        haystack.chars().into_iter().collect::<Vec<_>>().as_slice(),
    )
}

/// Generates a hyperlink which contains information of where we found the hit
/// for the question we want to ask
pub fn generate_hyperlink_from_snippet(
    snippet: &Snippet,
    ask_question: AskQuestionSymbolHint,
) -> String {
    let thinking = ask_question.thinking();
    let file_path = ask_question.file_path();
    let snippet_start_line = snippet.range().start_line();
    let snippet_lines = snippet
        .content()
        .lines()
        .enumerate()
        .map(|(idx, line)| (idx + snippet_start_line, line.to_owned()))
        .collect::<Vec<_>>();
    let possible_line_match = snippet_lines
        .iter()
        .find(|(_idx, line)| line == ask_question.line_content())
        .map(|(idx, _)| idx.clone());
    let hyperlink = snippet_lines
        .into_iter()
        .map(|(idx, line)| {
            if Some(&idx) == possible_line_match.as_ref() {
                format!(
                    r#"<line_with_reference>
{line}
</line_with_reference>"#
                )
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"<hyperlink>
<thinking>
{thinking}
</thinking>
<code_snippet>
<file_path>
{file_path}
</file_path>
{hyperlink}
</code_snippet>
</hyperlink>"#
    )
}

/// Applies the inlay hints to the code which is already present
pub fn apply_inlay_hints_to_code(
    code: &str,
    code_range: &Range,
    inlay_hints: InlayHintsResponse,
) -> String {
    let range_start_line = code_range.start_line();
    let code_lines = code
        .lines()
        .enumerate()
        .into_iter()
        .map(|(idx, line)| (idx + range_start_line, line.to_owned()))
        .collect::<Vec<_>>();

    let mut inlay_hints_per_line: HashMap<usize, Vec<InlayHintsResponseParts>> = HashMap::new();
    inlay_hints.parts().into_iter().for_each(|inlay_hint| {
        let line_number = inlay_hint.position().line();
        if let Some(hints) = inlay_hints_per_line.get_mut(&line_number) {
            hints.push(inlay_hint);
        } else {
            inlay_hints_per_line.insert(line_number, vec![inlay_hint]);
        }
    });

    // now we go over each line of the code and then get the inlay-hint at that
    // line and insert it into our code
    let code_with_inlay_hints = code_lines
        .into_iter()
        .map(|(line_number, content)| {
            if let Some(mut inlay_hints) = inlay_hints_per_line.remove(&line_number) {
                // figure out how to set the inlay hints here properly
                inlay_hints.sort_by_key(|inlay_hint| inlay_hint.position().column());
                // padding right and padding left here determines the space and where we want to give it
                let content_by_characters = content
                    .chars()
                    .into_iter()
                    .enumerate()
                    .map(|(idx, char)| (idx, char.to_string()))
                    .map(|(idx, char)| {
                        let inlay_hint_at_char = inlay_hints
                            .iter()
                            .filter(|inlay_hint| inlay_hint.position().column() == idx)
                            .map(|inlay_hint| {
                                let mut value = inlay_hint.values().join("");
                                if inlay_hint.padding_left() {
                                    value = " ".to_owned() + &value;
                                } else if inlay_hint.padding_right() {
                                    value = value + " ";
                                }
                                value
                            })
                            .collect::<Vec<_>>()
                            .join("");
                        inlay_hint_at_char + &char.to_owned()
                    })
                    .collect::<Vec<_>>()
                    .join("");
                content_by_characters
            } else {
                content.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    code_with_inlay_hints
}

#[cfg(test)]
mod tests {

    use crate::{
        agentic::tool::lsp::inlay_hints::InlayHintsResponse,
        chunking::text_document::{Position, Range},
    };

    use super::apply_inlay_hints_to_code;

    #[test]
    fn test_applying_inlay_hints_works() {
        let inlay_hints_response = r#"
{
  "parts": [
    {
      "position": {
        "line": 308,
        "character": 17,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "Application",
        ""
      ]
    },
    {
      "position": {
        "line": 310,
        "character": 18,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "String",
        ""
      ]
    },
    {
      "position": {
        "line": 311,
        "character": 18,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "String",
        ""
      ]
    },
    {
      "position": {
        "line": 312,
        "character": 18,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "String",
        ""
      ]
    },
    {
      "position": {
        "line": 313,
        "character": 24,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "UserContext",
        ""
      ]
    },
    {
      "position": {
        "line": 314,
        "character": 26,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "Option",
        "<",
        "ProbeRequestActiveWindow",
        ">"
      ]
    },
    {
      "position": {
        "line": 315,
        "character": 22,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "String",
        ""
      ]
    },
    {
      "position": {
        "line": 316,
        "character": 23,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": bool"
      ]
    },
    {
      "position": {
        "line": 440,
        "character": 1,
        "byteOffset": 0
      },
      "padding_left": true,
      "padding_right": false,
      "values": [
        "fn code_editing"
      ]
    },
    {
      "position": {
        "line": 320,
        "character": 15,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "UnboundedSender",
        "<",
        "UIEventWithID",
        ">"
      ]
    },
    {
      "position": {
        "line": 320,
        "character": 25,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "UnboundedReceiver",
        "<",
        "UIEventWithID",
        ">"
      ]
    },
    {
      "position": {
        "line": 321,
        "character": 28,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "Arc",
        "<",
        "ProbeRequestTracker",
        ">"
      ]
    },
    {
      "position": {
        "line": 322,
        "character": 19,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "Arc",
        "<",
        "ToolBroker",
        ">"
      ]
    },
    {
      "position": {
        "line": 322,
        "character": 31,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "data:"
      ]
    },
    {
      "position": {
        "line": 323,
        "character": 8,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "llm_client:"
      ]
    },
    {
      "position": {
        "line": 324,
        "character": 8,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "code_edit_broker:"
      ]
    },
    {
      "position": {
        "line": 325,
        "character": 8,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "symbol_tracking:"
      ]
    },
    {
      "position": {
        "line": 326,
        "character": 8,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "language_broker:"
      ]
    },
    {
      "position": {
        "line": 328,
        "character": 8,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "tool_broker_config:"
      ]
    },
    {
      "position": {
        "line": 329,
        "character": 8,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "fail_over_llm:"
      ]
    },
    {
      "position": {
        "line": 324,
        "character": 17,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "data:"
      ]
    },
    {
      "position": {
        "line": 328,
        "character": 37,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "editor_agent:"
      ]
    },
    {
      "position": {
        "line": 328,
        "character": 43,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "apply_edits_directly:"
      ]
    },
    {
      "position": {
        "line": 330,
        "character": 12,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "llm:"
      ]
    },
    {
      "position": {
        "line": 331,
        "character": 12,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "provider:"
      ]
    },
    {
      "position": {
        "line": 332,
        "character": 12,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "api_keys:"
      ]
    },
    {
      "position": {
        "line": 333,
        "character": 16,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "api_key:"
      ]
    },
    {
      "position": {
        "line": 343,
        "character": 34,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "ProbeRequestActiveWindow",
        ""
      ]
    },
    {
      "position": {
        "line": 351,
        "character": 20,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "String",
        ""
      ]
    },
    {
      "position": {
        "line": 352,
        "character": 18,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "Vec",
        "<u8>"
      ]
    },
    {
      "position": {
        "line": 354,
        "character": 16,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "msg:"
      ]
    },
    {
      "position": {
        "line": 353,
        "character": 14,
        "byteOffset": 0
      },
      "padding_left": true,
      "padding_right": false,
      "values": [
        "",
        "Result",
        "<",
        "Vec",
        "<u8>, ",
        "Error",
        ">"
      ]
    },
    {
      "position": {
        "line": 352,
        "character": 61,
        "byteOffset": 0
      },
      "padding_left": true,
      "padding_right": false,
      "values": [
        "impl ",
        "Future",
        "<",
        "Output",
        " = ",
        "Result",
        "<…, …>>",
        ""
      ]
    },
    {
      "position": {
        "line": 352,
        "character": 37,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "path:"
      ]
    },
    {
      "position": {
        "line": 355,
        "character": 20,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "String",
        ""
      ]
    },
    {
      "position": {
        "line": 355,
        "character": 60,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "msg:"
      ]
    },
    {
      "position": {
        "line": 355,
        "character": 41,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "vec:"
      ]
    },
    {
      "position": {
        "line": 357,
        "character": 73,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "language:"
      ]
    },
    {
      "position": {
        "line": 359,
        "character": 29,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "LLMProperties",
        ""
      ]
    },
    {
      "position": {
        "line": 360,
        "character": 8,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "llm:"
      ]
    },
    {
      "position": {
        "line": 361,
        "character": 8,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "provider:"
      ]
    },
    {
      "position": {
        "line": 362,
        "character": 8,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "api_keys:"
      ]
    },
    {
      "position": {
        "line": 363,
        "character": 12,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "api_key:"
      ]
    },
    {
      "position": {
        "line": 367,
        "character": 13,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "LLMType",
        ""
      ]
    },
    {
      "position": {
        "line": 368,
        "character": 21,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "LLMProvider",
        ""
      ]
    },
    {
      "position": {
        "line": 369,
        "character": 26,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "LLMProviderAPIKeys",
        ""
      ]
    },
    {
      "position": {
        "line": 369,
        "character": 80,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "api_key:"
      ]
    },
    {
      "position": {
        "line": 370,
        "character": 22,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "SymbolManager",
        ""
      ]
    },
    {
      "position": {
        "line": 371,
        "character": 8,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "tools:"
      ]
    },
    {
      "position": {
        "line": 372,
        "character": 8,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "symbol_broker:"
      ]
    },
    {
      "position": {
        "line": 373,
        "character": 8,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "editor_parsing:"
      ]
    },
    {
      "position": {
        "line": 374,
        "character": 8,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "editor_url:"
      ]
    },
    {
      "position": {
        "line": 375,
        "character": 8,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "ui_sender:"
      ]
    },
    {
      "position": {
        "line": 376,
        "character": 8,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "llm_properties:"
      ]
    },
    {
      "position": {
        "line": 382,
        "character": 8,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "request_id:"
      ]
    },
    {
      "position": {
        "line": 377,
        "character": 12,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "llm:"
      ]
    },
    {
      "position": {
        "line": 387,
        "character": 23,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "String",
        ""
      ]
    },
    {
      "position": {
        "line": 390,
        "character": 19,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "JoinHandle",
        "<()>"
      ]
    },
    {
      "position": {
        "line": 390,
        "character": 35,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "future:"
      ]
    },
    {
      "position": {
        "line": 413,
        "character": 14,
        "byteOffset": 0
      },
      "padding_left": true,
      "padding_right": false,
      "values": [
        "impl ",
        "Future",
        "<",
        "Output",
        " = ",
        "Result",
        "<…, …>>",
        ""
      ]
    },
    {
      "position": {
        "line": 392,
        "character": 29,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "input_event:"
      ]
    },
    {
      "position": {
        "line": 391,
        "character": 30,
        "byteOffset": 0
      },
      "padding_left": true,
      "padding_right": false,
      "values": [
        "",
        "SymbolManager",
        ""
      ]
    },
    {
      "position": {
        "line": 394,
        "character": 16,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "llm:"
      ]
    },
    {
      "position": {
        "line": 399,
        "character": 16,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "swe_bench_test_endpoint:"
      ]
    },
    {
      "position": {
        "line": 400,
        "character": 16,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "repo_map_fs_path:"
      ]
    },
    {
      "position": {
        "line": 401,
        "character": 16,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "gcloud_access_token:"
      ]
    },
    {
      "position": {
        "line": 402,
        "character": 16,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "swe_bench_id:"
      ]
    },
    {
      "position": {
        "line": 403,
        "character": 16,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "swe_bench_git_dname:"
      ]
    },
    {
      "position": {
        "line": 404,
        "character": 16,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "swe_bench_code_editing:"
      ]
    },
    {
      "position": {
        "line": 405,
        "character": 16,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "swe_bench_gemini_api_keys:"
      ]
    },
    {
      "position": {
        "line": 406,
        "character": 16,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "swe_bench_long_context_editing:"
      ]
    },
    {
      "position": {
        "line": 407,
        "character": 16,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "full_symbol_edit:"
      ]
    },
    {
      "position": {
        "line": 409,
        "character": 16,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "root_directory:"
      ]
    },
    {
      "position": {
        "line": 410,
        "character": 16,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "fast_code_symbol_search_llm:"
      ]
    },
    {
      "position": {
        "line": 411,
        "character": 16,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "file_important_search:"
      ]
    },
    {
      "position": {
        "line": 412,
        "character": 16,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "big_search:"
      ]
    },
    {
      "position": {
        "line": 417,
        "character": 52,
        "byteOffset": 0
      },
      "padding_left": true,
      "padding_right": false,
      "values": [
        "impl ",
        "Future",
        "<",
        "Output",
        " = ()>",
        ""
      ]
    },
    {
      "position": {
        "line": 416,
        "character": 32,
        "byteOffset": 0
      },
      "padding_left": true,
      "padding_right": false,
      "values": [
        "",
        "Arc",
        "<",
        "ProbeRequestTracker",
        ">"
      ]
    },
    {
      "position": {
        "line": 420,
        "character": 20,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "Sse",
        "<",
        "Map",
        "<",
        "UnboundedReceiverStream",
        "<…>, …>>"
      ]
    },
    {
      "position": {
        "line": 421,
        "character": 8,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "stream:"
      ]
    },
    {
      "position": {
        "line": 421,
        "character": 61,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "recv:"
      ]
    },
    {
      "position": {
        "line": 421,
        "character": 81,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": false,
      "values": [
        ": ",
        "UIEventWithID",
        ""
      ]
    },
    {
      "position": {
        "line": 424,
        "character": 25,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "op:"
      ]
    },
    {
      "position": {
        "line": 423,
        "character": 33,
        "byteOffset": 0
      },
      "padding_left": true,
      "padding_right": false,
      "values": [
        "",
        "Result",
        "<",
        "Event",
        ", ",
        "Error",
        ">"
      ]
    },
    {
      "position": {
        "line": 422,
        "character": 33,
        "byteOffset": 0
      },
      "padding_left": true,
      "padding_right": false,
      "values": [
        "",
        "Event",
        ""
      ]
    },
    {
      "position": {
        "line": 431,
        "character": 45,
        "byteOffset": 0
      },
      "padding_left": true,
      "padding_right": false,
      "values": [
        "",
        "KeepAlive",
        ""
      ]
    },
    {
      "position": {
        "line": 431,
        "character": 22,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "time:"
      ]
    },
    {
      "position": {
        "line": 430,
        "character": 29,
        "byteOffset": 0
      },
      "padding_left": true,
      "padding_right": false,
      "values": [
        "",
        "KeepAlive",
        ""
      ]
    },
    {
      "position": {
        "line": 437,
        "character": 28,
        "byteOffset": 0
      },
      "padding_left": false,
      "padding_right": true,
      "values": [
        "msg:"
      ]
    },
    {
      "position": {
        "line": 436,
        "character": 23,
        "byteOffset": 0
      },
      "padding_left": true,
      "padding_right": false,
      "values": [
        "",
        "Result",
        "<",
        "Event",
        ", ",
        "Error",
        ">"
      ]
    },
    {
      "position": {
        "line": 433,
        "character": 37,
        "byteOffset": 0
      },
      "padding_left": true,
      "padding_right": false,
      "values": [
        "",
        "Event",
        ""
      ]
    }
  ]
}"#;
        let response =
            serde_json::from_str::<InlayHintsResponse>(inlay_hints_response).expect("to work");
        let code_to_check = r#"pub async fn code_editing(
    Extension(app): Extension<Application>,
    Json(AgenticCodeEditing {
        user_query,
        editor_url,
        request_id,
        mut user_context,
        active_window_data,
        root_directory,
        codebase_search,
    }): Json<AgenticCodeEditing>,
) -> Result<impl IntoResponse> {
    println!("webserver::code_editing_start");
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let edit_request_tracker = app.probe_request_tracker.clone();
    let tool_broker = Arc::new(ToolBroker::new(
        app.llm_broker.clone(),
        Arc::new(CodeEditBroker::new()),
        app.symbol_tracker.clone(),
        app.language_parsing.clone(),
        // do not apply the edits directly
        ToolBrokerConfiguration::new(None, false),
        LLMProperties::new(
            LLMType::Gpt4O,
            LLMProvider::OpenAI,
            LLMProviderAPIKeys::OpenAI(OpenAIProvider::new(
                "".to_owned(),
            )),
        ), // LLMProperties::new(
           //     LLMType::GeminiPro,
           //     LLMProvider::GoogleAIStudio,
           //     LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new(
           //         "".to_owned(),
           //     )),
           // ),
    ));
    if let Some(active_window_data) = active_window_data {
        user_context = user_context.update_file_content_map(
            active_window_data.file_path,
            active_window_data.file_content,
            active_window_data.language,
        );
    }

    let fs_file_path = "/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs".to_owned();
    let file_bytes = tokio::fs::read(fs_file_path.to_owned())
        .await
        .expect("to work");
    let file_content = String::from_utf8(file_bytes).expect("to work");
    user_context =
        user_context.update_file_content_map(fs_file_path, file_content, "rust".to_owned());

    let _llama_70b_properties = LLMProperties::new(
        LLMType::Llama3_1_70bInstruct,
        LLMProvider::FireworksAI,
        LLMProviderAPIKeys::FireworksAI(FireworksAPIKey::new(
            "s8Y7yIXdL0lMeHHgvbZXS77oGtBAHAsfsLviL2AKnzuGpg1n".to_owned(),
        )),
    );

    let model = LLMType::ClaudeSonnet;
    let provider_type = LLMProvider::Anthropic;
    let anthropic_api_keys = LLMProviderAPIKeys::Anthropic(AnthropicAPIKey::new("".to_owned()));
    let symbol_manager = SymbolManager::new(
        tool_broker,
        app.symbol_tracker.clone(),
        app.editor_parsing.clone(),
        editor_url.to_owned(),
        sender,
        LLMProperties::new(
            model.clone(),
            provider_type.clone(),
            anthropic_api_keys.clone(),
        ),
        user_context.clone(),
        request_id.to_owned(),
    );

    println!("webserver::code_editing_flow::endpoint_hit");

    let edit_request_id = request_id.clone(); // Clone request_id before creating the closure
                                              // Now we send the original request over here and then await on the sender like
                                              // before
    let join_handle = tokio::spawn(async move {
        let _ = symbol_manager
            .initial_request(SymbolInputEvent::new(
                user_context,
                model,
                provider_type,
                anthropic_api_keys,
                user_query,
                edit_request_id,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                true,
                codebase_search,
                Some(root_directory),
                None,
                false,
                false,
            ))
            .await;
    });
    let _ = edit_request_tracker
        .track_new_request(&request_id, join_handle)
        .await;

    let event_stream = Sse::new(
        tokio_stream::wrappers::UnboundedReceiverStream::new(receiver).map(|event| {
            sse::Event::default()
                .json_data(event)
                .map_err(anyhow::Error::new)
        }),
    );

    // return the stream as a SSE event stream over here
    Ok(event_stream.keep_alive(
        sse::KeepAlive::new()
            .interval(Duration::from_secs(3))
            .event(
                sse::Event::default()
                    .json_data(json!({
                        "keep_alive": "alive"
                    }))
                    .expect("json to not fail in keep alive"),
            ),
    ))
}"#;
        let interested_range = Range::new(Position::new(307, 0, 0), Position::new(400, 0, 0));
        let inlay_hint_code =
            apply_inlay_hints_to_code(&code_to_check, &interested_range, response);
        let expected_code = r#"pub async fn code_editing(
    Extension(app: Application): Extension<Application>,
    Json(AgenticCodeEditing {
        user_query: String,
        editor_url: String,
        request_id: String,
        mut user_context: UserContext,
        active_window_data: Option<ProbeRequestActiveWindow>,
        root_directory: String,
        codebase_search: bool,
    }): Json<AgenticCodeEditing>,
) -> Result<impl IntoResponse> {
    println!("webserver::code_editing_start");
    let (sender: UnboundedSender<UIEventWithID>, receiver: UnboundedReceiver<UIEventWithID>) = tokio::sync::mpsc::unbounded_channel();
    let edit_request_tracker: Arc<ProbeRequestTracker> = app.probe_request_tracker.clone();
    let tool_broker: Arc<ToolBroker> = Arc::new(data: ToolBroker::new(
        llm_client: app.llm_broker.clone(),
        code_edit_broker: Arc::new(data: CodeEditBroker::new()),
        symbol_tracking: app.symbol_tracker.clone(),
        language_broker: app.language_parsing.clone(),
        // do not apply the edits directly
        tool_broker_config: ToolBrokerConfiguration::new(editor_agent: None, apply_edits_directly: false),
        fail_over_llm: LLMProperties::new(
            llm: LLMType::Gpt4O,
            provider: LLMProvider::OpenAI,
            api_keys: LLMProviderAPIKeys::OpenAI(OpenAIProvider::new(
                api_key: "".to_owned(),
            )),
        ), // LLMProperties::new(
           //     LLMType::GeminiPro,
           //     LLMProvider::GoogleAIStudio,
           //     LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new(
           //         "".to_owned(),
           //     )),
           // ),
    ));
    if let Some(active_window_data: ProbeRequestActiveWindow) = active_window_data {
        user_context = user_context.update_file_content_map(
            active_window_data.file_path,
            active_window_data.file_content,
            active_window_data.language,
        );
    }

    let fs_file_path: String = "/Users/skcd/test_repo/sidecar/sidecar/src/webserver/agentic.rs".to_owned();
    let file_bytes: Vec<u8> = tokio::fs::read(path: fs_file_path.to_owned())
        .await
        .expect(msg: "to work");
    let file_content: String = String::from_utf8(vec: file_bytes).expect(msg: "to work");
    user_context =
        user_context.update_file_content_map(fs_file_path, file_content, language: "rust".to_owned());

    let _llama_70b_properties: LLMProperties = LLMProperties::new(
        llm: LLMType::Llama3_1_70bInstruct,
        provider: LLMProvider::FireworksAI,
        api_keys: LLMProviderAPIKeys::FireworksAI(FireworksAPIKey::new(
            api_key: "s8Y7yIXdL0lMeHHgvbZXS77oGtBAHAsfsLviL2AKnzuGpg1n".to_owned(),
        )),
    );

    let model: LLMType = LLMType::ClaudeSonnet;
    let provider_type: LLMProvider = LLMProvider::Anthropic;
    let anthropic_api_keys: LLMProviderAPIKeys = LLMProviderAPIKeys::Anthropic(AnthropicAPIKey::new(api_key: "".to_owned()));
    let symbol_manager: SymbolManager = SymbolManager::new(
        tools: tool_broker,
        symbol_broker: app.symbol_tracker.clone(),
        editor_parsing: app.editor_parsing.clone(),
        editor_url: editor_url.to_owned(),
        ui_sender: sender,
        llm_properties: LLMProperties::new(
            llm: model.clone(),
            provider_type.clone(),
            anthropic_api_keys.clone(),
        ),
        user_context.clone(),
        request_id: request_id.to_owned(),
    );

    println!("webserver::code_editing_flow::endpoint_hit");

    let edit_request_id: String = request_id.clone(); // Clone request_id before creating the closure
                                              // Now we send the original request over here and then await on the sender like
                                              // before
    let join_handle: JoinHandle<()> = tokio::spawn(future: async move {
        let _ = symbol_manager
            .initial_request(input_event: SymbolInputEvent::new(
                user_context,
                llm: model,
                provider_type,
                anthropic_api_keys,
                user_query,
                edit_request_id,
                swe_bench_test_endpoint: None,
                repo_map_fs_path: None,
                gcloud_access_token: None,
                swe_bench_id: None,
                swe_bench_git_dname: None,
                swe_bench_code_editing: None,
                swe_bench_gemini_api_keys: None,
                swe_bench_long_context_editing: None,
                full_symbol_edit: true,
                codebase_search,
                root_directory: Some(root_directory),
                fast_code_symbol_search_llm: None,
                file_important_search: false,
                big_search: false,
            ))
            .await;
    });
    let _ = edit_request_tracker
        .track_new_request(&request_id, join_handle)
        .await;

    let event_stream: Sse<Map<UnboundedReceiverStream<…>, …>> = Sse::new(
        stream: tokio_stream::wrappers::UnboundedReceiverStream::new(recv: receiver).map(|event: UIEventWithID| {
            sse::Event::default()
                .json_data(event)
                .map_err(op: anyhow::Error::new)
        }),
    );

    // return the stream as a SSE event stream over here
    Ok(event_stream.keep_alive(
        sse::KeepAlive::new()
            .interval(time: Duration::from_secs(3))
            .event(
                sse::Event::default()
                    .json_data(json!({
                        "keep_alive": "alive"
                    }))
                    .expect(msg: "json to not fail in keep alive"),
            ),
    ))
}"#;
        assert_eq!(inlay_hint_code, expected_code);
    }
}
