//! We are going to send a probing request over here
//! to ask for more questions

use crate::{agentic::symbol::identifier::SymbolIdentifier, chunking::text_document::Range};

#[derive(Debug, Clone)]
pub enum ProbeEnoughOrDeeperResponseParsed {
    AnswerUserQuery(String),
    ProbeDeeperInSubSymbols(Vec<SubSymbolToProbe>),
}

impl ProbeEnoughOrDeeperResponseParsed {
    pub fn answer_user_query(&self) -> Option<String> {
        match self {
            Self::AnswerUserQuery(response) => Some(response.to_owned()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SubSymbolToProbe {
    symbol: String,
    range: Range,
    fs_file_path: String,
    reason: String,
    is_outline: bool,
}

impl SubSymbolToProbe {
    pub fn new(
        symbol: String,
        range: Range,
        fs_file_path: String,
        reason: String,
        is_outline: bool,
    ) -> Self {
        Self {
            symbol,
            range,
            fs_file_path,
            reason,
            is_outline,
        }
    }

    pub fn symbol_name(&self) -> &str {
        &self.symbol
    }

    pub fn is_outline(&self) -> bool {
        self.is_outline
    }

    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolToProbeHistory {
    symbol: String,
    fs_file_path: String,
    content: String,
    question: String,
}

impl SymbolToProbeHistory {
    pub fn new(symbol: String, fs_file_path: String, content: String, question: String) -> Self {
        Self {
            symbol,
            fs_file_path,
            content,
            question,
        }
    }

    pub fn fs_file_path(&self) -> &str {
        &self.fs_file_path
    }

    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    pub fn question(&self) -> &str {
        &self.question
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolToProbeRequest {
    symbol_identifier: SymbolIdentifier,
    probe_request: String,
    original_request: String,
    original_request_id: String,
    history: Vec<SymbolToProbeHistory>,
}

impl SymbolToProbeRequest {
    pub fn new(
        symbol_identifier: SymbolIdentifier,
        probe_request: String,
        original_request: String,
        original_request_id: String,
        history: Vec<SymbolToProbeHistory>,
    ) -> Self {
        Self {
            symbol_identifier,
            probe_request,
            original_request,
            original_request_id,
            history,
        }
    }

    pub fn symbol_identifier(&self) -> &SymbolIdentifier {
        &self.symbol_identifier
    }

    pub fn original_request_id(&self) -> &str {
        &self.original_request_id
    }

    pub fn original_request(&self) -> &str {
        &self.original_request
    }

    pub fn probe_request(&self) -> &str {
        &self.probe_request
    }

    pub fn history_slice(&self) -> &[SymbolToProbeHistory] {
        self.history.as_slice()
    }

    pub fn history(&self) -> String {
        self.history
            .iter()
            .map(|history| {
                let symbol = &history.symbol;
                let file_path = &history.fs_file_path;
                let content = &history.content;
                let question = &history.question;
                format!(
                    r#"<item>
<symbol>
{symbol}
</symbol>
<file_path>
{file_path}
</file_path>
<content>
{content}
</content>
<question>
{question}
</question>
</item>"#
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
