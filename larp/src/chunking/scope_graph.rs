use std::collections::HashMap;
use std::ops::Range as OpsRange;
use std::str::FromStr;
use std::sync::Arc;

use petgraph::Graph;
use petgraph::{visit::EdgeRef, Direction};
use serde::Deserialize;
use serde::Serialize;
use smallvec::smallvec;
use smallvec::SmallVec;
use tracing::warn;
use tree_sitter::QueryCursor;

use super::languages::{TSLanguageConfig, TSLanguageParsing};
use super::navigation::Snippet;
use super::text_document::Range;

pub type NodeIndex = petgraph::graph::NodeIndex<u32>;

/// Collection of symbol locations for *single* file
#[derive(Default, Debug, Clone, Deserialize, Serialize)]
#[non_exhaustive]
pub enum SymbolLocations {
    /// tree-sitter powered symbol-locations (and more!)
    TreeSitter(ScopeGraph),

    /// no symbol-locations for this file
    #[default]
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub kind: String,
    pub range: Range,
}

impl SymbolLocations {
    pub fn list(&self, language_parsing: Arc<TSLanguageParsing>) -> Vec<Symbol> {
        match self {
            Self::TreeSitter(graph) => graph.symbols(language_parsing),
            Self::Empty => Vec::new(),
        }
    }

    pub fn scope_graph(&self) -> Option<&ScopeGraph> {
        match self {
            Self::TreeSitter(graph) => Some(graph),
            Self::Empty => None,
        }
    }
}

/// An opaque identifier for every symbol in a language
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SymbolId {
    pub namespace_idx: usize,
    pub symbol_idx: usize,
}

impl SymbolId {
    pub fn name(&self, namespaces: NameSpaces) -> String {
        namespaces[self.namespace_idx][self.symbol_idx].to_owned()
    }
}

/// A grouping of symbol kinds that allow references among them.
/// A variable can refer only to other variables, and not types, for example.
pub type NameSpace = Vec<String>;

/// A collection of namespaces
pub type NameSpaces = Vec<Vec<String>>;

/// Helper trait
pub trait NameSpaceMethods {
    fn all_symbols(self) -> Vec<String>;

    fn symbol_id_of(&self, symbol: &str) -> Option<SymbolId>;
}

impl NameSpaceMethods for NameSpaces {
    fn all_symbols(self) -> Vec<String> {
        self.iter().flat_map(|ns| ns.iter().cloned()).collect()
    }

    fn symbol_id_of(&self, symbol: &str) -> Option<SymbolId> {
        self.iter()
            .enumerate()
            .find_map(|(namespace_idx, namespace)| {
                namespace
                    .iter()
                    .position(|s| s == &symbol)
                    .map(|symbol_idx| SymbolId {
                        namespace_idx,
                        symbol_idx,
                    })
            })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Hash, Eq)]
pub struct LocalScope {
    pub range: Range,
}

impl LocalScope {
    pub fn new(range: Range) -> Self {
        Self { range }
    }
}

/// Describes the relation between two nodes in the ScopeGraph
#[derive(Serialize, Deserialize, PartialEq, Eq, Copy, Clone, Debug)]
pub enum EdgeKind {
    /// The edge weight from a nested scope to its parent scope
    ScopeToScope,

    /// The edge weight from a definition to its definition scope
    DefToScope,

    /// The edge weight from an import to its definition scope
    ImportToScope,

    /// The edge weight from a reference to its definition
    RefToDef,

    /// The edge weight from a reference to its import
    RefToImport,
}

pub struct ScopeStack<'a> {
    pub scope_graph: &'a ScopeGraph,
    pub start: Option<NodeIndex>,
}

