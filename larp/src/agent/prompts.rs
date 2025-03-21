/// We list out all the prompts here which are required for the agent to work.

/// First we have the search functions which are required by the agent

pub fn code_function() -> serde_json::Value {
    serde_json::json!(
        {
            "name": "code",
            "description":  "Search the contents of files in a codebase semantically. Results will not necessarily match search terms exactly, but should be related.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The query with which to search. This should consist of keywords that might match something in the codebase, e.g. 'react functional components', 'contextmanager', 'bearer token'. It should NOT contain redundant words like 'usage' or 'example'."
                    }
                },
                "required": ["query"]
            }
        }
    )
}

pub fn path_function() -> serde_json::Value {
    serde_json::json!(
        {
            "name": "path",
            "description": "Search the pathnames in a codebase. Use when you want to find a specific file or directory. Results may not be exact matches, but will be similar by some edit-distance.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The query with which to search. This should consist of keywords that might match a path, e.g. 'server/src'."
                    }
                },
                "required": ["query"]
            }
        }
    )
}

pub fn generate_answer_function() -> serde_json::Value {
    serde_json::json!(
        {
            "name": "none",
            "description": "Call this to answer the user. Call this only when you have enough information to answer the user's query.",
            "parameters": {
                "type": "object",
                "properties": {
                    "paths": {
                        "type": "array",
                        "items": {
                            "type": "integer",
                            "description": "The indices of the paths to answer with respect to. Can be empty if the answer is not related to a specific path."
                        }
                    }
                },
                "required": ["paths"]
            }
        }
    )
}

pub fn proc_function() -> serde_json::Value {
    serde_json::json!(
        {
            "name": "proc",
            "description": "Read one or more files and extract the line ranges that are relevant to the search terms",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The query with which to search the files."
                    },
                    "paths": {
                        "type": "array",
                        "items": {
                            "type": "integer",
                            "description": "The indices of the paths to search. paths.len() <= 5"
                        }
                    }
                },
                "required": ["query", "paths"]
            }
        }
    )
}

pub fn functions(add_proc: bool) -> serde_json::Value {
    let mut funcs =
        serde_json::json!([code_function(), path_function(), generate_answer_function()]);

    if add_proc {
        funcs.as_array_mut().unwrap().push(proc_function());
    }
    funcs
}

pub fn lexical_search_functions() -> serde_json::Value {
    let funcs = serde_json::json!([code_function()]);
    funcs
}

pub fn proc_function_truncate() -> serde_json::Value {
    serde_json::json!([proc_function()])
}

pub fn proc_search_system_prompt<'a>(file_outline: Option<String>, file_path: &'a str) -> String {
    match file_outline {
        Some(file_outline) => {
            let system_prompt = format!(
                r#"
##### FILE PATH #####
{file_path}

##### FILE OUTLINE #####
<file_path>
{file_path}
</file_path>

{file_outline}
#####

Your job is to select keywords which should be used to search for relevant code snippets using lexical search for the file path `{file_path}`:

- You are also given a user conversation history, use that to understand which keywords should be used to search for relevant code snippets
- The user conversation is given to you to decide the keywords which can further answer the user query
- You are given an outline of the code in the file, use the outline to select keywords
- ALWAYS call a function, DO NOT answer the question directly, even if the query is not in English
- Only call functions.proc with path indices that are under the PATHS heading above
- Call functions.proc with paths that might contain relevant information. Either because of the path name, or to expand on the code outline
- DO NOT end the keywords with ing, so instead of 'streaming' use 'stream', 'querying' use 'query'
- DO NOT use plural form of a word, so instead of 'queries' use 'query', 'functions' use 'function'
- ALWAYS call a function. DO NOT answer the question directly"#
            );
            system_prompt
        }
        None => {
            let system_prompt = format!(
                r#"
##### FILE PATH #####
{file_path}

Your job is to select keywords which should be used to search for relevant code snippets using lexical search for the file path `{file_path}`:

- You are given an outline of the code in the file, use the outline to select keywords
- ALWAYS call a function, DO NOT answer the question directly, even if the query is not in English
- When calling functions.code your query should consist of keywords. E.g. if the user says 'What does contextmanager do?', your query should be 'contextmanager'. If the user says 'How is contextmanager used in app', your query should be 'contextmanager app'. If the user says 'What is in the src directory', your query should be 'src'
- DO NOT end the keywords with ing, so instead of 'streaming' use 'stream', 'querying' use 'query'
- DO NOT use plural form of a word, so instead of 'queries' use 'query', 'functions' use 'function'
- ALWAYS call a function. DO NOT answer the question directly"#
            );
            system_prompt
        }
    }
}

