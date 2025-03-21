use std::sync::Arc;

use axum::response::{sse, IntoResponse, Sse};
use axum::{Extension, Json};
use llm_client::broker::LLMBroker;
use llm_client::clients::codestory::CodeStoryClient;
use llm_prompts::reranking::types::{CodeSpan, CodeSpanDigest};
use regex::Regex;
use tracing::info;

use crate::application::application::Application;
use crate::chunking::languages::TSLanguageParsing;
use crate::chunking::text_document::Range;
use crate::chunking::types::ClassNodeType;
use crate::in_line_agent::types::ContextSelection;
use crate::reranking::snippet_reranking::rerank_snippets;

use super::model_selection::LLMClientConfig;
use super::types::{ApiResponse, Result};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EditFileRequest {
    pub file_path: String,
    pub file_content: String,
    pub new_content: String,
    pub language: String,
    pub user_query: String,
    pub session_id: String,
    pub code_block_index: usize,
    pub model_config: LLMClientConfig,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TextEditStreaming {
    Start {
        code_block_index: usize,
        context_selection: ContextSelection,
    },
    EditStreaming {
        code_block_index: usize,
        range: Range,
        content_up_until_now: String,
        content_delta: String,
    },
    End {
        code_block_index: usize,
        reason: String,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum EditFileResponse {
    Message {
        message: String,
    },
    Action {
        action: DiffActionResponse,
        range: Range,
        content: String,
        previous_content: String,
    },
    TextEdit {
        range: Range,
        content: String,
        should_insert: bool,
    },
    TextEditStreaming {
        data: TextEditStreaming,
    },
    Status {
        session_id: String,
        status: String,
    },
}

impl ApiResponse for EditFileResponse {}

pub async fn file_edit(
    Extension(app): Extension<Application>,
    Json(EditFileRequest {
        file_path,
        file_content,
        language,
        new_content,
        user_query,
        session_id,
        code_block_index,
        model_config,
    }): Json<EditFileRequest>,
) -> Result<impl IntoResponse> {
    info!(event_name = "file_edit", file_path = file_path.as_str(),);
    // Here we have to first check if the new content is tree-sitter valid, if
    // thats the case only then can we apply it to the file
    // First we check if the output generated is valid by itself, if it is then
    // we can think about applying the changes to the file
    let llm_broker = app.llm_broker.clone();

    {
        let nearest_range_for_symbols = find_nearest_position_for_code_edit(
            &file_content,
            &file_path,
            &new_content,
            &language,
            app.language_parsing.clone(),
        )
        .await;
        println!("nearest range of symbols: {:?}", &nearest_range_for_symbols);

        // Now we apply the edits and send it over to the user
        // After generating the git diff we want to send back the responses to the
        // user depending on what edit information we get, we can stream this to the
        // user so they know the agent is working on some action and it will show up
        // as edits on the editor
        let split_lines = Regex::new(r"\r\n|\r|\n").unwrap();
        let file_lines: Vec<String> = split_lines
            .split(&file_content)
            .map(|s| s.to_owned())
            .collect();

        let result = send_edit_events(
            file_lines,
            file_content,
            new_content,
            user_query,
            language,
            session_id,
            llm_broker,
            app.language_parsing.clone(),
            file_path,
            nearest_range_for_symbols,
            code_block_index,
            model_config,
        )
        .await;
        result
    }
}

// We use this enum as a placeholder for the different type of variables which we support exporting at the
// moment
#[derive(Debug, Clone)]
enum CodeSymbolInformation {
    Class,
    Function,
    Type,
}

async fn find_nearest_position_for_code_edit(
    file_content: &str,
    _file_path: &str,
    new_content: &str,
    language: &str,
    language_parsing: Arc<TSLanguageParsing>,
) -> Vec<(Option<Range>, CodeSymbolInformation)> {
    // Steps taken:
    // - First get all the classes and functions which are present in the code blocks provided
    // - Get the types which are provided in the code block as well (these might be types or anything else in typescript)
    // - Search the current open file to see if this already exists in the file
    // - If it exists we have a more restricted area to apply the diff to
    // - Handle the imports properly as always
    let language_parser = language_parsing.for_lang(language);
    if language_parser.is_none() {
        return vec![];
    }
    let language_parser = language_parser.unwrap();
    if !language_parser.is_valid_code(new_content) {
        return vec![];
    }
    let class_with_funcs_llm = language_parser.generate_file_symbols(new_content.as_bytes());
    let class_with_funcs = language_parser.generate_file_symbols(file_content.as_bytes());
    let types_llm = language_parser.capture_type_data(new_content.as_bytes());
    let types_file = language_parser.capture_type_data(file_content.as_bytes());
    // First we want to try and match all the classes as much as possible
    // then we will look at the individual functions and try to match them

    // These are the functions which are prensent in the class of the file
    let class_functions_from_file = class_with_funcs_llm
        .to_vec()
        .into_iter()
        .filter_map(|class_with_func| {
            if class_with_func.class_information.is_some() {
                Some(class_with_func.function_information)
            } else {
                None
            }
        })
        .flatten()
        .collect::<Vec<_>>();
    // These are the classes which the llm has generated (we use it to only match with other classes)
    let classes_llm_generated = class_with_funcs_llm
        .to_vec()
        .into_iter()
        .filter_map(|class_with_func| {
            if class_with_func.class_information.is_some() {
                Some(class_with_func.class_information)
            } else {
                None
            }
        })
        .flatten()
        .collect::<Vec<_>>();
    // These are the classes which are present in the file
    let classes_from_file = class_with_funcs
        .to_vec()
        .into_iter()
        .filter_map(|class_with_func| {
            if class_with_func.class_information.is_some() {
                Some(class_with_func.class_information)
            } else {
                None
            }
        })
        .flatten()
        .collect::<Vec<_>>();
    // These are the independent functions which the llm has generated
    let independent_functions_llm_generated = class_with_funcs_llm
        .into_iter()
        .filter_map(|class_with_func| {
            if class_with_func.class_information.is_none() {
                Some(class_with_func.function_information)
            } else {
                None
            }
        })
        .flatten()
        .collect::<Vec<_>>();
    // These are the independent functions which are present in the file
    // TODO(skcd): Pick up from here, for some reason the functions are not matching
    // up properly in the new content and the file content, we want to get the proper
    // function matches so we can ask the llm to rewrite it, and also difftastic is not required
    // as a dependency anymore (yay?) so we can skip it completely :)))
    let independent_functions_from_file = class_with_funcs
        .into_iter()
        .filter_map(|class_with_func| {
            if class_with_func.class_information.is_none() {
                Some(class_with_func.function_information)
            } else {
                None
            }
        })
        .flatten()
        .collect::<Vec<_>>();

    // Now we try to check if any of the functions match,
    // if they do we capture the matching range in the original value, this allows us to have a finer area to apply the diff to
    let llm_functions_to_range = independent_functions_llm_generated
        .into_iter()
        .map(|function_llm| {
            let node_information = function_llm.get_node_information();
            match node_information {
                Some(node_information) => {
                    let function_name_llm = node_information.get_name();
                    let parameters_llm = node_information.get_parameters();
                    let return_type_llm = node_information.get_return_type();
                    // We have the 3 identifiers above to figure out which function can match with this, if none match then we know
                    // that this is a new function and we should treat it as such
                    let mut found_function_vec = independent_functions_from_file
                        .iter()
                        .filter_map(|function_information| {
                            let node_information = function_information.get_node_information();
                            match node_information {
                                Some(node_information) => {
                                    let function_name = node_information.get_name();
                                    let parameters = node_information.get_parameters();
                                    let return_type = node_information.get_return_type();
                                    let score = (function_name_llm == function_name) as usize
                                        + (parameters_llm == parameters) as usize
                                        + (return_type_llm == return_type) as usize;
                                    // We have the 3 identifiers above to figure out which function can match with this, if none match then we know
                                    // that this is a new function and we should treat it as such
                                    if score == 0 || function_name_llm != function_name {
                                        None
                                    } else {
                                        Some((score, function_information.clone()))
                                    }
                                }
                                None => None,
                            }
                        })
                        .collect::<Vec<_>>();
                    found_function_vec.sort_by(|a, b| b.0.cmp(&a.0));
                    let found_function = found_function_vec
                        .first()
                        .map(|(_, function_information)| function_information);
                    if let Some(found_function) = found_function {
                        // We have a match! let's lock onto the range of this function node which we found and then
                        // we can go about applying the diff to this range
                        return (Some(found_function.range().clone()), function_llm);
                    }

                    // Now it might happen that these functions are part of the clas function, in which case
                    // we should check the class functions as well to figure out if that's the case and we can
                    // get the correct range that way
                    let found_function =
                        class_functions_from_file
                            .iter()
                            .find(|function_information| {
                                let node_information = function_information.get_node_information();
                                match node_information {
                                    Some(node_information) => {
                                        let function_name = node_information.get_name();
                                        let parameters = node_information.get_parameters();
                                        let return_type = node_information.get_return_type();
                                        let score = (function_name_llm == function_name) as usize
                                            + (parameters_llm == parameters) as usize
                                            + (return_type_llm == return_type) as usize;
                                        // We have the 3 identifiers above to figure out which function can match with this, if none match then we know
                                        // that this is a new function and we should treat it as such
                                        if score == 0 || function_name_llm != function_name {
                                            false
                                        } else {
                                            true
                                        }
                                    }
                                    None => false,
                                }
                            });
                    if let Some(found_function) = found_function {
                        // We have a match! let's lock onto the range of this function node which we found and then
                        // we can go about applying the diff to this range
                        return (Some(found_function.range().clone()), function_llm);
                    }
                    // If the class function finding also fails, then we just return None here :(
                    // since it might be a new function at this point?
                    (None, function_llm)
                }
                None => (None, function_llm),
            }
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|(range, _function)| (range, CodeSymbolInformation::Function))
        .collect::<Vec<_>>();

    // Now we have to try and match the classes in the same way, so we can figure out if we have a smaller range to apply the diff
    let llm_classes_to_range = classes_llm_generated
        .into_iter()
        .map(|llm_class_information| {
            let class_identifier = llm_class_information.get_name();
            let class_type = llm_class_information.get_class_type();
            match class_type {
                ClassNodeType::ClassDeclaration => {
                    // Try to find which class in the original file this could match with
                    let possible_class = classes_from_file
                        .iter()
                        .find(|class_information| class_information.get_name() == class_identifier);
                    match possible_class {
                        // yay, happy path we found some class, lets return this as the range for the class right now
                        Some(possible_class) => {
                            (Some(possible_class.range().clone()), llm_class_information)
                        }
                        None => (None, llm_class_information),
                    }
                }
                ClassNodeType::Identifier => (None, llm_class_information),
            }
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|(range, _class)| (range, CodeSymbolInformation::Class))
        .collect::<Vec<_>>();

    // Now we try to get the types which the llm has suggested and which might be also present in the file
    // this allows us to figure out the delta between them
    let llm_types_to_range = types_llm
        .into_iter()
        .map(|llm_type_information| {
            let type_identifier = llm_type_information.name.to_owned();
            let possible_type = types_file
                .iter()
                .find(|type_information| type_information.name == type_identifier);
            match possible_type {
                // yay, happy path we found some type, lets return this as the range for the type right now
                Some(possible_type) => (Some(possible_type.range.clone()), llm_type_information),
                None => (None, llm_type_information),
            }
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|(range, _type_information)| (range, CodeSymbolInformation::Type))
        .collect::<Vec<_>>();

    // TODO(skcd): Now we have classes and functions which are mapped to their actual representations in the file
    // this is very useful since our diff application can be more coherent now and we can send over more
    // correct data, but what about the things that we missed? let's get to them in a bit, focus on these first

    // First we have to order the functions and classes in the order of their ranges
    let mut identified: Vec<(Option<Range>, CodeSymbolInformation)> = llm_functions_to_range
        .into_iter()
        .chain(llm_classes_to_range)
        .chain(llm_types_to_range)
        .collect();
    identified.sort_by(|a, b| match (a.0.as_ref(), b.0.as_ref()) {
        (Some(a_range), Some(b_range)) => a_range.start_byte().cmp(&b_range.start_byte()),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    identified
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum DiffActionResponse {
    // Accept the current changes
    AcceptCurrentChanges,
    AcceptIncomingChanges,
    AcceptBothChanges,
}

impl DiffActionResponse {
    pub fn from_gpt_response(response: &str) -> Option<DiffActionResponse> {
        // we are going to parse data between <answer>{your_answer}</answer>
        let response = response
            .split("<answer>")
            .collect::<Vec<_>>()
            .last()
            .unwrap()
            .split("</answer>")
            .collect::<Vec<_>>()
            .first()
            .unwrap()
            .to_owned();
        if response.to_lowercase().contains("accept")
            && response.to_lowercase().contains("current")
            && response.to_lowercase().contains("change")
        {
            return Some(DiffActionResponse::AcceptCurrentChanges);
        }
        if response.to_lowercase().contains("accept")
            && response.to_lowercase().contains("incoming")
            && response.to_lowercase().contains("change")
        {
            return Some(DiffActionResponse::AcceptIncomingChanges);
        }
        if response.to_lowercase().contains("accept")
            && response.to_lowercase().contains("both")
            && response.to_lowercase().contains("change")
        {
            return Some(DiffActionResponse::AcceptBothChanges);
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct FileLineContent {
    pub content: String,
    pub line_content_type: LineContentType,
}

impl FileLineContent {
    pub fn get_content(&self) -> String {
        self.content.to_owned()
    }

    pub fn is_diff_start(&self) -> bool {
        matches!(self.line_content_type, LineContentType::DiffStartMarker)
    }

    pub fn is_diff_end(&self) -> bool {
        matches!(self.line_content_type, LineContentType::DiffEndMarker)
    }

    pub fn is_diff_separator(&self) -> bool {
        matches!(self.line_content_type, LineContentType::DiffSeparator)
    }

    pub fn is_line(&self) -> bool {
        matches!(self.line_content_type, LineContentType::FileLine)
    }

    pub fn from_lines(lines: Vec<String>) -> Vec<Self> {
        lines
            .into_iter()
            .map(|content| FileLineContent {
                line_content_type: {
                    if content.contains("<<<<<<<") {
                        LineContentType::DiffStartMarker
                    } else if content.contains("=======") {
                        LineContentType::DiffSeparator
                    } else if content.contains(">>>>>>>") {
                        LineContentType::DiffEndMarker
                    } else {
                        LineContentType::FileLine
                    }
                },
                content,
            })
            .collect::<Vec<_>>()
    }
}

#[derive(Debug, Clone)]
pub enum LineContentType {
    FileLine,
    DiffStartMarker,
    DiffEndMarker,
    DiffSeparator,
}

async fn send_edit_events(
    file_lines: Vec<String>,
    file_content: String,
    _llm_content: String,
    user_query: String,
    _language: String,
    _session_id: String,
    _llm_broker: Arc<LLMBroker>,
    language_parsing: Arc<TSLanguageParsing>,
    file_path: String,
    _nearest_range_symbols: Vec<(Option<Range>, CodeSymbolInformation)>,
    _code_block_index: usize,
    _model_config: LLMClientConfig,
) -> Result<
    Sse<std::pin::Pin<Box<dyn tokio_stream::Stream<Item = anyhow::Result<sse::Event>> + Send>>>,
> {
    let codestory_client =
        CodeStoryClient::new("https://codestory-provider-dot-anton-390822.ue.r.appspot.com");
    // To implement this using the new flow we do the following:
    // - If the file is less than 500 lines, then we can start editing the whole file
    // - If the file is more than 500 lines, then we use the reranking to select the most relevant parts of the file
    // - We then use the llm to generate the code in that section of the code snippet
    // let mut start_line = None;
    // let mut end_line: Option<u64>;
    if file_lines.len() >= 500 {
        let ts_language_parsing = language_parsing.clone();
        let snippets = ts_language_parsing
            .chunk_file(&file_path, &file_content, None, None)
            .into_iter()
            .enumerate()
            .filter_map(|(idx, span)| {
                let start = span.start;
                let end = span.end;
                match span.data {
                    Some(data) => Some({
                        let data = format!(
                            r#"Line range from: {start}-{end}
{data}"#
                        )
                        .to_owned();
                        let code_span_digest = CodeSpanDigest::new(
                            CodeSpan::new(file_path.to_owned(), start as u64, end as u64, data),
                            &file_path,
                            idx,
                        );
                        code_span_digest
                    }),
                    None => None,
                }
            })
            .collect::<Vec<CodeSpanDigest>>();
        let reranked_snippet = rerank_snippets(&codestory_client, snippets, &user_query).await;

        // Now that we have the ranked snippets we try to see what the suitable range
        // for making edits is
        // Merge the reranked snippets based on their start and end lines
        let mut current_snippet: Option<CodeSpanDigest> = None;

        for snippet in reranked_snippet {
            if let Some(ref mut cs) = current_snippet {
                // if the current snippet and cs both intersect or follow each other
                // then we should merge them
                if cs.code_span().intersects(snippet.code_span()) {
                    // then we update the start and end lines
                    // start_line = Some(
                    //     cs.code_span()
                    //         .start_line()
                    //         .min(snippet.code_span().start_line()),
                    // );
                    // end_line = Some(
                    //     cs.code_span()
                    //         .end_line()
                    //         .max(snippet.code_span().end_line()),
                    // );
                }
            } else {
                // start_line = Some(snippet.code_span().start_line());
                // end_line = Some(snippet.code_span().end_line());
                current_snippet = Some(snippet);
            }
        }
    } else {
        // start_line = Some(0);
        // end_line = Some((file_lines.len() - 1) as u64);
    }

    // we now have the range where we have to make the edit, the next step is to have the llm generate the updates
    // and send it over the wire
    // we also need the LLM prompt here to make the edit
    // yield EditFileResponse::start_text_edit(selection_context, code_block_index);
    // yield EditFileResponse::stream_edit ....
    // yeild EditFileResponse::end_text_edit ....
    unimplemented!();
}