impl<'a> Iterator for ScopeStack<'a> {
    type Item = NodeIndex;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(start) = self.start {
            let parent = self
                .scope_graph
                .graph
                .edges_directed(start, Direction::Outgoing)
                .find(|edge| *edge.weight() == EdgeKind::ScopeToScope)
                .map(|edge| edge.target());
            let original = start;
            self.start = parent;
            Some(original)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct LocalDef {
    pub range: Range,
    pub symbol_id: Option<SymbolId>,
    pub local_scope: LocalScope,
}

impl LocalDef {
    /// Initialize a new definition
    pub fn new(range: Range, symbol_id: Option<SymbolId>, local_scope: LocalScope) -> Self {
        Self {
            range,
            symbol_id,
            local_scope,
        }
    }

    pub fn name<'a>(&self, buffer: &'a [u8]) -> &'a [u8] {
        &buffer[self.range.start_byte()..self.range.end_byte()]
    }

    pub fn range<'a>(&'a self) -> &'a Range {
        &self.local_scope.range
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct LocalImport {
    pub range: Range,
}

impl LocalImport {
    /// Initialize a new import
    pub fn new(range: Range) -> Self {
        Self { range }
    }

    pub fn name<'a>(&self, buffer: &'a [u8]) -> &'a [u8] {
        &buffer[self.range.start_byte()..self.range.end_byte()]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    pub range: Range,
    pub symbol_id: Option<SymbolId>,
}

impl Reference {
    /// Initialize a new reference
    pub fn new(range: Range, symbol_id: Option<SymbolId>) -> Self {
        Self { range, symbol_id }
    }

    pub fn name<'a>(&self, buffer: &'a [u8]) -> &'a [u8] {
        &buffer[self.range.start_byte()..self.range.end_byte()]
    }
}

/// The type of a node in the ScopeGraph
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum NodeKind {
    /// A scope node
    Scope(LocalScope),

    /// A definition node
    Def(LocalDef),

    /// An import node
    Import(LocalImport),

    /// A reference node
    Ref(Reference),
}

impl NodeKind {
    /// Construct a scope node from a range
    pub fn scope(range: Range) -> Self {
        Self::Scope(LocalScope::new(range))
    }

    /// Produce the range spanned by this node
    pub fn range(&self) -> Range {
        match self {
            Self::Scope(l) => l.range,
            // This change is important here because otherwise we just capture
            // the tree node for the identifier for this local def, but instead
            // what we want is the full scope of the definition
            Self::Def(d) => d.local_scope.range,
            Self::Ref(r) => r.range,
            Self::Import(i) => i.range,
        }
    }
}

/// A graph representation of scopes and names in a single syntax tree
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ScopeGraph {
    /// The raw graph
    pub graph: Graph<NodeKind, EdgeKind>,

    // Graphs do not have the concept of a `root`, but lexical scopes follow the syntax
    // tree, and as a result, have a "root" node. The root_idx points to a scope node that
    // encompasses the entire file: the global scope.
    root_idx: NodeIndex,

    /// String representation of the language
    lang: String,
}

impl ScopeGraph {
    pub fn new(range: Range, lang: String) -> Self {
        let mut graph = Graph::new();
        let root_idx = graph.add_node(NodeKind::scope(range));
        Self {
            graph,
            root_idx,
            lang,
        }
    }

    pub fn is_definition(&self, node_idx: NodeIndex) -> bool {
        matches!(self.graph[node_idx], NodeKind::Def(_))
    }

    pub fn is_reference(&self, node_idx: NodeIndex) -> bool {
        matches!(self.graph[node_idx], NodeKind::Ref(_))
    }

    pub fn is_import(&self, node_idx: NodeIndex) -> bool {
        matches!(self.graph[node_idx], NodeKind::Import(_))
    }

    pub fn node_by_range(&self, start_byte: usize, end_byte: usize) -> Option<NodeIndex> {
        self.graph
            .node_indices()
            .filter(|&idx| self.is_definition(idx) || self.is_reference(idx) || self.is_import(idx))
            .find(|&idx| {
                let node = self.graph[idx].range();
                start_byte >= node.start_byte() && end_byte <= node.end_byte()
            })
    }

