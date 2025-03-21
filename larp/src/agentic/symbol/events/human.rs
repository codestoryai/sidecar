//! Contains the different kind of messages which are coming from the human

use crate::{agentic::symbol::anchored::AnchoredSymbol, user_context::types::UserContext};

#[derive(Debug, Clone)]
pub struct HumanAnchorRequest {
    query: String,
    anchored_symbols: Vec<AnchoredSymbol>,
    anchor_request_context: Option<String>,
}

impl HumanAnchorRequest {
    pub fn new(
        query: String,
        anchored_symbols: Vec<AnchoredSymbol>,
        anchor_request_context: Option<String>,
    ) -> Self {
        Self {
            query,
            anchored_symbols,
            anchor_request_context,
        }
    }

    pub fn anchored_symbols(&self) -> &[AnchoredSymbol] {
        self.anchored_symbols.as_slice()
    }

    pub fn user_query(&self) -> &str {
        &self.query
    }

    pub fn anchor_request_context(&self) -> Option<String> {
        self.anchor_request_context.clone()
    }

    pub fn to_string(&self) -> String {
        let query = &self.query;
        let anchored_symbols_to_string = self
            .anchored_symbols
            .iter()
            .map(|anchor_symbol| {
                let name = anchor_symbol.name();
                let fs_file_path = anchor_symbol.fs_file_path().unwrap_or_default();
                format!("{name} at {fs_file_path}")
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "developer request:{query}
edited: {anchored_symbols_to_string}"
        )
    }
}

#[derive(Debug, Clone)]
pub struct HumanAgenticRequest {
    user_query: String,
    root_directory: String,
    codebase_search: bool,
    user_context: UserContext,
    aide_rules: Option<String>,
    deep_reasoning: bool,
}

impl HumanAgenticRequest {
    pub fn new(
        user_query: String,
        root_directory: String,
        codebase_search: bool,
        user_context: UserContext,
        aide_rules: Option<String>,
        deep_reasoning: bool,
    ) -> Self {
        Self {
            user_context,
            user_query,
            root_directory,
            codebase_search,
            aide_rules,
            deep_reasoning,
        }
    }
    pub fn user_context(&self) -> UserContext {
        self.user_context.clone()
    }

    pub fn aide_rules(&self) -> Option<String> {
        self.aide_rules.clone()
    }

    pub fn user_query(&self) -> &str {
        &self.user_query
    }

    pub fn root_directory(&self) -> &str {
        &self.root_directory
    }

    pub fn codebase_search(&self) -> bool {
        self.codebase_search
    }

    pub fn deep_reasoning(&self) -> bool {
        self.deep_reasoning
    }
}

#[derive(Debug)]
pub enum HumanMessage {
    Followup(String),
    Anchor(HumanAnchorRequest),
    Agentic(HumanAgenticRequest),
}
