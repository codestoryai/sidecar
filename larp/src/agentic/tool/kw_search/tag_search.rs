use std::collections::HashSet;

use crate::repomap::tag::{SearchMode, Tag, TagIndex};
use thiserror::Error;
pub struct TagSearch {}

impl TagSearch {
    pub fn new() -> Self {
        Self {}
    }

    pub fn search<'a>(
        &self,
        index: &'a TagIndex,
        input: &str,
    ) -> Result<HashSet<&'a Tag>, TagSearchError> {
        if input.len() < 2 {
            return Err(TagSearchError::InputTooShort);
        }

        let tags = index.search_definitions_flattened(input, false, SearchMode::Both);

        println!("TagSearch::search:: Search for {input}: {:?}", tags.len());

        if tags.is_empty() {
            return Err(TagSearchError::NoTagsFound);
        }

        Ok(tags)
    }
}

#[derive(Debug, Error)]
pub enum TagSearchError {
    #[error("Input too long")]
    InputTooLong,
    #[error("Input too short")]
    InputTooShort,
    #[error("Input empty")]
    InputEmpty,
    #[error("No tags found")]
    NoTagsFound,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[tokio::test]
    async fn test_tag_search() {
        let path = Path::new("/Users/zi/codestory/sidecar/sidecar");

        // Create a mock TagIndex
        let index = TagIndex::from_path(path).await;

        let tag_search = TagSearch::new();

        // Test successful search
        let result = tag_search.search(&index, "tagindex");
        assert!(result.is_ok());

        // Test search with no results
        let result = tag_search.search(&index, "nonexistent");
        assert!(matches!(result, Err(TagSearchError::NoTagsFound)));

        // Test search with empty input
        let result = tag_search.search(&index, "");
        assert!(matches!(result, Err(TagSearchError::InputTooShort)));
    }
}