    pub fn tightest_node_for_range(&self, start_byte: usize, end_byte: usize) -> Option<NodeIndex> {
        let mut node_idxs = self
            .graph
            .node_indices()
            .filter(|&idx| self.is_definition(idx))
            .filter(|&idx| {
                let node = self.graph[idx].range();
                node.start_byte() >= start_byte && node.end_byte() <= end_byte
            })
            .collect::<Vec<_>>();
        node_idxs.sort_by(|a, b| {
            let first_node = self.graph[a.clone()].range().byte_size();
            let second_node = self.graph[b.clone()].range().byte_size();
            first_node.cmp(&second_node)
        });
        node_idxs.get(0).map(|val| val.clone())
    }

    pub fn symbols(&self, language_parsing: Arc<TSLanguageParsing>) -> Vec<Symbol> {
        let namespaces = language_parsing
            .for_lang(&self.lang)
            .map(|ts_config| ts_config.namespaces.clone())
            .unwrap_or_default();
        self.graph
            .node_weights()
            .filter_map(|weight| match weight {
                NodeKind::Def(LocalDef {
                    range,
                    symbol_id: Some(symbol_id),
                    ..
                }) => Some(Symbol {
                    kind: symbol_id.name(namespaces.clone()).to_owned(), // FIXME: this should use SymbolId::name
                    range: *range,
                }),
                _ => None,
            })
            .collect()
    }

    // The smallest scope that encompasses `range`. Start at `start` and narrow down if possible.
    fn scope_by_range(&self, range: Range, start: NodeIndex) -> Option<NodeIndex> {
        let target_range = self.graph[start].range();
        if target_range.contains(&range) {
            let child_scopes = self
                .graph
                .edges_directed(start, Direction::Incoming)
                .filter(|edge| *edge.weight() == EdgeKind::ScopeToScope)
                .map(|edge| edge.source())
                .collect::<Vec<_>>();
            for child_scope in child_scopes {
                if let Some(t) = self.scope_by_range(range, child_scope) {
                    return Some(t);
                }
            }
            return Some(start);
        }
        None
    }

    /// Insert a local scope into the scope-graph
    pub fn insert_local_scope(&mut self, new: LocalScope) {
        if let Some(parent_scope) = self.scope_by_range(new.range, self.root_idx) {
            let new_scope = NodeKind::Scope(new);
            let new_idx = self.graph.add_node(new_scope);
            self.graph
                .add_edge(new_idx, parent_scope, EdgeKind::ScopeToScope);
        }
    }

    /// We try to find the tightest local scope which contains this range
    fn find_tightest_local_scope(&self, range: &Range) -> LocalScope {
        let mut current_node = self.root_idx;
        loop {
            let mut found = false;
            for edge in self.graph.edges_directed(current_node, Direction::Incoming) {
                if let EdgeKind::ScopeToScope = edge.weight() {
                    let node = &self.graph[edge.source()];
                    if let NodeKind::Scope(scope) = node {
                        if scope.range.contains(range) {
                            current_node = edge.source();
                            found = true;
                            break;
                        }
                    }
                }
            }
            if !found {
                break;
            }
        }
        if let NodeKind::Scope(scope) = &self.graph[current_node] {
            scope.clone()
        } else {
            unreachable!()
        }
    }

    /// Insert an import into the scope-graph
    pub fn insert_local_import(&mut self, new: LocalImport) {
        if let Some(defining_scope) = self.scope_by_range(new.range, self.root_idx) {
            let new_imp = NodeKind::Import(new);
            let new_idx = self.graph.add_node(new_imp);
            self.graph
                .add_edge(new_idx, defining_scope, EdgeKind::ImportToScope);
        }
    }

    /// Insert a def into the scope-graph, at the parent scope of the defining scope
    pub fn insert_hoisted_def(&mut self, new: LocalDef) {
        if let Some(defining_scope) = self.scope_by_range(new.range, self.root_idx) {
            let new_def = NodeKind::Def(new);
            let new_idx = self.graph.add_node(new_def);

            // if the parent scope exists, insert this def there, if not,
            // insert into the defining scope
            let target_scope = self.parent_scope(defining_scope).unwrap_or(defining_scope);

            self.graph
                .add_edge(new_idx, target_scope, EdgeKind::DefToScope);
        }
    }

