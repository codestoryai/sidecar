use regex::Regex;

use crate::chunking::{
    text_document::{OutlineForRange, Position, Range},
    types::FunctionInformation,
};

use super::types::{ContextSelection, InLineAgentSelectionData};

#[derive(Debug)]
pub struct ContextWindowTracker {
    token_limit: usize,
    total_tokens: usize,
}

impl ContextWindowTracker {
    pub fn new(token_limit: usize) -> Self {
        Self {
            token_limit,
            total_tokens: 0,
        }
    }

    pub fn large_window() -> Self {
        Self::new(1_000_000_000)
    }

    pub fn add_tokens(&mut self, tokens: usize) {
        self.total_tokens += tokens;
    }

    pub fn tokens_remaining(&self) -> usize {
        self.token_limit - self.total_tokens
    }

    pub fn line_would_fit(&self, line: &str) -> bool {
        self.total_tokens + line.len() + 1 < self.token_limit
    }

    pub fn add_line(&mut self, line: &str) {
        self.total_tokens += line.len() + 1;
    }

    pub fn process_outlines(&mut self, generated_outline: OutlineForRange) -> OutlineForRange {
        // here we will process the outline again and try to generate it after making
        // sure that it fits in the limit
        let split_lines_regex = Regex::new(r"\r\n|\r|\n").unwrap();
        let lines_above: Vec<String> = split_lines_regex
            .split(&generated_outline.above())
            .map(|s| s.to_owned())
            .collect();
        let lines_below: Vec<String> = split_lines_regex
            .split(&generated_outline.below())
            .map(|s| s.to_owned())
            .collect();

        let mut processed_above = vec![];
        let mut processed_below = vec![];

        let mut try_add_above_line =
            |line: &str, context_manager: &mut ContextWindowTracker| -> bool {
                if context_manager.line_would_fit(line) {
                    context_manager.add_line(line);
                    processed_above.insert(0, line.to_owned());
                    return true;
                }
                false
            };

        let mut try_add_below_line =
            |line: &str, context_manager: &mut ContextWindowTracker| -> bool {
                if context_manager.line_would_fit(line) {
                    context_manager.add_line(line);
                    processed_below.push(line.to_owned());
                    return true;
                }
                false
            };

        let mut above_index: i64 = <i64>::try_from(lines_above.len() - 1).expect("to work");
        let mut below_index = 0;
        let mut can_add_above = true;
        let mut can_add_below = true;

        for index in 0..100 {
            if !can_add_above || (can_add_below && index % 4 == 3) {
                if below_index < lines_below.len()
                    && try_add_below_line(&lines_below[below_index], self)
                {
                    below_index += 1;
                } else {
                    can_add_below = false;
                }
            } else {
                if above_index >= 0
                    && try_add_above_line(
                        &lines_above[<usize>::try_from(above_index).expect("to work")],
                        self,
                    )
                {
                    above_index -= 1;
                } else {
                    can_add_above = false;
                }
            }
        }

        OutlineForRange::new(processed_above.join("\n"), processed_below.join("\n"))
    }
}

#[derive(Debug)]
pub struct ContextParserInLineEdit {
    language: String,
    _unique_identifier: String,
    first_line_index: i64,
    last_line_index: i64,
    is_complete: bool,
    non_trim_whitespace_character_count: i64,
    start_marker: String,
    end_marker: String,
    // This is the lines coming from the source
    source_lines: Vec<String>,
    /// This is the lines we are going to use for the context
    lines: Vec<String>,
    fs_file_path: String,
}

