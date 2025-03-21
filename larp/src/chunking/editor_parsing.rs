use std::path::PathBuf;

use regex::Regex;

use crate::repo::types::RepoRef;

use super::{
    file_content::file_content_language_config,
    go::go_language_config,
    javascript::javascript_language_config,
    languages::TSLanguageConfig,
    python::python_language_config,
    rust::rust_language_config,
    text_document::{DocumentSymbol, Position, Range, TextDocument},
    types::FunctionInformation,
    typescript::typescript_language_config,
};

/// Here we will parse the document we get from the editor using symbol level
/// information, as its very fast

#[derive(Debug, Clone)]
pub struct EditorParsing {
    configs: Vec<TSLanguageConfig>,
}

impl Default for EditorParsing {
    fn default() -> Self {
        Self {
            configs: vec![
                rust_language_config(),
                javascript_language_config(),
                typescript_language_config(),
                python_language_config(),
                go_language_config(),
                file_content_language_config(),
            ],
        }
    }
}

impl EditorParsing {
    pub fn ts_language_config(&self, language: &str) -> Option<&TSLanguageConfig> {
        self.configs
            .iter()
            .find(|config| config.language_ids.contains(&language))
    }

    pub fn for_file_path(&self, file_path: &str) -> Option<&TSLanguageConfig> {
        let file_path = PathBuf::from(file_path);
        let file_extension = file_path
            .extension()
            .map(|extension| extension.to_str())
            .map(|extension| extension.to_owned())
            .flatten();
        match file_extension {
            Some(extension) => {
                let language_config = self
                    .configs
                    .iter()
                    .find(|config| config.file_extensions.contains(&extension));
                if language_config.is_none() {
                    // return the file config
                    self.configs
                        .iter()
                        .find(|config| config.file_extensions.contains(&"*"))
                } else {
                    language_config
                }
            }
            None => {
                // return the file config
                self.configs
                    .iter()
                    .find(|config| config.file_extensions.contains(&"*"))
            }
        }
    }

    fn is_node_identifier(
        &self,
        node: &tree_sitter::Node,
        language_config: &TSLanguageConfig,
    ) -> bool {
        match language_config
            .language_ids
            .first()
            .expect("language_id to be present")
            .to_lowercase()
            .as_ref()
        {
            "typescript" | "typescriptreact" | "javascript" | "javascriptreact" => {
                Regex::new(r"(definition|declaration|declarator|export_statement)")
                    .unwrap()
                    .is_match(node.kind())
            }
            "golang" => Regex::new(r"(definition|declaration|declarator|var_spec)")
                .unwrap()
                .is_match(node.kind()),
            "cpp" => Regex::new(r"(definition|declaration|declarator|class_specifier)")
                .unwrap()
                .is_match(node.kind()),
            "ruby" => Regex::new(r"(module|class|method|assignment)")
                .unwrap()
                .is_match(node.kind()),
            "rust" => Regex::new(r"(item)").unwrap().is_match(node.kind()),
            _ => Regex::new(r"(definition|declaration|declarator)")
                .unwrap()
                .is_match(node.kind()),
        }
    }