    /// Insert a def into the scope-graph, at the root scope
    pub fn insert_global_def(&mut self, new: LocalDef) {
        let new_def = NodeKind::Def(new);
        let new_idx = self.graph.add_node(new_def);
        self.graph
            .add_edge(new_idx, self.root_idx, EdgeKind::DefToScope);
    }

    // Produce the parent scope of a given scope
    fn parent_scope(&self, start: NodeIndex) -> Option<NodeIndex> {
        if matches!(self.graph[start], NodeKind::Scope(_)) {
            return self
                .graph
                .edges_directed(start, Direction::Outgoing)
                .filter(|edge| *edge.weight() == EdgeKind::ScopeToScope)
                .map(|edge| edge.target())
                .next();
        }
        None
    }

    /// Insert a def into the scope-graph
    pub fn insert_local_def(&mut self, new: LocalDef) {
        if let Some(defining_scope) = self.scope_by_range(new.range, self.root_idx) {
            let new_def = NodeKind::Def(new);
            let new_idx = self.graph.add_node(new_def);
            self.graph
                .add_edge(new_idx, defining_scope, EdgeKind::DefToScope);
        }
    }

    fn scope_stack(&self, start: NodeIndex) -> ScopeStack<'_> {
        ScopeStack {
            scope_graph: self,
            start: Some(start),
        }
    }

    /// Insert a ref into the scope-graph
    pub fn insert_ref(&mut self, new: Reference, src: &[u8]) {
        let mut possible_defs = vec![];
        let mut possible_imports = vec![];
        if let Some(local_scope_idx) = self.scope_by_range(new.range, self.root_idx) {
            // traverse the scopes from the current-scope to the root-scope
            for scope in self.scope_stack(local_scope_idx) {
                // find candidate definitions in each scope
                for local_def in self
                    .graph
                    .edges_directed(scope, Direction::Incoming)
                    .filter(|edge| *edge.weight() == EdgeKind::DefToScope)
                    .map(|edge| edge.source())
                {
                    if let NodeKind::Def(def) = &self.graph[local_def] {
                        if new.name(src) == def.name(src) {
                            match (&def.symbol_id, &new.symbol_id) {
                                // both contain symbols, but they don't belong to the same namepspace
                                (Some(d), Some(r)) if d.namespace_idx != r.namespace_idx => {}

                                // in all other cases, form an edge from the ref to def.
                                // an empty symbol belongs to all namespaces:
                                // * (None, None)
                                // * (None, Some(_))
                                // * (Some(_), None)
                                // * (Some(_), Some(_)) if def.namespace == ref.namespace
                                _ => {
                                    possible_defs.push(local_def);
                                }
                            };
                        }
                    }
                }

                // find candidate imports in each scope
                for local_import in self
                    .graph
                    .edges_directed(scope, Direction::Incoming)
                    .filter(|edge| *edge.weight() == EdgeKind::ImportToScope)
                    .map(|edge| edge.source())
                {
                    if let NodeKind::Import(import) = &self.graph[local_import] {
                        if new.name(src) == import.name(src) {
                            possible_imports.push(local_import);
                        }
                    }
                }
            }
        }

        if !possible_defs.is_empty() || !possible_imports.is_empty() {
            let new_ref = NodeKind::Ref(new);
            let ref_idx = self.graph.add_node(new_ref);
            for def_idx in possible_defs {
                self.graph.add_edge(ref_idx, def_idx, EdgeKind::RefToDef);
            }
            for imp_idx in possible_imports {
                self.graph.add_edge(ref_idx, imp_idx, EdgeKind::RefToImport);
            }
        }
    }
}

