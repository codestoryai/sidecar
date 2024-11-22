use llm_client::{
    clients::{
        gemini_pro::GeminiProClient,
        types::{LLMClient, LLMClientCompletionRequest, LLMClientMessage, LLMType},
    },
    provider::{GoogleAIStudioKey, LLMProviderAPIKeys},
};

#[tokio::main]
async fn main() {
    let api_key = LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new("".to_owned()));
    let _request = LLMClientCompletionRequest::from_messages(
        vec![
            LLMClientMessage::system("You are an expert software engineer".to_owned()),
            LLMClientMessage::user(
                "Help me write a function in rust which adds 2 numbers".to_owned(),
            ),
        ],
        LLMType::GeminiProFlash,
    );
    let gemini_pro_clint = GeminiProClient::new();
    let context = std::fs::read_to_string("/tmp/repo_map").expect("to work");
    let user_query = r#"delete() on instances of models without any dependencies doesn't clear PKs.
Description
    
Deleting any model with no dependencies not updates the PK on the model. It should be set to None after .delete() call.
See Django.db.models.deletion:276-281. Should update the model line 280."#.to_owned();
    let _result = gemini_pro_clint
        .count_tokens(&context, "anton-390822", "ya29.a0AXooCgs8y42lwdXpYkBXCiReRXBjVnvfkdnGA3JadAVraH6sGw_hqyOPVy0R-eSsSElaPKAI9OjQGDG9S0i4fFfaUtKBaF9qXaRQbUTIwIwbJX0T2yciqEJTPHbV2YQe4jrZwLs2rifas3FsCu3eIW5wfMLKGiutdJA6QKVs7QaCgYKAccSARESFQHGX2MiGybA9H-ZXPMSK5Pvz3XuoA0177", "gemini-1.5-flash-001")
        .await;

    let user_message = format!(
        r#"<code_selection>
{context}
</code_selection>
<user_query>
{user_query}
</user_query>"#
    )
    .to_owned();

    // Request for the LLM to search using the repo map
    let system_message = r#"You are a search engine which makes no mistakes while retriving important context for a user-query.
You will be given context which the user has selected in <user_context> and you have to retrive the "code symbols" which are important for answering to the user query.
- The user might have selected some context manually in the form of <selection> these might be more important
- You will be given files which contains a lot of code, you have to select the "code symbols" which are important
- "code symbols" here referes to the different classes, functions, or constants which might be necessary to answer the user query.
- Now you will write a step by step process for making the code edit, this ensures that you lay down the plan before making the change, put this in an xml section called <step_by_step> where each step is in <step_list> section where each section has the name of the symbol on which the operation will happen, if no such symbol exists and you need to create a new one put a <new>true</new> inside the step section and after the symbols
- In your step by step list make sure that the symbols are listed in the order in which we have to go about making the changes
- Strictly follow the reply format which is mentioned to you below, your reply should always start with <reply> tag and end with </reply> tag

Let's focus on getting the "code symbols" which are necessary to satisfy the user query.

As an example, given the following code selection:
<code_selection>
<file_path>
sidecar/broker/fill_in_middle.rs
</file_path>
```rust
pub struct FillInMiddleBroker {{
    providers: HashMap<LLMType, Box<dyn FillInMiddleFormatter + Send + Sync>>,
}}

impl FillInMiddleBroker {{
    pub fn new() -> Self {{
        let broker = Self {{
            providers: HashMap::new(),
        }};
        broker
            .add_llm(
                LLMType::CodeLlama13BInstruct,
                Box::new(CodeLlamaFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::CodeLlama7BInstruct,
                Box::new(CodeLlamaFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::DeepSeekCoder1_3BInstruct,
                Box::new(DeepSeekFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::DeepSeekCoder6BInstruct,
                Box::new(DeepSeekFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::DeepSeekCoder33BInstruct,
                Box::new(DeepSeekFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::ClaudeHaiku,
                Box::new(ClaudeFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::ClaudeOpus,
                Box::new(ClaudeFillInMiddleFormatter::new()),
            )
            .add_llm(
                LLMType::ClaudeSonnet,
                Box::new(ClaudeFillInMiddleFormatter::new()),
            )
    }}
```
</code_selection>

and the user query is:
<user_query>
I want to add support for the grok llm
</user_query>

