use super::identifier::SymbolIdentifier;
use crate::chunking::text_document::Range;

#[derive(Debug, Clone)]
pub struct AnchoredSymbol {
    identifier: SymbolIdentifier,
    content: String,
    sub_symbol_names: Vec<String>,
    // we hae a possible range location over here
    // related to where the symbol could be present
    possible_range: Range,
}

impl AnchoredSymbol {
    pub fn new(
        identifier: SymbolIdentifier,
        content: &str,
        sub_symbol_names: &[String],
        possible_range: Range,
    ) -> Self {
        Self {
            identifier,
            content: content.to_string(),
            sub_symbol_names: sub_symbol_names.to_vec(),
            possible_range,
        }
    }

    pub fn possible_range(&self) -> &Range {
        &self.possible_range
    }

    pub fn name(&self) -> &str {
        &self.identifier.symbol_name()
    }

    pub fn fs_file_path(&self) -> Option<String> {
        self.identifier.fs_file_path()
    }

    pub fn identifier(&self) -> &SymbolIdentifier {
        &self.identifier
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn sub_symbol_names(&self) -> &[String] {
        &self.sub_symbol_names
    }
}
