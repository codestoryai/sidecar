use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::chunking::languages::TSLanguageParsing;

use super::error::RepoMapError;

use super::file::errors::FileError;
use super::file::git::GitWalker;
use futures::{stream, StreamExt};

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Tag {
    pub rel_fname: PathBuf,
    pub fname: PathBuf,
    pub line: usize,
    pub name: String,
    pub kind: TagKind,
}

impl Tag {
    pub fn new(
        rel_fname: PathBuf,
        fname: PathBuf,
        line: usize,
        name: String,
        kind: TagKind,
    ) -> Self {
        Self {
            rel_fname,
            fname,
            line,
            name,
            kind,
        }
    }

    /// Using this to generate a dummy tag
    pub fn dummy() -> Self {
        Self {
            rel_fname: PathBuf::new(),
            fname: PathBuf::new(),
            line: 0,
            name: "".to_owned(),
            kind: TagKind::Definition,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TagKind {
    Definition,
    Reference,
}

/// An index structure for managing tags across multiple files.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TagIndex {
    /// Maps tag names to the set of file paths where the tag is defined.
    ///
    /// Useful for answering: "In which files is tag X defined?"
    pub defines: HashMap<String, HashSet<PathBuf>>,

    /// Maps tag names to a list of file paths where the tag is referenced.
    ///
    /// Allows duplicates to accommodate multiple references to the same definition.
    pub references: HashMap<String, Vec<PathBuf>>,

    /// Maps (file path, tag name) pairs to a set of tag definitions.
    ///
    /// Useful for answering: "What are the details of tag X in file Y?"
    ///
    /// Needs to be a HashSet<Tag> due to function overloading where multiple functions share the same name but have different parameters
    pub definitions: HashMap<(PathBuf, String), HashSet<Tag>>,

    /// A set of commonly used tags across all files.
    pub common_tags: HashSet<String>,

    /// Maps file paths to the set of tags defined in the file.
    ///
    /// Useful for answering: "What are the tags defined in file X?"
    pub file_to_tags: HashMap<PathBuf, HashSet<(PathBuf, String)>>,
    pub path: PathBuf,
}

impl TagIndex {
    pub fn new(path: &Path) -> Self {
        Self {
            defines: HashMap::new(),
            references: HashMap::new(),
            definitions: HashMap::new(),
            common_tags: HashSet::new(),
            file_to_tags: HashMap::new(),
            path: path.to_path_buf(),
        }
    }

    pub async fn from_files(root_path: &Path, file_paths: Vec<String>) -> Self {
        let mut index = TagIndex::new(root_path);
        let mut files = HashMap::new();

        for path in file_paths {
            let content = fs::read(&path).unwrap();
            files.insert(path, content);
        }

        index.generate_tag_index(files).await;

        index
    }

    pub fn get_files(root: &Path) -> Result<HashMap<String, Vec<u8>>, FileError> {
        let git_walker = GitWalker {};
        git_walker.read_files(root)
    }

    pub async fn generate_from_files(&mut self, files: HashMap<String, Vec<u8>>) {
        self.generate_tag_index(files).await;
    }

    pub async fn from_path(path: &Path) -> Self {
        let mut index = TagIndex::new(path);
        let files = TagIndex::get_files(path).unwrap();

        index.generate_tag_index(files).await;

        index
    }

    pub fn post_process_tags(&mut self) {
        self.process_empty_references();
        self.process_common_tags();
    }

    pub fn add_tag(&mut self, tag: Tag, rel_path: &PathBuf) {
        match tag.kind {
            TagKind::Definition => {
                self.defines
                    .entry(tag.name.clone())
                    .or_default()
                    .insert(rel_path.clone());
                self.definitions
                    .entry((rel_path.clone(), tag.name.clone()))
                    .or_default()
                    .insert(tag.clone());

                self.file_to_tags
                    .entry(rel_path.clone())
                    .or_default()
                    .insert((rel_path.clone(), tag.name.clone()));
            }
            TagKind::Reference => {
                self.references
                    .entry(tag.name.clone())
                    .or_default()
                    .push(rel_path.clone());

                self.file_to_tags
                    .entry(rel_path.clone())
                    .or_default()
                    .insert((rel_path.clone(), tag.name.clone()));
            }
        }
    }

    pub fn process_empty_references(&mut self) {
        if self.references.is_empty() {
            self.references = self
                .defines
                .iter()
                .map(|(k, v)| (k.clone(), v.iter().cloned().collect::<Vec<PathBuf>>()))
                .collect();
        }
    }

    pub fn process_common_tags(&mut self) {
        self.common_tags = self
            .defines
            .keys()
            .filter_map(|key| match self.references.contains_key(key) {
                true => Some(key.clone()),
                false => None,
            })
            .collect();
    }

    pub fn debug_print(&self) {
        println!("==========Defines==========");
        self.defines.iter().for_each(|(key, set)| {
            println!("Key {}, Set: {:?}", key, set);
        });

        println!("==========Definitions==========");
        self.definitions
            .iter()
            .for_each(|((pathbuf, tag_name), set)| {
                println!("Key {:?}, Set: {:?}", (pathbuf, tag_name), set);
            });

        println!("==========References==========");
        self.references.iter().for_each(|(tag_name, paths)| {
            println!("Tag: {}, Paths: {:?}", tag_name, paths);
        });

        println!("==========Common Tags==========");
        self.common_tags.iter().for_each(|tag| {
            println!(
                "Common Tag: {}\n(defined in: {:?}, referenced in: {:?})",
                tag, &self.defines[tag], &self.references[tag]
            );
        });
    }

    async fn generate_tag_index(&mut self, files: HashMap<String, Vec<u8>>) {
        let ts_parsing = Arc::new(TSLanguageParsing::init());
        let _ = stream::iter(
            files
                .into_iter()
                .map(|(file, _)| (file, ts_parsing.clone())),
        )
        .map(|(file, ts_parsing)| async {
            self.generate_tags_for_file(&file, ts_parsing)
                .await
                .map(|tags| (tags, file))
                .ok()
        })
        .buffer_unordered(10000)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(|s| s)
        .for_each(|(tags, file)| {
            let file_ref = &file;
            tags.into_iter().for_each(|tag| {
                self.add_tag(tag, &PathBuf::from(file_ref));
            });
        });

        self.post_process_tags();
    }

    async fn generate_tags_for_file(
        &self,
        fname: &str,
        ts_parsing: Arc<TSLanguageParsing>,
    ) -> Result<Vec<Tag>, RepoMapError> {
        let rel_path = self.get_rel_fname(&PathBuf::from(fname));
        let config = ts_parsing.for_file_path(fname).ok_or_else(|| {
            RepoMapError::ParseError(format!("Language configuration not found for: {}", fname,))
        });
        let content = tokio::fs::read(fname).await;
        if let Err(_) = content {
            return Err(RepoMapError::IoError);
        }
        let content = content.expect("if let Err to hold");
        if let Ok(config) = config {
            let tags = config
                .get_tags(&PathBuf::from(fname), &rel_path, content)
                .await;
            Ok(tags)
        } else {
            Ok(vec![])
        }
    }

    pub fn get_tags_for_file(&self, file_name: &Path) -> Option<Vec<Tag>> {
        self.file_to_tags.get(file_name).map(|tag_ids| {
            tag_ids
                .iter()
                .filter_map(|(relative_path, tag_name)| {
                    self.definitions
                        .get(&(relative_path.clone(), tag_name.clone()))
                        .and_then(|tags| tags.iter().next().cloned())
                })
                .collect()
        })
    }

    pub fn print_file_to_tag_keys(&self) {
        self.file_to_tags.keys().for_each(|key| {
            println!("{}", key.display());
        });
    }

    fn get_rel_fname(&self, fname: &PathBuf) -> PathBuf {
        fname
            .strip_prefix(&self.path)
            .unwrap_or(fname)
            .to_path_buf()
    }

    pub fn definitions(&self) -> &HashMap<(PathBuf, String), HashSet<Tag>> {
        &self.definitions
    }

    pub fn search_definitions(
        &self,
        search_term: &str,
        case_sensitive: bool,
        search_mode: SearchMode,
    ) -> Vec<(&(PathBuf, String), &HashSet<Tag>)> {
        let search_term = if case_sensitive {
            search_term.to_owned()
        } else {
            search_term.to_lowercase()
        };

        println!("=========");
        println!("searching for: {}", search_term);
        println!("=========");

        self.definitions
            .iter()
            .filter(|((path, tag_name), _)| {
                let file_name = path
                    .file_name()
                    .and_then(|os_str| os_str.to_str())
                    .unwrap_or("");

                let file_name = if case_sensitive {
                    file_name.to_owned()
                } else {
                    file_name.to_lowercase()
                };

                let tag_name = if case_sensitive {
                    tag_name.to_owned()
                } else {
                    tag_name.to_lowercase()
                };

                match search_mode {
                    SearchMode::FileName => file_name.contains(&search_term),
                    SearchMode::FilePath => path.to_str().unwrap().contains(&search_term),
                    SearchMode::TagName => tag_name.contains(&search_term),
                    SearchMode::Both => {
                        file_name.contains(&search_term) || tag_name.contains(&search_term)
                    }
                    SearchMode::ExactFileName => file_name == search_term,
                    SearchMode::ExactTagName => tag_name == search_term,
                    SearchMode::StartsWith => {
                        file_name.starts_with(&search_term) || tag_name.starts_with(&search_term)
                    }
                    SearchMode::EndsWith => {
                        file_name.ends_with(&search_term) || tag_name.ends_with(&search_term)
                    }
                }
            })
            .collect()
    }

    pub fn search_definitions_flattened(
        &self,
        search_term: &str,
        case_sensitive: bool,
        search_mode: SearchMode,
    ) -> HashSet<&Tag> {
        self.search_definitions(search_term, case_sensitive, search_mode)
            .into_iter()
            .flat_map(|(_, tags)| tags)
            .collect()
    }
}

pub enum SearchMode {
    FileName,
    FilePath,
    TagName,
    Both,
    ExactFileName,
    ExactTagName,
    StartsWith,
    EndsWith,
}
