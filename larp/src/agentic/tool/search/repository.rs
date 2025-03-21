use std::{
    collections::HashSet,
    fs::{self},
    path::PathBuf,
};

use crate::{
    agentic::tool::search::iterative::{SearchResultSnippet, SearchToolType},
    repomap::{
        file::git::GitWalker,
        tag::{SearchMode, Tag, TagIndex},
        types::RepoMap,
    },
};

use super::iterative::{SearchQuery, SearchResult};

pub const SNIPPET_LINE_BUDGET: usize = 20; // arbitrary budget

#[derive(Debug, Clone)]
pub struct Repository {
    _tree: String,
    _outline: String,
    tag_index: TagIndex,
    root: PathBuf,
}

impl Repository {
    pub fn new(tree: String, outline: String, tag_index: TagIndex, root: PathBuf) -> Self {
        Self {
            _tree: tree,
            _outline: outline,
            tag_index,
            root,
        }
    }

    pub fn get_file_tags(&self, path: &PathBuf) -> Option<Vec<Tag>> {
        self.tag_index.get_tags_for_file(path)
    }

    pub fn get_tag(&self, path: &PathBuf, tag_name: &str) -> Option<&HashSet<Tag>> {
        self.tag_index
            .definitions()
            .get(&(path.to_owned(), tag_name.to_string()))
    }

    pub fn get_tag_snippet(&self, path: &PathBuf, tag_name: &str) -> Option<String> {
        self.get_tag(path, tag_name)
            .and_then(|tag_set| tag_set.iter().next())
            .and_then(|tag| RepoMap::get_tag_snippet(tag, SNIPPET_LINE_BUDGET).ok())
    }

    pub fn execute_search(&self, search_query: &SearchQuery) -> Vec<SearchResult> {
        match search_query.tool {
            SearchToolType::File => {
                println!(
                    "repository::execute_search::query::SearchToolType::File, searching for {}",
                    search_query.query
                );

                let tags_in_file = self.tag_index.search_definitions_flattened(
                    &search_query.query,
                    false,
                    SearchMode::FilePath,
                );

                match tags_in_file.is_empty() {
                    true => {
                        println!("No tags for file: {}", search_query.query);

                        let gitwalker = GitWalker {};

                        let file = gitwalker.find_file(self.root.as_path(), &search_query.query);

                        println!(
                            "repository::execute_search::query::SearchToolType::File::file: {:?}",
                            file
                        );

                        if let Some(path) = file {
                            println!(
                                "repository::execute_search::query::SearchToolType::File::Some(path): {:?}",
                                path
                            );
                            let contents = match fs::read(&path) {
                                Ok(content) => content,
                                Err(error) => {
                                    eprintln!("Error reading file: {}", error);
                                    vec![]
                                }
                            };

                            vec![SearchResult::new(
                                path,
                                &search_query.thinking,
                                SearchResultSnippet::FileContent(contents),
                            )]
                        } else {
                            vec![]
                        }
                    }
                    false => {
                        println!("Tags found for file: {}", tags_in_file.len());

                        let search_results = tags_in_file
                            .iter()
                            .take(20) // so we don't exceed token limits
                            .map(|t| {
                                // helps identify step understand the symbol
                                let thinking_message = format!(
                                    "This file contains a {:?} named {}",
                                    t.kind,
                                    t.name.to_owned()
                                );

                                // todo(zi): make async/parallel
                                let snippet = self.get_tag_snippet(&t.fname, &t.name);

                                SearchResult::new(
                                    t.fname.to_owned(),
                                    &thinking_message,
                                    SearchResultSnippet::Tag(snippet.unwrap_or(t.name.to_owned())),
                                )
                            })
                            .collect::<Vec<SearchResult>>();

                        search_results
                    }
                }
            } // maybe give the thinking to TreeSearch...?
            SearchToolType::Keyword => {
                println!("repository::execute_search::query::SearchToolType::Keyword");

                let result = self.tag_index.search_definitions_flattened(
                    &search_query.query,
                    false,
                    SearchMode::ExactTagName,
                );

                result
                    .iter()
                    .map(|t| {
                        // helps identify step understand the symbol
                        let thinking_message =
                            format!("This file contains a {:?} named {}", t.kind, t.name);
                        SearchResult::new(
                            t.fname.to_owned(),
                            &thinking_message,
                            SearchResultSnippet::Tag(t.name.to_owned()), // why not the tag snippet?
                        )
                    })
                    .collect()
            }
        }
    }
}