pub fn scope_res_generic(
    query: &tree_sitter::Query,
    root_node: tree_sitter::Node<'_>,
    src: &[u8],
    language: &TSLanguageConfig,
) -> ScopeGraph {
    let namespaces = &language.namespaces;

    enum Scoping {
        Global,
        Hoisted,
        Local,
    }

    // extract supported capture groups
    struct LocalDefCapture<'a> {
        index: u32,
        symbol: Option<&'a str>,
        scoping: Scoping,
    }

    struct LocalRefCapture<'a> {
        index: u32,
        symbol: Option<&'a str>,
    }

    impl FromStr for Scoping {
        type Err = String;
        fn from_str(s: &str) -> Result<Self, Self::Err> {
            match s {
                "hoist" => Ok(Self::Hoisted),
                "global" => Ok(Self::Global),
                "local" => Ok(Self::Local),
                s => Err(s.to_owned()),
            }
        }
    }

    // every capture of the form:
    //  - local.definition.<symbol>
    //  - hoist.definition.<symbol>
    // is a local_def
    let mut local_def_captures = Vec::<LocalDefCapture<'_>>::new();

    // every capture of the form local.import is a local_import
    let mut local_import_capture_index = None;

    // every capture of the form local.reference.<symbol> is a local_ref
    let mut local_ref_captures = Vec::<LocalRefCapture<'_>>::new();

    // every capture of the form local.scope is a local_scope
    let mut local_scope_capture_index = None;

    // determine indices of every capture group in the query file
    for (i, name) in query.capture_names().iter().enumerate() {
        let i = i as u32;
        let parts: Vec<_> = name.split('.').collect();

        match parts.as_slice() {
            [scoping, "definition", sym] => {
                let index = i;
                let symbol = Some(sym.to_owned());
                let scoping = Scoping::from_str(scoping).expect("invalid scope keyword");

                let l = LocalDefCapture {
                    index,
                    symbol,
                    scoping,
                };
                local_def_captures.push(l)
            }
            [scoping, "definition"] => {
                let index = i;
                let symbol = None;
                let scoping = Scoping::from_str(scoping).expect("invalid scope keyword");

                let l = LocalDefCapture {
                    index,
                    symbol,
                    scoping,
                };
                local_def_captures.push(l)
            }
            ["local", "reference", sym] => {
                let index = i;
                let symbol = Some(sym.to_owned());

                let l = LocalRefCapture { index, symbol };
                local_ref_captures.push(l);
            }
            ["local", "reference"] => {
                let index = i;
                let symbol = None;

                let l = LocalRefCapture { index, symbol };
                local_ref_captures.push(l);
            }
            ["local", "scope"] => local_scope_capture_index = Some(i),
            ["local", "import"] => local_import_capture_index = Some(i),
            _ if !name.starts_with('_') => warn!(?name, "unrecognized query capture"),
            _ => (), // allow captures that start with underscore to fly under the radar
        }
    }

    // run scope-query upon the syntax-tree
    let mut cursor = QueryCursor::new();
    let captures = cursor.captures(query, root_node, src);

    let mut scope_graph = ScopeGraph::new(
        Range::for_tree_node(&root_node),
        language
            .language_ids
            .get(0)
            .expect("atleast one language id to be present")
            .to_string(),
    );

    let capture_map = captures.fold(HashMap::new(), |mut map, (match_, capture_idx)| {
        let capture = match_.captures[capture_idx];
        let range: Range = Range::for_tree_node(&capture.node);
        map.entry(capture.index)
            .or_insert_with(Vec::new)
            .push(range);
        map
    });

    // insert scopes first
    if let Some(ranges) = local_scope_capture_index.and_then(|idx| capture_map.get(&idx)) {
        for range in ranges {
            let scope = LocalScope::new(*range);
            scope_graph.insert_local_scope(scope);
        }
    }

    // followed by imports
    if let Some(ranges) = local_import_capture_index.and_then(|idx| capture_map.get(&idx)) {
        for range in ranges {
            let import = LocalImport::new(*range);
            scope_graph.insert_local_import(import);
        }
    }

    // followed by defs
    for LocalDefCapture {
        index,
        symbol,
        scoping,
    } in local_def_captures
    {
        if let Some(ranges) = capture_map.get(&index) {
            for range in ranges {
                // if the symbol is present, is it one of the supported symbols for this language?
                let symbol_id = symbol.and_then(|s| namespaces.symbol_id_of(s));
                let local_scope = scope_graph.find_tightest_local_scope(range);
                let local_def = LocalDef::new(*range, symbol_id, local_scope);

                match scoping {
                    Scoping::Hoisted => scope_graph.insert_hoisted_def(local_def),
                    Scoping::Global => scope_graph.insert_global_def(local_def),
                    Scoping::Local => scope_graph.insert_local_def(local_def),
                };
            }
        }
    }

    // and then refs
    for LocalRefCapture { index, symbol } in local_ref_captures {
        if let Some(ranges) = capture_map.get(&index) {
            for range in ranges {
                // if the symbol is present, is it one of the supported symbols for this language?
                let symbol_id = symbol.and_then(|s| namespaces.symbol_id_of(s));
                let ref_ = Reference::new(range.clone(), symbol_id);

                scope_graph.insert_ref(ref_, src);
            }
        }
    }

    scope_graph
}