impl ContextParserInLineEdit {
    pub fn new(
        language: String,
        unique_identifier: String,
        lines_count: i64,
        source_lines: Vec<String>,
        fs_file_path: String,
    ) -> Self {
        let comment_style = "//".to_owned();
        Self {
            language,
            _unique_identifier: unique_identifier.to_owned(),
            first_line_index: lines_count,
            last_line_index: -1,
            is_complete: false,
            non_trim_whitespace_character_count: 0,
            // we also need to provide the comment style here, lets assume
            // that we are using //
            start_marker: format!("{} BEGIN: {}", &comment_style, unique_identifier),
            end_marker: format!("{} END: {}", &comment_style, unique_identifier),
            source_lines,
            lines: vec![],
            fs_file_path,
        }
    }

    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn start_marker(&self) -> &str {
        &self.start_marker
    }

    pub fn end_marker(&self) -> &str {
        &self.end_marker
    }

    pub fn to_agent_selection_data(&self) -> InLineAgentSelectionData {
        InLineAgentSelectionData::new(
            self.has_context(),
            self.first_line_index,
            self.last_line_index,
            self.lines.to_vec(),
        )
    }

    pub fn first_line_index(&self) -> i64 {
        self.first_line_index
    }

    pub fn last_line_index(&self) -> i64 {
        self.last_line_index
    }

    pub fn line_string(&self) -> String {
        self.lines.join("\n")
    }

    pub fn is_complete(&self) -> bool {
        self.is_complete
    }

    pub fn mark_complete(&mut self) {
        self.is_complete = true;
    }

    pub fn has_context(&self) -> bool {
        if self.lines.len() == 0 || self.non_trim_whitespace_character_count == 0 {
            false
        } else {
            !self.lines.is_empty()
        }
    }

    pub fn prepend_line(
        &mut self,
        line_index: usize,
        character_limit: &mut ContextWindowTracker,
    ) -> bool {
        let line_text = self.source_lines[line_index].to_owned();
        if !character_limit.line_would_fit(&line_text) {
            return false;
        }

        self.first_line_index = std::cmp::min(self.first_line_index, line_index as i64);
        self.last_line_index = std::cmp::max(self.last_line_index, line_index as i64);

        character_limit.add_line(&line_text);
        self.non_trim_whitespace_character_count += line_text.trim().len() as i64;
        self.lines.insert(0, line_text);

        true
    }

    pub fn append_line(
        &mut self,
        line_index: usize,
        character_limit: &mut ContextWindowTracker,
    ) -> bool {
        let line_text = self.source_lines[line_index].to_owned();
        if !character_limit.line_would_fit(&line_text) {
            return false;
        }

        self.first_line_index = std::cmp::min(self.first_line_index, line_index as i64);
        self.last_line_index = std::cmp::max(self.last_line_index, line_index as i64);

        character_limit.add_line(&line_text);
        self.non_trim_whitespace_character_count += line_text.trim().len() as i64;
        self.lines.push(line_text);

        true
    }

    pub fn trim(&mut self, range: Option<&Range>) {
        // now we can begin trimming it on a range if appropriate and then
        // do things properly
        let last_line_index = if let Some(range) = range.clone() {
            if self.last_line_index
                < range
                    .start_position()
                    .line()
                    .try_into()
                    .expect("usize to i64 not fail")
            {
                self.last_line_index
            } else {
                range
                    .start_position()
                    .line()
                    .try_into()
                    .expect("usize to i64 not fail")
            }
        } else {
            self.last_line_index
        };
        for _ in self.first_line_index..last_line_index {
            if self.lines.len() > 0 && self.lines[0].trim().len() == 0 {
                self.first_line_index += 1;
                self.lines.remove(0);
            }
        }

        let first_line_index = if let Some(range) = range {
            if self.first_line_index
                > range
                    .end_position()
                    .line()
                    .try_into()
                    .expect("usize to i64 not fail")
            {
                self.first_line_index
            } else {
                range
                    .end_position()
                    .line()
                    .try_into()
                    .expect("usize to i64 not fail")
            }
        } else {
            self.first_line_index
        };

        for _ in first_line_index..self.last_line_index {
            if self.lines.len() > 0 && self.lines[self.lines.len() - 1].trim().len() == 0 {
                self.last_line_index -= 1;
                self.lines.pop();
            }
        }
    }

