use crate::agentic::symbol::{events::edit::SymbolToEdit, identifier::SymbolIdentifier};

pub type OriginalContent = String;
pub type UpdatedContent = String;
pub type Changes = Vec<(SymbolToEdit, OriginalContent, UpdatedContent)>;

#[derive(Debug, Clone)]
pub struct SymbolChanges {
    symbol_identifier: SymbolIdentifier,
    changes: Changes,
}

impl SymbolChanges {
    pub fn new(symbol_identifier: SymbolIdentifier, changes: Changes) -> Self {
        Self {
            symbol_identifier,
            changes,
        }
    }

    pub fn add_change(
        &mut self,
        edit: SymbolToEdit,
        original_content: OriginalContent,
        updated_content: UpdatedContent,
    ) {
        self.changes.push((edit, original_content, updated_content));
    }

    pub fn symbol_identifier(&self) -> &SymbolIdentifier {
        &self.symbol_identifier
    }

    pub fn changes(&self) -> &Changes {
        &self.changes
    }

    pub fn remove_changes(self) -> Changes {
        self.changes
    }
}

#[derive(Debug, Clone)]
pub struct SymbolChangeSet {
    changes: Vec<SymbolChanges>,
}

impl Default for SymbolChangeSet {
    fn default() -> Self {
        Self {
            changes: Vec::new(),
        }
    }
}

impl SymbolChangeSet {
    pub fn new(changes: Vec<SymbolChanges>) -> Self {
        Self { changes }
    }

    pub fn changes(&self) -> &[SymbolChanges] {
        &self.changes
    }

    pub fn remove_changes(self) -> Vec<SymbolChanges> {
        self.changes
    }
}
