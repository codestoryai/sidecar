use std::{collections::HashSet, sync::Arc};

/// Here we are going to parse the diff stat output and see if we can figure
/// out what kind of merging questions we should ask to the LLM
use serde::{Deserialize, Serialize};
use sidecar::agent::{
    llm_funcs,
    prompts::{self, diff_accept_prompt},
};

fn get_content_from_file_line(content: &str, line_number: String) -> String {
    let lines: Vec<String> = content
        .lines()
        .into_iter()
        .map(|s| s.to_owned())
        .collect::<Vec<_>>();
    let line_number_usize: usize = line_number.trim().parse::<usize>().expect("to work");
    lines[line_number_usize - 1].to_owned()
}

#[tokio::main]
async fn main() {
    // read left from this file: /Users/skcd/scratch/sidecar/src/bin/testing.ts
    // read right from here: /Users/skcd/scratch/sidecar/src/bin/testing2.ts
    let left = std::fs::read_to_string("/Users/skcd/scratch/sidecar/src/bin/testing.ts").unwrap();
    let right = std::fs::read_to_string("/Users/skcd/scratch/sidecar/src/bin/testing2.ts").unwrap();

    let user_query = "Can you make the run function sync?";

    let file_lines = parse_difft_output(left, right).await;
    let final_response = process_file_lines_to_gpt(file_lines, user_query).await;
    println!("==============================");
    println!("==============================");
    println!("{}", final_response.join("\n"));
    println!("==============================");
    println!("==============================");
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

async fn call_gpt_for_action_resolution(
    current_changes: Vec<String>,
    incoming_changes: Vec<String>,
    prefix: Vec<String>,
    query: &str,
) -> Vec<String> {
    let system_message = llm_funcs::llm::Message::system(&diff_accept_prompt(query));
    let user_messages = prompts::diff_user_messages(
        &prefix.join("\n"),
        &current_changes.join("\n"),
        &incoming_changes.join("\n"),
    )
    .into_iter()
    .map(|message| llm_funcs::llm::Message::user(&message));
    let messages = vec![system_message]
        .into_iter()
        .chain(user_messages)
        .collect::<Vec<_>>();
    let llm_client = Arc::new(llm_funcs::LlmClient::codestory_infra());
    let model = llm_funcs::llm::OpenAIModel::GPT4;
    let response = llm_client.response(model, messages, None, 0.1, None).await;
    dbg!(&response);
    let diff_action = match response {
        Ok(response) => DiffActionResponse::from_gpt_response(&response),
        Err(_) => {
            // leave it as it is
            None
        }
    };
    match diff_action {
        Some(DiffActionResponse::AcceptCurrentChanges) => {
            // we have to accept the current changes
            current_changes
        }
        Some(DiffActionResponse::AcceptIncomingChanges) => {
            // we have to accept the incoming changes
            incoming_changes
        }
        Some(DiffActionResponse::AcceptBothChanges) => {
            // we have to accept both the changes
            current_changes
                .into_iter()
                .chain(incoming_changes)
                .collect()
        }
        None => {
            // we have to accept the current changes
            current_changes
        }
    }
}

/// We will use gpt to generate the lines of the code which should be applied
/// to the delta using llm (this is like the machine version of doing git diff(accept/reject))
async fn process_file_lines_to_gpt(file_lines: Vec<String>, user_query: &str) -> Vec<String> {
    // Find where the markers are and then send it over to the llm and ask it
    // to accept/reject the code which has been generated.
    // we detect the git markers and use that for sending over the file and showing that to the LLM
    // we have to check for the <<<<<<, ======, >>>>>> markers and then send the code in between these ranges
    // and 5 lines of prefix to the LLM to ask it to perform the git operation
    // and then use that to build up the file thats how we can solve this
    let mut initial_index = 0;
    let total_lines = file_lines.len();
    dbg!(&file_lines);
    let mut total_file_lines: Vec<String> = vec![];
    while initial_index < total_lines {
        let line = file_lines[initial_index].to_owned();
        if line.contains("<<<<<<<") {
            let mut current_changes = vec![];
            let mut current_iteration_index = initial_index + 1;
            while !file_lines[current_iteration_index].contains("=======") {
                // we have to keep going here
                current_changes.push(file_lines[current_iteration_index].to_owned());
                current_iteration_index = current_iteration_index + 1;
            }
            // Now we are at the index which has ======, so move to the next one
            current_iteration_index = current_iteration_index + 1;
            let mut incoming_changes = vec![];
            while !file_lines[current_iteration_index].contains(">>>>>>>") {
                // we have to keep going here
                incoming_changes.push(file_lines[current_iteration_index].to_owned());
                current_iteration_index = current_iteration_index + 1;
            }
            // This is where we will call the agent out and ask it to decide
            // which of the following git diffs to keep and which to remove
            // before we do this, we can do some hand-woven checks to figure out
            // what action to take
            // we also want to keep a prefix of the lines here and send that along
            // to the llm for context as well
            let selection_lines = call_gpt_for_action_resolution(
                current_changes,
                incoming_changes,
                total_file_lines
                    .iter()
                    .rev()
                    .take(5)
                    .rev()
                    .into_iter()
                    .map(|s| s.to_owned())
                    .collect::<Vec<_>>(),
                user_query,
            )
            .await;
            total_file_lines.extend(selection_lines.to_vec());
            println!("===== selection lines =====");
            println!("{}", selection_lines.to_vec().join("\n"));
            println!("===== selection lines =====");
            println!("==============================");
            println!("==============================");
            println!("{}", total_file_lines.join("\n"));
            println!("==============================");
            println!("==============================");
            // Now we are at the index which has >>>>>>>, so move to the next one on the iteration loop
            initial_index = current_iteration_index + 1;
            // we have a git diff event now, so lets try to fix that
        } else {
            // just insert the line here and then push the current line to the
            // total_file_lines
            total_file_lines.push(line);
            initial_index = initial_index + 1;
        }
    }
    println!("==============================");
    println!("==============================");
    println!("{}", total_file_lines.join("\n"));
    println!("==============================");
    println!("==============================");
    unimplemented!("something here");
}

// Here we will first parse the llm output and get the left and right links
async fn parse_difft_output(left: String, right: String) -> Vec<String> {
    let left_lines: Vec<String> = left
        .lines()
        .into_iter()
        .map(|s| s.to_owned())
        .collect::<Vec<_>>();
    let right_lines: Vec<String> = right
        .lines()
        .into_iter()
        .map(|s| s.to_owned())
        .collect::<Vec<_>>();
    let left_lines: Vec<Option<(&str, Option<bool>)>> = vec![
        Some((" 1 ", Some(false))),
        Some((" 2 ", Some(false))),
        Some((" 3 ", None)),
        Some((" 4 ", Some(false))),
        Some((" 5 ", Some(false))),
        Some((" 6 ", Some(false))),
        Some((" 7 ", Some(false))),
        Some((" 8 ", Some(false))),
        Some((" 9 ", Some(false))),
        Some(("10 ", Some(false))),
        Some(("11 ", Some(false))),
        Some(("12 ", Some(false))),
        Some(("13 ", Some(false))),
        Some(("14 ", Some(false))),
        Some(("15 ", Some(false))),
        Some(("16 ", Some(false))),
        Some(("17 ", Some(false))),
        Some(("18 ", Some(false))),
        Some(("19 ", Some(false))),
        Some(("20 ", Some(false))),
        Some(("21 ", Some(false))),
        Some(("22 ", Some(false))),
        Some(("23 ", Some(false))),
        Some(("24 ", Some(false))),
        Some(("25 ", Some(false))),
        Some(("26 ", Some(false))),
        Some(("27 ", Some(false))),
        Some(("28 ", Some(false))),
        Some(("29 ", Some(false))),
        Some(("30 ", Some(false))),
        Some(("31 ", Some(false))),
        Some(("32 ", Some(false))),
        Some(("33 ", Some(false))),
        Some(("34 ", Some(false))),
        Some(("35 ", Some(false))),
        Some(("36 ", Some(false))),
        Some(("37 ", Some(false))),
        Some(("38 ", Some(false))),
        Some(("39 ", Some(false))),
        Some(("40 ", None)),
        Some(("41 ", None)),
        Some(("42 ", Some(false))),
        Some(("43 ", None)),
        Some(("44 ", None)),
        Some(("45 ", Some(false))),
        Some(("46 ", None)),
        Some(("47 ", Some(false))),
        Some(("48 ", Some(false))),
        Some(("49 ", Some(false))),
        Some(("50 ", None)),
        Some(("51 ", None)),
        Some(("52 ", Some(false))),
        Some(("53 ", None)),
        Some(("54 ", Some(false))),
        Some(("55 ", Some(false))),
        Some(("56 ", Some(false))),
        Some(("57 ", Some(false))),
        Some(("58 ", Some(false))),
        Some(("59 ", Some(false))),
        Some(("60 ", Some(false))),
    ];
    let right_lines: Vec<Option<(&str, Option<bool>)>> = vec![
        None,
        Some((" 1 ", Some(true))),
        Some((" 2 ", None)),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some((" 3 ", Some(true))),
        Some((" 4 ", None)),
        Some((" 5 ", None)),
        Some((" 6 ", Some(true))),
        Some((" 7 ", None)),
        Some((" 8 ", None)),
        Some((" 9 ", Some(true))),
        Some(("10 ", None)),
        Some(("11 ", Some(true))),
        None,
        None,
        Some(("12 ", None)),
        Some(("13 ", None)),
        Some(("14 ", Some(true))),
        Some(("15 ", None)),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ];
    dbg!(left_lines.len());
    dbg!(right_lines.len());
    assert_eq!(left_lines.len(), right_lines.len());
    let mut final_output: Vec<String> = vec![];
    let mut iteration_index = 0;
    let left_lines_limit = left_lines.len();
    let mut final_lines_file: Vec<String> = vec![];
    // Remember: left is our main file and right is the diff which the LLM has
    // generated
    while iteration_index < left_lines_limit {
        // dbg!("iterating loop break, iterating again");
        loop {
            // dbg!("loop iteration", iteration_index);
            if iteration_index >= left_lines_limit {
                break;
            }
            // Now we will here greedily try to insert the markers for git and then
            let left_content_now_maybe = left_lines[iteration_index];
            if iteration_index >= right_lines.len() {
                // empty the left side to the final lines
                loop {
                    let left_content_now_maybe = left_lines[iteration_index];
                    final_lines_file.push(get_content_from_file_line(
                        &left,
                        left_content_now_maybe.unwrap().0.to_owned(),
                    ));
                    iteration_index = iteration_index + 1;
                    if iteration_index >= left_lines.len() {
                        break;
                    }
                }
            }
            let right_content_now_maybe = right_lines[iteration_index];
            // we have content on the left but nothing on the right, so we keep going for as long
            // as possible we have content
            if left_content_now_maybe.is_some() && right_content_now_maybe.is_none() {
                // Let's get the color of the left side
                // we will always have a left color ALWAYS and it will be RED or false
                final_lines_file.push(get_content_from_file_line(
                    &left,
                    left_content_now_maybe.unwrap().0.to_owned(),
                ));
                // Now we can start going down on left and right, if we keep getting
                // right None as usual..
                loop {
                    iteration_index = iteration_index + 1;
                    if left_lines.len() >= iteration_index {
                        break;
                    }
                    if right_lines.len() <= iteration_index {
                        // If we are here, we have to collect the rest of the lines
                        // in the right and call it a day
                        loop {
                            let left_content_now_maybe = left_lines[iteration_index];
                            final_lines_file.push(get_content_from_file_line(
                                &left,
                                left_content_now_maybe.unwrap().0.to_owned(),
                            ));
                            iteration_index = iteration_index + 1;
                            if iteration_index >= left_lines.len() {
                                break;
                            }
                        }
                        break;
                    }
                    // otherwise we want to keep checking the lines after this
                    let left_content_now_maybe = left_lines[iteration_index];
                    let right_content_now_maybe = right_lines[iteration_index];
                    if !(left_content_now_maybe.is_some() && right_content_now_maybe.is_none()) {
                        // we are not in the same style as before, so we break it
                        break;
                    } else {
                        final_output
                            .push(left_content_now_maybe.expect("to be there ").0.to_owned());
                    }
                }
                break;
            }
            // we have some content on the right but nothing ont he left
            if left_content_now_maybe.is_none() && right_content_now_maybe.is_some() {
                // Now we are in a state where we can be sure that on the right
                // we have a GREEN and nothing on the left side, cause that's
                // the only case where its possible
                final_lines_file.push(get_content_from_file_line(
                    &right,
                    right_content_now_maybe.unwrap().0.to_owned(),
                ));
                // Now we start the loop again
                loop {
                    iteration_index = iteration_index + 1;
                    if right_lines.len() >= iteration_index {
                        break;
                    }
                    let left_content_now_maybe = left_lines[iteration_index];
                    let right_content_now_maybe = right_lines[iteration_index];
                    if !(left_content_now_maybe.is_none() && right_content_now_maybe.is_some()) {
                        break;
                    } else {
                        final_output.push(get_content_from_file_line(
                            &right,
                            right_content_now_maybe.expect("to be there ").0.to_owned(),
                        ));
                    }
                }
                break;
            }
            // we have content on both the sides, so we keep going
            if left_content_now_maybe.is_some() && right_content_now_maybe.is_some() {
                // things get interesting here, so let's handle each case by case
                let left_color = left_content_now_maybe.unwrap().1;
                let right_color = right_content_now_maybe.unwrap().1;
                let left_content =
                    get_content_from_file_line(&left, left_content_now_maybe.unwrap().0.to_owned());
                let right_content = get_content_from_file_line(
                    &right,
                    right_content_now_maybe.unwrap().0.to_owned(),
                );
                // no change both are equivalent, best case <3
                if left_color.is_none() && right_color.is_none() {
                    final_lines_file.push(get_content_from_file_line(
                        &left,
                        left_content_now_maybe.unwrap().0.to_owned(),
                    ));
                    iteration_index = iteration_index + 1;
                    continue;
                }
                // if we have some color on the left and no color on the right
                // we have to figure out what to do
                // this case represents deletion on the left line and no change
                // on the right line, so we want to keep the left line and not
                // delete it, this is akin to a deletion and insertion
                if left_color.is_some() && right_color.is_none() {
                    // in this case the LLM predicted that we have to remove
                    // a line, this is generally the case with whitespace
                    // otherwise we get a R and G on both sides
                    final_lines_file.push(get_content_from_file_line(
                        &left,
                        left_content_now_maybe.unwrap().0.to_owned(),
                    ));
                    iteration_index = iteration_index + 1;
                    continue;
                }
                if left_color.is_none() && right_color.is_some() {
                    // This is the complicated case we have to handle
                    // this is generally when the LLM wants to edit the file but
                    // whats added here is mostly a comment or something similar
                    // so we can just add the right content here and move on
                    final_lines_file.push(get_content_from_file_line(
                        &right,
                        right_content_now_maybe.unwrap().0.to_owned(),
                    ));
                    iteration_index = iteration_index + 1;
                    continue;
                }
                if left_color.is_some() && right_color.is_some() {
                    // we do have to insert a diff range here somehow
                    // but how long will be defined by the sequence after this
                    let mut left_content_vec = vec![left_content];
                    let mut right_content_vec = vec![right_content];
                    loop {
                        // the condition we want to look for here is the following
                        // R G
                        // R .
                        // R .
                        // ...
                        // This means that there is a range in the left range
                        // which we have to replace with the Green
                        // we keep going until we have a non-color on the left
                        // or right gets some content
                        iteration_index = iteration_index + 1;
                        if iteration_index >= left_lines.len() {
                            // If this happens, we can send a diff with the current
                            // collection
                            final_lines_file.push("<<<<<<<".to_owned());
                            final_lines_file.append(&mut left_content_vec);
                            final_lines_file.push("=======".to_owned());
                            final_lines_file.append(&mut right_content_vec);
                            final_lines_file.push(">>>>>>>".to_owned());
                            break;
                        }
                        let left_content_now_maybe = left_lines[iteration_index];
                        let right_content_now_maybe = right_lines[iteration_index];
                        // if the left content is none here, then we are taking
                        // a L, then we have to break from the loop right now
                        if left_content_now_maybe.is_none() {
                            final_lines_file.push("<<<<<<<".to_owned());
                            final_lines_file.append(&mut left_content_vec);
                            final_lines_file.push("=======".to_owned());
                            final_lines_file.append(&mut right_content_vec);
                            final_lines_file.push(">>>>>>>".to_owned());
                            break;
                        }
                        let left_color_updated = left_content_now_maybe.unwrap().1;
                        if left_color_updated == left_color && right_content_now_maybe.is_none() {
                            // we have to keep going here
                            left_content_vec.push(get_content_from_file_line(
                                &left,
                                left_content_now_maybe.unwrap().0.to_owned(),
                            ));
                            continue;
                        } else {
                            // we have to break here
                            final_lines_file.push("<<<<<<<".to_owned());
                            final_lines_file.append(&mut left_content_vec);
                            final_lines_file.push("=======".to_owned());
                            final_lines_file.append(&mut right_content_vec);
                            final_lines_file.push(">>>>>>>".to_owned());
                            break;
                        }
                    }
                    continue;
                }
                break;
            }
        }
    }
    let final_lines_vec = final_lines_file.to_vec();
    let final_content = final_lines_file.join("\n");
    println!("=============================================");
    println!("=============================================");
    println!("{}", final_content);
    println!("=============================================");
    println!("=============================================");
    final_lines_vec
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Unchanged,
    Changed,
    Created,
    Deleted,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
// TODO: use syntax::TokenKind and syntax::AtomKind instead of this merged enum,
// blocked by https://github.com/serde-rs/serde/issues/1402
enum Highlight {
    Delimiter,
    Normal,
    String,
    Type,
    Comment,
    Keyword,
    TreeSitterError,
}

#[derive(Debug, Serialize, Deserialize)]
struct Change {
    start: u32,
    end: u32,
    content: String,
    highlight: Highlight,
}

#[derive(Debug, Serialize, Deserialize)]
struct Side {
    line_number: u32,
    changes: Vec<Change>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Line {
    #[serde(skip_serializing_if = "Option::is_none")]
    lhs: Option<Side>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rhs: Option<Side>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct File {
    path: String,
    chunks: Vec<Vec<Line>>,
    status: Status,
}

// async fn run_diffstat_prompts(source_code: &str, llm_code: &str) {
//     // we will call out to the diffstat binary and then parse the output
// }