    pub fn generate_prompt(&self, should_use_markers: bool) -> Vec<String> {
        if !self.has_context() {
            Default::default()
        } else {
            let mut lines: Vec<String> = vec![];
            let language = &self.language;
            lines.push(format!("```{language}"));
            if should_use_markers {
                lines.push(self.start_marker.clone());
            }
            let fs_file_path = &self.fs_file_path;
            lines.push(format!("// FILEPATH: {fs_file_path}"));
            lines.extend(self.lines.to_vec().into_iter());
            if should_use_markers {
                lines.push(self.end_marker.clone());
            }
            lines.push("```".to_owned());
            lines
        }
    }
}

#[derive(Debug)]
struct SelectionLimits {
    above_line_index: i64,
    below_line_index: i64,
    minimum_line_index: i64,
    maximum_line_index: i64,
}

fn expand_above_and_below_selections(
    above: &mut ContextParserInLineEdit,
    below: &mut ContextParserInLineEdit,
    token_count: &mut ContextWindowTracker,
    selection_limits: SelectionLimits,
) {
    let mut prepend_line_index = selection_limits.above_line_index;
    let mut append_line_index = selection_limits.below_line_index;
    let mut can_prepend = true;
    let mut can_append = true;
    for iteration in 0..100 {
        if !can_prepend || (can_append && iteration % 4 == 3) {
            // If we're within the allowed range and the append is successful, increase the index
            if append_line_index <= selection_limits.maximum_line_index
                && below.append_line(
                    append_line_index
                        .try_into()
                        .expect("usize to i64 will not fail"),
                    token_count,
                )
            {
                append_line_index += 1;
            } else {
                // Otherwise, set the flag to stop appending
                can_append = false;
            }
        } else {
            // If we're within the allowed range and the prepend is successful, decrease the index
            if prepend_line_index >= selection_limits.minimum_line_index
                && above.prepend_line(
                    prepend_line_index
                        .try_into()
                        .expect("usize to i64 will not fail"),
                    token_count,
                )
            {
                prepend_line_index -= 1;
            } else {
                // Otherwise, set the flag to stop prepending
                can_prepend = false;
            }
        }
    }
    if prepend_line_index < selection_limits.minimum_line_index {
        above.mark_complete();
    }
    if append_line_index > selection_limits.maximum_line_index {
        below.mark_complete();
    }
}

#[derive(Debug)]
pub struct SelectionContext {
    pub above: ContextParserInLineEdit,
    pub range: ContextParserInLineEdit,
    pub below: ContextParserInLineEdit,
}

impl SelectionContext {
    pub fn get_selection_range(&self) -> Range {
        Range::new(
            Position::new(
                self.range
                    .first_line_index()
                    .try_into()
                    .expect("i64 to usize to work"),
                0,
                0,
            ),
            Position::new(
                self.range
                    .last_line_index()
                    .try_into()
                    .expect("i64 to usize to work"),
                0,
                0,
            ),
        )
    }

    pub fn to_context_selection(self) -> ContextSelection {
        ContextSelection::new(
            self.above.to_agent_selection_data(),
            self.range.to_agent_selection_data(),
            self.below.to_agent_selection_data(),
        )
    }
}