    fn get_identifier_node_fully_contained<'a>(
        &'a self,
        tree_sitter_node: tree_sitter::Node<'a>,
        range: &'a Range,
        language_config: &'a TSLanguageConfig,
        _source_str: &str,
    ) -> Option<tree_sitter::Node<'a>> {
        let mut nodes = vec![tree_sitter_node];
        let mut identifier_nodes: Vec<(tree_sitter::Node, f64)> = vec![];
        loop {
            // Here we take the nodes in [nodes] which have an intersection
            // with the range we are interested in
            let mut intersecting_nodes = nodes
                .into_iter()
                .map(|tree_sitter_node| {
                    (
                        tree_sitter_node,
                        Range::for_tree_node(&tree_sitter_node).intersection_size(range) as f64,
                    )
                })
                .filter(|(_, intersection_size)| intersection_size > &0.0)
                .collect::<Vec<_>>();
            // we sort the nodes by their intersection size
            // we want to keep the biggest size here on the top
            intersecting_nodes.sort_by(|a, b| b.1.partial_cmp(&a.1).expect("partial_cmp to work"));

            // if there are no nodes, then we return none or the most relevant nodes
            // from i, which is the biggest node here
            if intersecting_nodes.is_empty() {
                return if identifier_nodes.is_empty() {
                    None
                } else {
                    Some({
                        let mut current_node = identifier_nodes[0];
                        for identifier_node in &identifier_nodes[1..] {
                            if identifier_node.1 - current_node.1 > 0.0 {
                                current_node = identifier_node.clone();
                            }
                        }
                        current_node.0
                    })
                };
            }

            // For the nodes in intersecting_nodes, calculate a relevance score and filter the ones that are declarations or definitions for language 'language_config'
            let identifier_nodes_sorted = intersecting_nodes
                .iter()
                .map(|(tree_sitter_node, intersection_size)| {
                    let len = Range::for_tree_node(&tree_sitter_node).len();
                    let diff = ((range.len() as f64 - intersection_size) as f64).abs();
                    let relevance_score = (intersection_size - diff) as f64 / len as f64;
                    (tree_sitter_node.clone(), relevance_score)
                })
                .collect::<Vec<_>>();

            // now we filter out the nodes which are here based on the identifier function and set it to identifier nodes
            // which we want to find for documentation
            identifier_nodes.extend(
                identifier_nodes_sorted
                    .into_iter()
                    .filter(|(tree_sitter_node, _)| {
                        self.is_node_identifier(tree_sitter_node, language_config)
                    })
                    .map(|(tree_sitter_node, score)| (tree_sitter_node, score))
                    .collect::<Vec<_>>(),
            );

            // Now we prepare for the next iteration by setting nodes to the children of the nodes
            // in intersecting_nodes
            nodes = intersecting_nodes
                .into_iter()
                .map(|(tree_sitter_node, _)| {
                    let mut cursor = tree_sitter_node.walk();
                    tree_sitter_node.children(&mut cursor).collect::<Vec<_>>()
                })
                .flatten()
                .collect::<Vec<_>>();
        }
    }

    fn get_identifier_node_by_expanding<'a>(
        &'a self,
        tree_sitter_node: tree_sitter::Node<'a>,
        range: &Range,
        language_config: &TSLanguageConfig,
    ) -> Option<tree_sitter::Node<'a>> {
        let tree_sitter_range = range.to_tree_sitter_range();
        let mut expanding_node = tree_sitter_node
            .descendant_for_byte_range(tree_sitter_range.start_byte, tree_sitter_range.end_byte);
        loop {
            // Here we expand this node until we hit a identifier node, this is
            // a very easy way to get to the best node we are interested in by
            // bubbling up
            if expanding_node.is_none() {
                return None;
            }
            match expanding_node {
                Some(expanding_node_val) => {
                    // if this is not a identifier and the parent is there, we keep
                    // going up
                    if !self.is_node_identifier(&expanding_node_val, &language_config)
                        && expanding_node_val.parent().is_some()
                    {
                        expanding_node = expanding_node_val.parent()
                    // if we have a node identifier, return right here!
                    } else if self.is_node_identifier(&expanding_node_val, &language_config) {
                        return Some(expanding_node_val.clone());
                    } else {
                        // so we don't have a node identifier and neither a parent, so
                        // just return None
                        return None;
                    }
                }
                None => {
                    return None;
                }
            }
        }
    }

    pub fn get_documentation_node(
        &self,
        text_document: &TextDocument,
        language_config: &TSLanguageConfig,
        range: Range,
    ) -> Vec<DocumentSymbol> {
        let language = language_config.grammar;
        let source = text_document.get_content_buffer();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(language()).unwrap();
        let tree = parser
            .parse(text_document.get_content_buffer().as_bytes(), None)
            .unwrap();
        if let Some(identifier_node) = self.get_identifier_node_fully_contained(
            tree.root_node(),
            &range,
            &language_config,
            source,
        ) {
            // we have a identifier node right here, so lets get the document symbol
            // for this and return it back
            return DocumentSymbol::from_tree_node(
                &identifier_node,
                language_config,
                text_document.get_content_buffer(),
            )
            .into_iter()
            .collect();
        }
        // or else we try to expand the node out so we can get a symbol back
        if let Some(expanded_node) =
            self.get_identifier_node_by_expanding(tree.root_node(), &range, &language_config)
        {
            // we get the expanded node here again
            return DocumentSymbol::from_tree_node(
                &expanded_node,
                language_config,
                text_document.get_content_buffer(),
            )
            .into_iter()
            .collect();
        }
        // or else we return nothing here
        vec![]
    }

    pub fn get_documentation_node_for_range(
        &self,
        source_str: &str,
        language: &str,
        relative_path: &str,
        fs_file_path: &str,
        start_position: &Position,
        end_position: &Position,
        repo_ref: &RepoRef,
    ) -> Vec<DocumentSymbol> {
        // First we need to find the language config which matches up with
        // the language we are interested in
        let language_config = self.ts_language_config(&language);
        if let None = language_config {
            return vec![];
        }
        // okay now we have a language config, lets parse it
        self.get_documentation_node(
            &TextDocument::new(
                source_str.to_owned(),
                repo_ref.clone(),
                fs_file_path.to_owned(),
                relative_path.to_owned(),
            ),
            &language_config.expect("if let None check above to work"),
            Range::new(start_position.clone(), end_position.clone()),
        )
    }

    pub fn function_information_nodes(
        &self,
        source_code: &[u8],
        language: &str,
    ) -> Vec<FunctionInformation> {
        let language_config = self.ts_language_config(&language);
        if let None = language_config {
            return vec![];
        }
        language_config
            .expect("if let None check above")
            .function_information_nodes(source_code)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        chunking::{
            languages::TSLanguageParsing,
            text_document::{Position, Range, TextDocument},
        },
        repo::types::RepoRef,
    };

    use super::EditorParsing;

    #[test]
    fn rust_selection_parsing() {
        let editor_parsing = EditorParsing::default();
        // This is from the configuration file
        let source_str = "use std::{\n    num::NonZeroUsize,\n    path::{Path, PathBuf},\n};\n\nuse clap::Parser;\nuse serde::{Deserialize, Serialize};\n\nuse crate::repo::state::StateSource;\n\n#[derive(Serialize, Deserialize, Parser, Debug, Clone)]\n#[clap(author, version, about, long_about = None)]\npub struct Configuration {\n    #[clap(short, long, default_value_os_t = default_index_dir())]\n    #[serde(default = \"default_index_dir\")]\n    /// Directory to store all persistent state\n    pub index_dir: PathBuf,\n\n    #[clap(long, default_value_t = default_port())]\n    #[serde(default = \"default_port\")]\n    /// Bind the webserver to `<host>`\n    pub port: u16,\n\n    #[clap(long)]\n    /// Path to the embedding model directory\n    pub model_dir: PathBuf,\n\n    #[clap(long, default_value_t = default_host())]\n    #[serde(default = \"default_host\")]\n    /// Bind the webserver to `<port>`\n    pub host: String,\n\n    #[clap(flatten)]\n    #[serde(default)]\n    pub state_source: StateSource,\n\n    #[clap(short, long, default_value_t = default_parallelism())]\n    #[serde(default = \"default_parallelism\")]\n    /// Maximum number of parallel background threads\n    pub max_threads: usize,\n\n    #[clap(short, long, default_value_t = default_buffer_size())]\n    #[serde(default = \"default_buffer_size\")]\n    /// Size of memory to use for file indexes\n    pub buffer_size: usize,\n\n    /// Qdrant url here can be mentioned if we are running it remotely or have\n    /// it running on its own process\n    #[clap(long)]\n    #[serde(default = \"default_qdrant_url\")]\n    pub qdrant_url: String,\n\n    /// The folder where the qdrant binary is present so we can start the server\n    /// and power the qdrant client\n    #[clap(short, long)]\n    pub qdrant_binary_directory: Option<PathBuf>,\n\n    /// The location for the dylib directory where we have the runtime binaries\n    /// required for ort\n    #[clap(short, long)]\n    pub dylib_directory: PathBuf,\n\n    /// Qdrant allows us to create collections and we need to provide it a default\n    /// value to start with\n    #[clap(short, long, default_value_t = default_collection_name())]\n    #[serde(default = \"default_collection_name\")]\n    pub collection_name: String,\n\n    #[clap(long, default_value_t = interactive_batch_size())]\n    #[serde(default = \"interactive_batch_size\")]\n    /// Batch size for batched embeddings\n    pub embedding_batch_len: NonZeroUsize,\n\n    #[clap(long, default_value_t = default_user_id())]\n    #[serde(default = \"default_user_id\")]\n    user_id: String,\n\n    /// If we should poll the local repo for updates auto-magically. Disabled\n    /// by default, until we figure out the delta sync method where we only\n    /// reindex the files which have changed\n    #[clap(long)]\n    pub enable_background_polling: bool,\n}\n\nimpl Configuration {\n    /// Directory where logs are written to\n    pub fn log_dir(&self) -> PathBuf {\n        self.index_dir.join(\"logs\")\n    }\n\n    pub fn index_path(&self, name: impl AsRef<Path>) -> impl AsRef<Path> {\n        self.index_dir.join(name)\n    }\n\n    pub fn qdrant_storage(&self) -> PathBuf {\n        self.index_dir.join(\"qdrant_storage\")\n    }\n}\n\nfn default_index_dir() -> PathBuf {\n    match directories::ProjectDirs::from(\"ai\", \"codestory\", \"sidecar\") {\n        Some(dirs) => dirs.data_dir().to_owned(),\n        None => \"codestory_sidecar\".into(),\n    }\n}\n\nfn default_port() -> u16 {\n    42424\n}\n\nfn default_host() -> String {\n    \"127.0.0.1\".to_owned()\n}\n\npub fn default_parallelism() -> usize {\n    std::thread::available_parallelism().unwrap().get()\n}\n\nconst fn default_buffer_size() -> usize {\n    100_000_000\n}\n\nfn default_collection_name() -> String {\n    \"codestory\".to_owned()\n}\n\nfn interactive_batch_size() -> NonZeroUsize {\n    NonZeroUsize::new(1).unwrap()\n}\n\nfn default_qdrant_url() -> String {\n    \"http://127.0.0.1:6334\".to_owned()\n}\n\nfn default_user_id() -> String {\n    \"codestory\".to_owned()\n}\n";
        let range = Range::new(Position::new(134, 7, 3823), Position::new(137, 0, 3878));
        let ts_lang_parsing = TSLanguageParsing::init();
        let rust_config = ts_lang_parsing.for_lang("rust");
        let mut documentation_nodes = editor_parsing.get_documentation_node(
            &TextDocument::new(
                source_str.to_owned(),
                RepoRef::local("/Users/skcd/testing/").expect("test to work"),
                "".to_owned(),
                "".to_owned(),
            ),
            &rust_config.expect("rust config to be present"),
            range,
        );
        assert!(!documentation_nodes.is_empty());
        let first_entry = documentation_nodes.remove(0);
        assert_eq!(first_entry.start_position, Position::new(134, 0, 3816));
        assert_eq!(first_entry.end_position, Position::new(136, 1, 3877));
    }
}