pub fn system_search<'a>(paths: impl IntoIterator<Item = &'a str>) -> String {
    let mut system_prompt = "".to_string();

    let mut paths = paths.into_iter().peekable();

    if paths.peek().is_some() {
        system_prompt.push_str("## PATHS ##\nindex, path\n");
        for (i, path) in paths.enumerate() {
            system_prompt.push_str(&format!("{}, {}\n", i, path));
        }
        system_prompt.push('\n');
    }

    system_prompt.push_str(
        r#"Your job is to choose the best action. Call functions to find information that will help answer the user's query. Call functions.none when you have enough information to answer. Follow these rules at all times:

- ALWAYS call a function, DO NOT answer the question directly, even if the query is not in English
- DO NOT call a function that you've used before with the same arguments
- DO NOT assume the structure of the codebase, or the existence of files or folders
- Your queries to functions.code or functions.path should be significantly different to previous queries
- Call functions.none with paths that you are confident will help answer the user's query
- If the user query is general (e.g. 'What does this do?', 'What is this repo?') look for READMEs, documentation and entry points in the code (main files, index files, api files etc.)
- If the user is referring to, or asking for, information that is in your history, call functions.none
- If after attempting to gather information you are still unsure how to answer the query, call functions.none
- If the query is a greeting, or neither a question nor an instruction, call functions.none
- When calling functions.code your query should consist of keywords. E.g. if the user says 'What does contextmanager do?', your query should be 'contextmanager'. If the user says 'How is contextmanager used in app', your query should be 'contextmanager app'. If the user says 'What is in the src directory', your query should be 'src'
- When calling functions.path your query should be a single term (no whitespace). E.g. if the user says 'Where is the query parser?', your query should be 'parser'. If the users says 'What's in the auth dir?', your query should be 'auth'
- If the output of a function is empty, try calling the function again with DIFFERENT arguments OR try calling a different function
- Only call functions.proc with path indices that are under the PATHS heading above
- Call functions.proc with paths that might contain relevant information. Either because of the path name, or to expand on code that's been returned by functions.code
- ALWAYS call a function. DO NOT answer the question directly"#);
    system_prompt
}

pub fn system_sematic_search<'a>(paths: impl IntoIterator<Item = &'a str>) -> String {
    let mut system_prompt = "".to_string();

    let mut paths = paths.into_iter().peekable();

    if paths.peek().is_some() {
        system_prompt.push_str("## PATHS ##\nindex, path\n");
        for (i, path) in paths.enumerate() {
            system_prompt.push_str(&format!("{}, {}\n", i, path));
        }
        system_prompt.push('\n');
    }

    system_prompt.push_str(
        r#"Your job is to choose the best action. Call functions to find information that will help answer the user's query. Follow these rules at all times:

- ALWAYS call a function, DO NOT answer the question directly, even if the query is not in English
- DO NOT call a function that you've used before with the same arguments
- DO NOT assume the structure of the codebase, or the existence of files or folders
- Your queries to functions.code or functions.path should be significantly different to previous queries
- If the user query is general (e.g. 'What does this do?', 'What is this repo?') look for READMEs, documentation and entry points in the code (main files, index files, api files etc.)
- When calling functions.code your query should consist of keywords. E.g. if the user says 'What does contextmanager do?', your query should be 'contextmanager'. If the user says 'How is contextmanager used in app', your query should be 'contextmanager app'. If the user says 'What is in the src directory', your query should be 'src'
- When calling functions.path your query should be a single term (no whitespace). E.g. if the user says 'Where is the query parser?', your query should be 'parser'. If the users says 'What's in the auth dir?', your query should be 'auth'
- If the output of a function is empty, try calling the function again with DIFFERENT arguments OR try calling a different function
- ALWAYS call a function. DO NOT answer the question directly"#);
    system_prompt
}

pub fn hypothetical_document_prompt(query: &str) -> String {
    format!(
        r#"Write a code snippet that could hypothetically be returned by a code search engine as the answer to the query: {query}

- Write the snippets in a programming or markup language that is likely given the query
- The snippet should be between 5 and 10 lines long
- Surround the snippet in triple backticks

For example:

What's the Qdrant threshold?

```rust
SearchPoints {{
    limit,
    vector: vectors.get(idx).unwrap().clone(),
    collection_name: COLLECTION_NAME.to_string(),
    offset: Some(offset),
    score_threshold: Some(0.3),
    with_payload: Some(WithPayloadSelector {{
        selector_options: Some(with_payload_selector::SelectorOptions::Enable(true)),
    }}),
```"#
    )
}

pub fn try_parse_hypothetical_documents(document: &str) -> Vec<String> {
    let pattern = r"```([\s\S]*?)```";
    let re = regex::Regex::new(pattern).unwrap();

    re.captures_iter(document)
        .map(|m| m[1].trim().to_string())
        .collect()
}

pub fn file_explanation(question: &str, path: &str, code: &str) -> String {
    format!(
        r#"Below are some lines from the file /{path}. Each line is numbered.

#####

{code}

#####

Your job is to perform the following tasks:
1. Find all the relevant line ranges of code.
2. DO NOT cite line ranges that you are not given above
3. You MUST answer with only line ranges. DO NOT answer the question

Q: find Kafka auth keys
A: [[12,15]]

Q: find where we submit payment requests
A: [[37,50]]

Q: auth code expiration
A: [[486,501],[520,560],[590,631]]

Q: library matrix multiplication
A: [[68,74],[82,85],[103,107],[187,193]]

Q: how combine result streams
A: []

Q: {question}
A: "#
    )
}

pub fn answer_article_prompt(multi: bool, context: &str, location: &str) -> String {
    // Return different prompts depending on whether there is one or many aliases
    let one_prompt = format!(
        r#"{context}#####

A user is looking at the code above, your job is to answer their query.

Your output will be interpreted as codestory-markdown which renders with the following rules:
- Inline code must be expressed as a link to the correct line of code using the URL format: `[bar]({location}src/foo.rs#L50)` or `[bar]({location}src/foo.rs#L50-L54)`
- Do NOT output bare symbols. ALL symbols must include a link
  - E.g. Do not simply write `Bar`, write [`Bar`]({location}src/bar.rs#L100-L105).
  - E.g. Do not simply write "Foos are functions that create `Foo` values out of thin air." Instead, write: "Foos are functions that create [`Foo`]({location}src/foo.rs#L80-L120) values out of thin air."
- Only internal links to the current file work
- While generating code, do not leave any code partially generated
- Basic markdown text formatting rules are allowed

Here is an example response:

A function [`openCanOfBeans`]({location}src/beans/open.py#L7-L19) is defined. This function is used to handle the opening of beans. It includes the variable [`openCanOfBeans`]({location}src/beans/open.py#L9) which is used to store the value of the tin opener.
"#
    );

    let many_prompt = format!(
        r#"{context}####

Your job is to answer a query about a codebase using the information above.

Provide only as much information and code as is necessary to answer the query, but be concise. Keep number of quoted lines to a minimum when possible. If you do not have enough information needed to answer the query, do not make up an answer.
When referring to code, you must provide an example in a code block.

Respect these rules at all times:
- Link ALL paths AND code symbols (functions, methods, fields, classes, structs, types, variables, values, definitions, directories, etc) by embedding them in a markdown link, with the URL corresponding to the full path, and the anchor following the form `LX` or `LX-LY`, where X represents the starting line number, and Y represents the ending line number, if the reference is more than one line.
  - For example, to refer to lines 50 to 78 in a sentence, respond with something like: The compiler is initialized in [`src/foo.rs`]({location}src/foo.rs#L50-L78)
  - For example, to refer to the `new` function on a struct, respond with something like: The [`new`]({location}src/bar.rs#L26-53) function initializes the struct
  - For example, to refer to the `foo` field on a struct and link a single line, respond with something like: The [`foo`]({location}src/foo.rs#L138) field contains foos. Do not respond with something like [`foo`]({location}src/foo.rs#L138-L138)
  - For example, to refer to a folder `foo`, respond with something like: The files can be found in [`foo`]({location}path/to/foo/) folder
- Do not print out line numbers directly, only in a link
- Do not refer to more lines than necessary when creating a line range, be precise
- Do NOT output bare symbols. ALL symbols must include a link
  - E.g. Do not simply write `Bar`, write [`Bar`]({location}src/bar.rs#L100-L105).
  - E.g. Do not simply write "Foos are functions that create `Foo` values out of thin air." Instead, write: "Foos are functions that create [`Foo`]({location}src/foo.rs#L80-L120) values out of thin air."
- Link all fields
  - E.g. Do not simply write: "It has one main field: `foo`." Instead, write: "It has one main field: [`foo`]({location}src/foo.rs#L193)."
- Do NOT link external urls not present in the context, do NOT link urls from the internet
- Link all symbols, even when there are multiple in one sentence
  - E.g. Do not simply write: "Bars are [`Foo`]( that return a list filled with `Bar` variants." Instead, write: "Bars are functions that return a list filled with [`Bar`]({location}src/bar.rs#L38-L57) variants."
  - If you do not have enough information needed to answer the query, do not make up an answer. Instead respond only with a footnote that asks the user for more information, e.g. `assistant: I'm sorry, I couldn't find what you were looking for, could you provide more information?`
- While generating code, do not leave any code partially generated
- Code blocks MUST be displayed to the user using markdown"#
    );

    if multi {
        many_prompt
    } else {
        one_prompt
    }
}

pub fn explain_article_prompt(multi: bool, context: &str, location: &str) -> String {
    // Return different prompts depending on whether there is one or many aliases
    let one_prompt = format!(
        r#"{context}#####

Your job is to explain the selected code snippet to the user and they have provided in the query what kind of information they need.

Your output will be interpreted as codestory-markdown which renders with the following rules:
- Inline code must be expressed as a link to the correct line of code using the URL format: `[bar]({location}src/foo.rs#L50)` or `[bar]({location}src/foo.rs#L50-L54)`
- Do NOT output bare symbols. ALL symbols must include a link
  - E.g. Do not simply write `Bar`, write [`Bar`]({location}src/bar.rs#L100-L105).
  - E.g. Do not simply write "Foos are functions that create `Foo` values out of thin air." Instead, write: "Foos are functions that create [`Foo`]({location}src/foo.rs#L80-L120) values out of thin air."
- Only internal links to the current file work
- Basic markdown text formatting rules are allowed

Here is an example response:

A function [`openCanOfBeans`]({location}src/beans/open.py#L7-L19) is defined. This function is used to handle the opening of beans. It includes the variable [`openCanOfBeans`]({location}src/beans/open.py#L9) which is used to store the value of the tin opener.
"#
    );

    let many_prompt = format!(
        r#"{context}####

Your job is to explain the selected code snippet to the user and they have provided in the query what kind of information they need.

Provide only as much information and code as is necessary to answer the query, but be concise. Keep number of quoted lines to a minimum when possible. If you do not have enough information needed to answer the query, do not make up an answer.
When referring to code, you must provide an example in a code block.

Respect these rules at all times:
- Link ALL paths AND code symbols (functions, methods, fields, classes, structs, types, variables, values, definitions, directories, etc) by embedding them in a markdown link, with the URL corresponding to the full path, and the anchor following the form `LX` or `LX-LY`, where X represents the starting line number, and Y represents the ending line number, if the reference is more than one line.
  - For example, to refer to lines 50 to 78 in a sentence, respond with something like: The compiler is initialized in [`src/foo.rs`]({location}src/foo.rs#L50-L78)
  - For example, to refer to the `new` function on a struct, respond with something like: The [`new`]({location}src/bar.rs#L26-53) function initializes the struct
  - For example, to refer to the `foo` field on a struct and link a single line, respond with something like: The [`foo`]({location}src/foo.rs#L138) field contains foos. Do not respond with something like [`foo`]({location}src/foo.rs#L138-L138)
  - For example, to refer to a folder `foo`, respond with something like: The files can be found in [`foo`]({location}path/to/foo/) folder
- Do not print out line numbers directly, only in a link
- Do not refer to more lines than necessary when creating a line range, be precise
- Do NOT output bare symbols. ALL symbols must include a link
  - E.g. Do not simply write `Bar`, write [`Bar`]({location}src/bar.rs#L100-L105).
  - E.g. Do not simply write "Foos are functions that create `Foo` values out of thin air." Instead, write: "Foos are functions that create [`Foo`]({location}src/foo.rs#L80-L120) values out of thin air."
- Link all fields
  - E.g. Do not simply write: "It has one main field: `foo`." Instead, write: "It has one main field: [`foo`]({location}src/foo.rs#L193)."
- Do NOT link external urls not present in the context, do NOT link urls from the internet
- Link all symbols, even when there are multiple in one sentence
  - E.g. Do not simply write: "Bars are [`Foo`]( that return a list filled with `Bar` variants." Instead, write: "Bars are functions that return a list filled with [`Bar`]({location}src/bar.rs#L38-L57) variants."
  - If you do not have enough information needed to answer the query, do not make up an answer. Instead respond only with a footnote that asks the user for more information, e.g. `assistant: I'm sorry, I couldn't find what you were looking for, could you provide more information?`
- Code blocks MUST be displayed to the user using markdown"#
    );

    if multi {
        many_prompt
    } else {
        one_prompt
    }
}

pub fn followup_chat_prompt(
    context: &str,
    location: &str,
    is_followup: bool,
    user_context: bool,
    project_labels: &[String],
    system_instruction: Option<&str>,
) -> String {
    use std::collections::HashSet;
    let mut user_selected_instructions = "";
    let mut project_labels_context = "".to_owned();
    let user_system_instruction = if let Some(system_instruction) = system_instruction {
        format!(
            r#"The user has provided additional instructions for you to follow at all times:
{system_instruction}"#
        )
    } else {
        "".to_owned()
    };
    if !project_labels.is_empty() {
        let unique_project_labels = project_labels
            .iter()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .into_iter()
            .map(|value| format!("- {value}"))
            .collect::<Vec<_>>()
            .join("\n");
        project_labels_context = format!(
            r#"- You are given the following project labels which are associated with the codebase:
{unique_project_labels}
"#
        );
    }
    if user_context {
        user_selected_instructions = r#"- You are given the code which the user has selected explicitly in the USER SELECTED CODE section
- Pay special attention to the USER SELECTED CODE as these code snippets are specially selected by the user in their query
- If the user mentions #openFiles, note that the user is referring to the contents of the code they have selected explicitly. In such cases, assume the user has mentioned each of those files in the query and use the contents of those files to answer the query."#;
    }
    let not_followup_generate_question = format!(
        r#"{context}####
Your job is to answer the user query.
{user_system_instruction}

When referring to code, you must provide an example in a code block.

{project_labels_context}

Respect these rules at all times:
- When asked for your name, you must respond with "Aide".
- Follow the user's requirements carefully & to the letter.
- Minimize any other prose.
- Unless directed otherwise, the user is expecting for you to edit their selected code.
- Link ALL paths AND code symbols (functions, methods, fields, classes, structs, types, variables, values, definitions, directories, etc) by embedding them in a markdown link, with the URL corresponding to the full path, and the anchor following the form `LX` or `LX-LY`, where X represents the starting line number, and Y represents the ending line number, if the reference is more than one line.
    - For example, to refer to lines 50 to 78 in a sentence, respond with something like: The compiler is initialized in [`src/foo.rs`]({location}src/foo.rs#L50-L78)
    - For example, to refer to the `new` function on a struct, respond with something like: The [`new`]({location}src/bar.rs#L26-53) function initializes the struct
    - For example, to refer to the `foo` field on a struct and link a single line, respond with something like: The [`foo`]({location}src/foo.rs#L138) field contains foos. Do not respond with something like [`foo`]({location}src/foo.rs#L138-L138)
    - For example, to refer to a folder `foo`, respond with something like: The files can be found in [`foo`]({location}path/to/foo/) folder
- Do not print out line numbers directly, only in a link
- Do not refer to more lines than necessary when creating a line range, be precise
- Do NOT output bare symbols. ALL symbols must include a link
    - E.g. Do not simply write `Bar`, write [`Bar`]({location}src/bar.rs#L100-L105).
    - E.g. Do not simply write "Foos are functions that create `Foo` values out of thin air." Instead, write: "Foos are functions that create [`Foo`]({location}src/foo.rs#L80-L120) values out of thin air."
- Link all fields
    - E.g. Do not simply write: "It has one main field: `foo`." Instead, write: "It has one main field: [`foo`]({location}src/foo.rs#L193)."
- Do NOT link external urls not present in the context, do NOT link urls from the internet
- Link all symbols, even when there are multiple in one sentence
    - E.g. Do not simply write: "Bars are [`Foo`]( that return a list filled with `Bar` variants." Instead, write: "Bars are functions that return a list filled with [`Bar`]({location}src/bar.rs#L38-L57) variants."
- Code blocks MUST be displayed to the user using markdown
- Code blocks MUST be displayed to the user using markdown and must NEVER include the line numbers
- If you are going to not edit sections of the code, leave "// rest of code .." as the placeholder string
- Do NOT write the line number in the codeblock
    - E.g. Do not write:
    ```rust
    1. // rest of code ..
    2. // rest of code ..
    ```
    Here the codeblock has line numbers 1 and 2, do not write the line numbers in the codeblock
{user_selected_instructions}"#
    );
    let followup_prompt = format!(
        r#"{context}####

Your job is to answer the user query which is a followup to the conversation we have had.
{user_system_instruction}

Provide only as much information and code as is necessary to answer the query, but be concise. Keep number of quoted lines to a minimum when possible.
When referring to code, you must provide an example in a code block.

{project_labels_context}

Respect these rules at all times:
- When asked for your name, you must respond with "Aide".
- Follow the user's requirements carefully & to the letter.
- Minimize any other prose.
- Unless directed otherwise, the user is expecting for you to edit their selected code.
- Link ALL paths AND code symbols (functions, methods, fields, classes, structs, types, variables, values, definitions, directories, etc) by embedding them in a markdown link, with the URL corresponding to the full path, and the anchor following the form `LX` or `LX-LY`, where X represents the starting line number, and Y represents the ending line number, if the reference is more than one line.
    - For example, to refer to lines 50 to 78 in a sentence, respond with something like: The compiler is initialized in [`src/foo.rs`]({location}src/foo.rs#L50-L78)
    - For example, to refer to the `new` function on a struct, respond with something like: The [`new`]({location}src/bar.rs#L26-53) function initializes the struct
    - For example, to refer to the `foo` field on a struct and link a single line, respond with something like: The [`foo`]({location}src/foo.rs#L138) field contains foos. Do not respond with something like [`foo`]({location}src/foo.rs#L138-L138)
    - For example, to refer to a folder `foo`, respond with something like: The files can be found in [`foo`]({location}path/to/foo/) folder
- Do not print out line numbers directly, only in a link
- Do not refer to more lines than necessary when creating a line range, be precise
- Do NOT output bare symbols. ALL symbols must include a link
    - E.g. Do not simply write `Bar`, write [`Bar`]({location}src/bar.rs#L100-L105).
    - E.g. Do not simply write "Foos are functions that create `Foo` values out of thin air." Instead, write: "Foos are functions that create [`Foo`]({location}src/foo.rs#L80-L120) values out of thin air."
- Link all fields
    - E.g. Do not simply write: "It has one main field: `foo`." Instead, write: "It has one main field: [`foo`]({location}src/foo.rs#L193)."
- Do NOT link external urls not present in the context, do NOT link urls from the internet
- Link all symbols, even when there are multiple in one sentence
    - E.g. Do not simply write: "Bars are [`Foo`]( that return a list filled with `Bar` variants." Instead, write: "Bars are functions that return a list filled with [`Bar`]({location}src/bar.rs#L38-L57) variants."
- Code blocks MUST be displayed to the user using markdown
- Code blocks MUST be displayed to the user using markdown and must NEVER include the line numbers
- If you are going to not edit sections of the code, leave "// rest of code .." as the placeholder string.
- Do NOT write the line number in the codeblock
    - E.g. Do not write:
    ```rust
    1. // rest of code ..
    2. // rest of code ..
    ```
    Here the codeblock has line numbers 1 and 2, do not write the line numbers in the codeblock
{user_selected_instructions}"#
    );

    if is_followup {
        followup_prompt
    } else {
        not_followup_generate_question
    }
}

pub fn extract_goto_definition_symbols_from_snippet(language: &str) -> String {
    let system_prompt = format!(
        r#"
    Your job is to help the user understand a code snippet completely. You will be shown a code snippet in {language} and you have output a comma separated list of symbols for which we need to get the go-to-definition value.

    Respect these rules at all times:
    - Do not ask for go-to-definition for symbols which are common to {language}.
    - Do not ask for go-to-definition for symbols which are not present in the code snippet.
    - You should always output the list of symbols in a comma separated list.

    An example is given below for you to follow:
    ###
    ```typescript
    const limiter = createLimiter(
        // The concurrent requests limit is chosen very conservatively to avoid blocking the language
        // server.
        2,
        // If any language server API takes more than 2 seconds to answer, we should cancel the request
        5000
    );
    
    
    // This is the main function which gives us context about what's present on the
    // current view port of the user, this is important to get right
    export const getLSPGraphContextForChat = async (workingDirectory: string, repoRef: RepoRef): Promise<DeepContextForView> => {{
        const activeEditor = vscode.window.activeTextEditor;
    
        if (activeEditor === undefined) {{
            return {{
                repoRef: repoRef.getRepresentation(),
                preciseContext: [],
                cursorPosition: null,
                currentViewPort: null,
            }};
        }}
    
        const label = 'getLSPGraphContextForChat';
        performance.mark(label);
    
        const uri = URI.file(activeEditor.document.fileName);
    ```
    Your response: createLimiter, RepoRef, DeepContextForView, activeTextEditor, performance, file
    ###

    Another example:
    ###
    ```rust
    let mut previous_messages =
        ConversationMessage::load_from_db(app.sql.clone(), &repo_ref, thread_id)
            .await
            .expect("loading from db to never fail");

    let snippet = file_content
        .lines()
        .skip(start_line.try_into().expect("conversion_should_not_fail"))
        .take(
            (end_line - start_line)
                .try_into()
                .expect("conversion_should_not_fail"),
        )
        .collect::<Vec<_>>()
        .join("\n");

    let mut conversation_message = ConversationMessage::explain_message(
        thread_id,
        crate::agent::types::AgentState::Explain,
        query,
    );

    let code_span = CodeSpan {{
        file_path: relative_path.to_owned(),
        alias: 0,
        start_line,
        end_line,
        data: snippet,
        score: Some(1.0),
    }};
    conversation_message.add_user_selected_code_span(code_span.clone());
    conversation_message.add_code_spans(code_span.clone());
    conversation_message.add_path(relative_path);

    previous_messages.push(conversation_message);
    ```
    Your response: ConversationMessage, load_from_db, sql, repo_ref, thread_id, file_content, explain_message, AgentState, Explain, CodeSpan, add_user_selected_code_span, add_code_spans, add_path
    ###
    "#
    );
    system_prompt
}

pub fn definition_snippet_required(
    view_port_snippet: &str,
    definition_snippet: &str,
    query: &str,
) -> String {
    let system_prompt = format!(
        r#"
Below is a code snippet which the user is looking at. We can also see the code selection of the user which is indicated in the code snippet below by the start of <cursor_position> and ends with </cursor_position>. The cursor position might be of interest to you as that's where the user was when they were last navigating the file.

### CODE SNIPPET IN EDITOR ###
{view_port_snippet}    
###

You are also given a code snippet of the definition of some code symbols below this section is called the DEFINITION SNIPPET
### DEFINITION SNIPPET ###
{definition_snippet}
###

Your job is to perform the following tasks on the DEFINITION SNIPPET:
1. Find all the relevant line ranges from the DEFINITION SNIPPET and only from DEFINITION SNIPPET section which is necessary to answer the user question given the CODE SNIPPET IN THE EDITOR
2. DO NOT cite line ranges that you are not given above and which are not in the DEFINITION SNIPPET
3. DO NOT cite line ranges from the CODE SNIPPET IN THE EDITOR which the user is looking at.
3. You MUST answer with only YES or NO, if the DEFINITION SNIPPET is relevant to the user question.

Q: {query}
A:"#
    );
    system_prompt
}

pub fn code_snippet_important(
    location: &str,
    snippet: &str,
    language: &str,
    query: &str,
) -> String {
    let system_prompt = format!(
        r#"You will be asked to decide if the code snippet is relevant to the user query, reply with Yes or No
User query:
{query}

Code Snippet:
Location: {location}
```{language}
{snippet}
```
    
    "#
    );
    system_prompt
}

pub fn diff_accept_prompt(user_query: &str) -> String {
    let system_prompt = format!(
        r#"You are an expert at understanding and performing git commit changes action. Another junior engineer is working on changes to a current section of the code,  the engineer has generated code and this is the reasoning: "{user_query}"

You have 3 actions to use:
- Accept Current Change you can use this to keep the edit as it is
- Accept Incoming Change use this to accept the incoming changes from the code generated by the large language model
- Accept Both Changes use this to accept both the current code and the large language model generated code

Points to remember:
- You will be shown a merge conflict between the code which is present and the code written by the junior engineer
- You are also shown parts of the prefix of the merge conflict and the suffix of the merge conflict. The suffix of the merge conflict might have conflicts but YOU SHOULD NOT PAY ATTENTION TO THEM.
- You are only given a small section of the git diff, so no matter which option you select and even if the code is incomplete we will be okay with selecting either of the options
- The user will also tell you how the code will look like after doing the 3 operations we mentioned above, use that to understand the impact of your choice and select the best one based on the user query
- The junior engineer might generate code which is not exactly correct and tends to delete chunks of code which are required, so pay attention to what code the changes made by junior engineer will replace
- The junior engineer changes might end up deleting important part of the code, which is not correct
- If you select Accept Current Changes, then the changes done by the junior engineer are not applied
- <deleted_code>{{code}}</deleted_code> that means the code in between has been deleted and will not be part of the final output
- <added_code>{{code}}</added_code> that means the code in between will be added and will be part of the final output
- <suffix_code>{{code}}</suffix_code> that means the code in between will be part of the code after the merge conflict, these markers are not part of the code but are shown to you to understand how the code will be placed
- This junior engineer is forgetful so sometimes they make mistakes. It might be the case that the changes which are done can delete part of the code, so keep that in mind when selecting the 3 options below, if the change made by the user junior engineer is an indication of leaving the code as it, for example: "// ...", "// ... existing code", "// rest of the code ..." or other variants of this, just "accept current change"

As an example if the code looks like:
```
def add_nums(a: int, b: int) -> int:<deleted_code>
    print(a, b)
</deleted_code>
    return a + b
```
the final code will be:
```
def add_nums(a: int, b: int) -> int:
    return a + b
```
the line between <deleted_code>`print(a, b)`</deleted_code>  is removed from the code snippet

Another example:
```
def add_nums(a: int, b: int) -> int:
<added_code>
    print(a ,b)
</added_code>
    return a + b
```
the final code will be:
```
def add_nums(a: int, b: int) -> int:
    print(a, b)
    return a + b
```
the line between <added_code>`print(a, b)`</added_code> is added to the code snippet"#
    );
    system_prompt
}

pub fn diff_user_messages(
    prefix_code: &str,
    current_changes: &str,
    incoming_changes: &str,
    suffix_code: &str,
) -> Vec<String> {
    let git_patch = format!(
        r#"The git patch
```
<previous_code>
{prefix_code}
</previous_code>
<<<<<<<
{current_changes}
=======
{incoming_changes}
>>>>>>> JUNIOR ENGINEER CODE
<suffix_code>
{suffix_code}
</suffix_code>
```"#
    );
    let accept_current_changes = format!(
        r#"Code after 'Accept Current Change'
```
<previous_code>
{prefix_code}
</previous_code>
{current_changes}
<suffix_code>
{suffix_code}
</suffix_code>
```"#
    );
    let accept_incoming_changes = format!(
        r#"Code after 'Accept Incoming Change
```
<previous_code>
{prefix_code}
</previous_code>
<deleted_code_by_junior_engineer>
{current_changes}
</deleted_code_by_junior_engineer>
<added_code_by_junior_engineer>
{incoming_changes}
</added_code_by_junior_engineer>
<suffix_code>
{suffix_code}
</suffix_code>
```"#
    );
    let accept_both_changes = format!(
        r#"Code after 'Accept Both Changes'
```
<previous_code>
{prefix_code}
</previous_code>
{current_changes}
<added_code_by_junior_engineer>
{incoming_changes}
</added_code_by_junior_engineer>
<suffix_code>
{suffix_code}
</suffix_code>
```"#
    );
    let user_action = format!(
        r#"Chose one of 'Accept Current Change', 'Accept Incoming Change', 'Accept Both Changes'.
First give your reasoning and then given your selected option in in the format:
        
<answer>{{your answer}}</answer>"#
    );
    vec![
        git_patch,
        accept_current_changes,
        accept_incoming_changes,
        accept_both_changes,
        user_action,
    ]
}

pub fn system_prompt_for_git_patch(
    message: &str,
    language: &str,
    symbol_name: &str,
    symbol_type: &str,
) -> String {
    let system_prompt = format!(
        r#"You are an expert at applying the git patch to the given code snippet for {symbol_name} {symbol_type}.

Your job is to generate the updated {symbol_name} {symbol_type} after applying the git patch.

The reason for applying the git patch is given below, a junior engineer has generated the code and the junior engineer was lazy and sometimes forgets to write code, your job is to generate the final code symbol.

Reason:
<reason>
{message}
</reason>

You have to look at the reason and follow these rules:
- Then output the code in a single code block.
- Minimize any other prose.
- Each code block starts with ``` and // FILEPATH.
- If you suggest to run a terminal command, use a code block that starts with ```bash.
- You always answer with {language} code.
- Modify the code or create new code.
- Unless directed otherwise, the user is expecting for you to edit their selected code.
- Make sure to ALWAYS INCLUDE the BEGIN and END markers in your generated code with // BEGIN and then // END which is present in the code selection given by the user"#
    );
    system_prompt.to_owned()
}

pub fn user_message_for_git_patch(
    language: &str,
    symbol_name: &str,
    symbol_type: &str,
    git_diff: &str,
    file_path: &str,
    symbol_content: &str,
) -> Vec<String> {
    let git_diff_patch = format!(
        r#"git patch for {symbol_name} {symbol_type}:
```{language}
{git_diff}
```"#
    );
    let original_symbol = format!(
        r#"{symbol_name} {symbol_type}
```{language}
// FILEPATH: {file_path}
// BEGIN: be15d9bcejpp
{symbol_content}
// END: be15d9bcejpp
```"#
    );
    let additional_prompt = format!(
        r#"Do not forget to include the // BEGIN and // END markers in your generated code.
ONLY WRITE the code for the "{symbol_name} {symbol_type}"."#
    );
    vec![
        git_diff_patch.to_owned(),
        original_symbol.to_owned(),
        additional_prompt.to_owned(),
    ]
}