pub fn generate_selection_context_for_fix(
    // this is the total line count in the file
    line_count: i64,
    // this is the range we are interested in fixing
    fix_range_of_interest: &Range,
    // range which is under the selection from the error
    selection_range: &Range,
    language: &str,
    lines: Vec<String>,
    fs_file_path: String,
    mut token_count: &mut ContextWindowTracker,
) -> SelectionContext {
    let mut in_range = ContextParserInLineEdit::new(
        language.to_owned(),
        "ed8c6549bwf9".to_owned(),
        line_count,
        lines.to_vec(),
        fs_file_path.to_owned(),
    );
    let mut above = ContextParserInLineEdit::new(
        language.to_owned(),
        "abpxx6d04wxr".to_owned(),
        line_count,
        lines.to_vec(),
        fs_file_path.to_owned(),
    );
    let mut below = ContextParserInLineEdit::new(
        language.to_owned(),
        "be15d9bcejpp".to_owned(),
        line_count,
        lines.to_vec(),
        fs_file_path,
    );
    let mut should_expand = true;
    let middle_line =
        (selection_range.start_position().line() + selection_range.end_position().line()) / 2;
    let size = std::cmp::min(
        middle_line - fix_range_of_interest.start_position().line(),
        fix_range_of_interest.end_position().line() - middle_line,
    );
    in_range.append_line(middle_line, token_count);
    for index in 1..=size {
        if !should_expand {
            break;
        }
        let start_line_before = middle_line - index;
        let end_line_after = middle_line + index;
        if (start_line_before >= fix_range_of_interest.start_position().line()
            && !in_range.prepend_line(start_line_before, token_count))
            || (end_line_after <= fix_range_of_interest.end_position().line()
                && !in_range.append_line(end_line_after, token_count))
        {
            should_expand = false;
            break;
        }
    }
    if !should_expand {
        above.trim(None);
        in_range.trim(None);
        below.trim(None);
        return SelectionContext {
            above,
            range: in_range,
            below,
        };
    }
    expand_above_and_below_selections(
        &mut above,
        &mut below,
        &mut token_count,
        SelectionLimits {
            above_line_index: i64::try_from(fix_range_of_interest.start_position().line())
                .expect("usize to i64 to work")
                - 1,
            below_line_index: i64::try_from(fix_range_of_interest.end_position().line())
                .expect("usize to i64 to work")
                + 1,
            minimum_line_index: 0,
            maximum_line_index: line_count - 1,
        },
    );
    above.trim(None);
    below.trim(None);
    in_range.trim(None);
    SelectionContext {
        above,
        range: in_range,
        below,
    }
}

pub fn generate_selection_context(
    line_count: i64,
    original_selection: &Range,
    range_to_maintain: &Range,
    expanded_range: &Range,
    language: &str,
    lines: Vec<String>,
    fs_file_path: String,
    mut token_count: &mut ContextWindowTracker,
) -> SelectionContext {
    // Change this later on, this is the limits on the characters right
    // now and not the tokens
    let mut in_range = ContextParserInLineEdit::new(
        language.to_owned(),
        "ed8c6549bwf9".to_owned(),
        line_count,
        lines.to_vec(),
        fs_file_path.to_owned(),
    );
    let mut above = ContextParserInLineEdit::new(
        language.to_owned(),
        "abpxx6d04wxr".to_owned(),
        line_count,
        lines.to_vec(),
        fs_file_path.to_owned(),
    );
    let mut below = ContextParserInLineEdit::new(
        language.to_owned(),
        "be15d9bcejpp".to_owned(),
        line_count,
        lines.to_vec(),
        fs_file_path,
    );
    let start_line = range_to_maintain.start_position().line();
    let end_line = range_to_maintain.end_position().line();

    for index in (start_line..=end_line).rev() {
        if !in_range.prepend_line(index, &mut token_count) {
            above.trim(None);
            in_range.trim(Some(original_selection));
            below.trim(None);
            return {
                SelectionContext {
                    above,
                    range: in_range,
                    below,
                }
            };
        }
    }

    // Now we can try and expand the above and below ranges, since
    // we have some space for the context
    expand_above_and_below_selections(
        &mut above,
        &mut below,
        &mut token_count,
        SelectionLimits {
            above_line_index: i64::try_from(range_to_maintain.start_position().line())
                .expect("usize to i64 to work")
                - 1,
            below_line_index: i64::try_from(range_to_maintain.end_position().line())
                .expect("usize to i64 to work")
                + 1,
            minimum_line_index: std::cmp::max(
                0,
                expanded_range
                    .start_position()
                    .line()
                    .try_into()
                    .expect("usize to i64 to work"),
            ),
            maximum_line_index: std::cmp::min(
                line_count - 1,
                expanded_range
                    .end_position()
                    .line()
                    .try_into()
                    .expect("usize to i64 to work"),
            ),
        },
    );

    // Now we trim out the ranges again and send the result back
    above.trim(None);
    below.trim(None);
    in_range.trim(Some(original_selection));
    SelectionContext {
        above,
        range: in_range,
        below,
    }
}