Your reply should be, you should strictly follow this format:
<reply>
<symbol_list>
<symbol>
<name>
LLMType
</name>
<file_path>
sidecar/broker/fill_in_middle.rs
</file_path>
<thinking>
We need to first check if grok is part of the LLMType enum, this will make sure that the code we produce is never wrong
</thinking>
</symbol>
<symbol>
<name>
FillInMiddleFormatter
</name>
<file_path>
sidecar/broker/fill_in_middle.rs
</file_path>
<thinking>
Other LLM's are implementing FillInMiddleFormatter trait, grok will also require support for this, so we need to check how to implement FillInMiddleFormatter trait
</thinking>
</symbol>
<symbol>
<name>
new
</name>
<file_path>
sidecar/broker/fill_in_middle.rs
</file_path>
<thinking>
We have to change the new function and add the grok llm after implementing the formatter for grok llm.
</thinking>
</symbol>
</symbol_list>
<step_by_step>
<step_list>
<name>
LLMType
</name>
<file_path>
sidecar/broker/fill_in_middle.rs
</file_path>
<step>
We will need to first check the LLMType if it has support for grok or we need to edit it first
</step>
</step_list>
<step_list>
<name>
FillInMiddleFormatter
</name>
<file_path>
sidecar/broker/fill_in_middle.rs
</file_path>
<step>
Check the definition of `FillInMiddleFormatter` to see how to implement it
</step>
</step_list>
<step_list
<name>
CodeLlamaFillInMiddleFormatter
</name>
<file_path>
sidecar/broker/fill_in_middle.rs
</file_path>
<step>
We can follow the implementation of CodeLlamaFillInMiddleFormatter since we will also have to follow a similar pattern of making changes and adding it to the right places if there are more.
</step>
</step_list>
<step_list>
<name>
GrokFillInMiddleFormatter
</name>
<file_path>
sidecar/broker/fill_in_middle.rs
</file_path>
<new>
true
</new>
<step>
Implement the GrokFillInMiddleFormatter following the similar pattern in `CodeLlamaFillInMiddleFormatter`
</step>
</step_list>
</step_by_step>
</reply>

Another example:
<code_selection>
```rust
fn tree_sitter_router() -> Router {{
    use axum::routing::*;
    Router::new()
        .route(
            "/documentation_parsing",
            post(sidecar::webserver::tree_sitter::extract_documentation_strings),
        )
        .route(
            "/diagnostic_parsing",
            post(sidecar::webserver::tree_sitter::extract_diagnostics_range),
        )
        .route(
            "/tree_sitter_valid",
            post(sidecar::webserver::tree_sitter::tree_sitter_node_check),
        )
}}

fn file_operations_router() -> Router {{
    use axum::routing::*;
    Router::new().route("/edit_file", post(sidecar::webserver::file_edit::file_edit))
}}

fn inline_completion() -> Router {{
    use axum::routing::*;
    Router::new()
        .route(
            "/inline_completion",
            post(sidecar::webserver::inline_completion::inline_completion),
        )
        .route(
            "/cancel_inline_completion",
            post(sidecar::webserver::inline_completion::cancel_inline_completion),
        )
        .route(
            "/document_open",
            post(sidecar::webserver::inline_completion::inline_document_open),
        )
        .route(
            "/document_content_changed",
            post(sidecar::webserver::inline_completion::inline_completion_file_content_change),
        )
        .route(
            "/get_document_content",
            post(sidecar::webserver::inline_completion::inline_completion_file_content),
        )
        .route(
            "/get_identifier_nodes",
            post(sidecar::webserver::inline_completion::get_identifier_nodes),
        )
        .route(
            "/get_symbol_history",
            post(sidecar::webserver::inline_completion::symbol_history),
        )
}}

// TODO(skcd): Figure out why we are passing the context in the suffix and not the prefix

```
</code_selection>

and the user query is:
<user_query>
I want to get the list of most important symbols in inline completions
</user_query>

Your reply should be:
<reply>
<symbol_list>
<symbol>
<name>
inline_completion
</name>
<thinking>
inline_completion holds all the endpoints for symbols because it also has the `get_symbol_history` endpoint. We have to start adding the endpoint there
</thinking>
</symbol>
<symbol>
<name>
symbol_history
</name>
<thinking>
I can find more information on how to write the code for the endpoint by following the symbol `symbol_history` in the line: `             post(sidecar::webserver::inline_completion::symbol_history),`
<thinking>
</symbol>
</symbol_list>
<step_by_step>
<step_list>
<name>
symbol_history
</name>
<thinking>
We need to follow the symbol_history to check the pattern on how we are going to implement the very similar functionality
</thinking>
</step_list>
<step_list>
<name>
inline_completion
</name>
<thinking>
We have to add the newly created endpoint in inline_completion to add support for the new endpoint which we want to create
</thinking>
</step_list>
</step_by_step>
</reply>"#.to_owned();
    let request = LLMClientCompletionRequest::new(
        LLMType::GeminiProFlash,
        vec![
            LLMClientMessage::system(system_message),
            LLMClientMessage::user(user_message),
        ],
        0.2,
        None,
    );
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let response = gemini_pro_clint
        .stream_completion(api_key, request, sender)
        .await;
    println!("{:?}", &response);
    // println!("{:?}", &result);
}