/// A marker indicating a subset of some source text, with a list of highlighted ranges.
///
/// This doesn't store the actual text data itself, just the position information for simplified
/// merging.
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct Location {
    /// The subset's byte range in the original input string.
    pub byte_range: OpsRange<usize>,

    /// The subset's line range in the original input string.
    pub line_range: OpsRange<usize>,

    /// A set of byte ranges denoting highlighted text indices, on the subset string.
    pub highlights: SmallVec<[OpsRange<usize>; 2]>,
}

impl Location {
    /// Reify this `Location` into a `Snippet`, given the source string and symbols list.
    pub fn reify(self, s: &str, symbols: &[Symbol]) -> Snippet {
        Snippet {
            data: s[self.byte_range.clone()].to_owned(),
            line_range: self.line_range.clone(),
            symbols: symbols
                .iter()
                .filter(|s| {
                    s.range.start_line() >= self.line_range.start
                        && s.range.end_line() <= self.line_range.end
                })
                .cloned()
                .map(|mut sym| {
                    let start_byte = sym.range.start_byte() - self.byte_range.start;
                    let end_byte = sym.range.end_byte() - self.byte_range.start;
                    sym.range.set_start_byte(start_byte);
                    sym.range.set_end_byte(end_byte);
                    sym
                })
                .collect(),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Snipper {
    pub context_before: usize,
    pub context_after: usize,
    pub find_symbols: bool,
    pub case_sensitive: bool,
}

impl Default for Snipper {
    fn default() -> Self {
        Self {
            context_before: 0,
            context_after: 0,
            find_symbols: false,
            case_sensitive: true,
        }
    }
}

impl Snipper {
    pub fn context(mut self, before: usize, after: usize) -> Self {
        self.context_before = before;
        self.context_after = after;
        self
    }

    pub fn find_symbols(mut self, find_symbols: bool) -> Self {
        self.find_symbols = find_symbols;
        self
    }

    pub fn case_sensitive(mut self, case_sensitive: bool) -> Self {
        self.case_sensitive = case_sensitive;
        self
    }

    pub fn expand<'a>(
        &'a self,
        highlight: std::ops::Range<usize>,
        text: &'a str,
        line_ends: &'a [u32],
    ) -> Location {
        let start = text[..highlight.start]
            .rmatch_indices('\n')
            .nth(self.context_before)
            .map(|(i, _)| i + 1)
            .unwrap_or(0);

        let end = text[highlight.end..]
            .match_indices('\n')
            .nth(self.context_after)
            .map(|(i, _)| i + highlight.end)
            .unwrap_or(text.len());

        let line_end = line_ends
            .iter()
            .position(|i| end <= *i as usize)
            .unwrap_or(line_ends.len());

        let line_start = line_ends
            .iter()
            .rev()
            .position(|i| (*i as usize) < start)
            .map(|i| line_ends.len() - i)
            .unwrap_or(0);

        Location {
            byte_range: start..end,
            line_range: line_start..line_end,
            highlights: smallvec![(highlight.start - start)..(highlight.end - start)],
        }
    }
}