#[derive(Debug)]
pub struct SelectionWithOutlines {
    pub selection_context: SelectionContext,
    pub outline_above: String,
    pub outline_below: String,
}

impl SelectionWithOutlines {
    pub fn to_context_selection(&self) -> ContextSelection {
        ContextSelection::new(
            self.selection_context.above.to_agent_selection_data(),
            self.selection_context.range.to_agent_selection_data(),
            self.selection_context.below.to_agent_selection_data(),
        )
    }

    pub fn fs_file_path(&self) -> String {
        self.selection_context.above.fs_file_path.to_owned()
    }
}

pub fn generate_context_for_range(
    source_code: Vec<u8>,
    lines_count: usize,
    original_selection: &Range,
    maintain_range: &Range,
    expanded_range: &Range,
    language: &str,
    character_limit: usize,
    source_lines: Vec<String>,
    function_bodies: Vec<FunctionInformation>,
    fs_file_path: String,
) -> SelectionWithOutlines {
    // Here we will try 2 things:
    // - try to send the whole document as the context first
    // - if that fails, then we try to send the partial document as the
    // context

    let line_count_i64 = <i64>::try_from(lines_count).expect("usize to i64 should not fail");

    // first try with the whole context
    let mut token_tracker = ContextWindowTracker::new(character_limit);
    let selection_context = generate_selection_context(
        line_count_i64,
        original_selection,
        maintain_range,
        &Range::new(Position::new(0, 0, 0), Position::new(lines_count, 0, 0)),
        language,
        source_lines.to_vec(),
        fs_file_path.to_owned(),
        &mut token_tracker,
    );
    if !(selection_context.above.has_context() && !selection_context.above.is_complete()) {
        return SelectionWithOutlines {
            selection_context,
            outline_above: "".to_owned(),
            outline_below: "".to_owned(),
        };
    }

    // now we try to send just the amount of data we have in the selection
    let mut token_tracker = ContextWindowTracker::new(character_limit);
    let restricted_selection_context = generate_selection_context(
        line_count_i64,
        original_selection,
        maintain_range,
        expanded_range,
        language,
        source_lines,
        fs_file_path,
        &mut token_tracker,
    );
    let mut outline_above = "".to_owned();
    let mut outline_below = "".to_owned();
    if restricted_selection_context.above.is_complete()
        && restricted_selection_context.below.is_complete()
    {
        let generated_outline = OutlineForRange::generate_outline_for_range(
            function_bodies,
            expanded_range.clone(),
            language,
            source_code,
        );
        // this is where we make sure we are fitting the above and below
        // into the context window
        let processed_outline = token_tracker.process_outlines(generated_outline);
        (outline_above, outline_below) = processed_outline.get_tuple();
    }

    SelectionWithOutlines {
        selection_context: restricted_selection_context,
        outline_above,
        outline_below,
    }
}

#[derive(Debug, Clone)]
pub struct EditExpandedSelectionRange {
    pub expanded_selection: Range,
    pub range_expanded_to_functions: Range,
    pub function_bodies: Vec<FunctionInformation>,
}

impl EditExpandedSelectionRange {
    pub fn new(
        expanded_selection: Range,
        range_expanded_to_functions: Range,
        function_bodies: Vec<FunctionInformation>,
    ) -> Self {
        Self {
            expanded_selection,
            range_expanded_to_functions,
            function_bodies,
        }
    }
}
