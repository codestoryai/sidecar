use std::cmp::min;

use crate::chunking::languages::TSLanguageParsing;
use crate::repomap::tree_context::TreeContext;

use super::analyser::TagAnalyzer;
use super::error::RepoMapError;
use super::tag::{Tag, TagIndex};

pub struct RepoMap {
    map_tokens: usize,
}

const REPOMAP_DEFAULT_TOKENS: usize = 1024;

impl RepoMap {
    pub fn new() -> Self {
        Self {
            map_tokens: REPOMAP_DEFAULT_TOKENS,
        }
    }

    pub fn with_map_tokens(mut self, map_tokens: usize) -> Self {
        self.map_tokens = map_tokens;
        self
    }

    pub async fn get_repo_map(&self, tag_index: &TagIndex) -> Result<String, RepoMapError> {
        let repomap = self.get_ranked_tags_map(self.map_tokens, tag_index).await?;

        if repomap.is_empty() {
            return Err(RepoMapError::TreeGenerationError(
                "No tree generated".to_string(),
            ));
        }

        println!("Repomap: {}k tokens", self.get_token_count(&repomap) / 1024);

        Ok(repomap)
    }

    fn get_token_count(&self, tree: &str) -> usize {
        let chars = tree.chars().count();

        // https://platform.openai.com/tokenizer
        let token_per_char_ratio = 0.25;

        let token_estimate = (chars as f64 * token_per_char_ratio) as usize;

        token_estimate
    }

    fn find_best_tree(&self, ranked_tags: Vec<Tag>, max_map_tokens: usize) -> String {
        let num_tags = ranked_tags.len();
        println!("Initial conditions:");
        println!("  Number of tags: {}", num_tags);
        println!("  Max map tokens: {}", max_map_tokens);

        let mut lower_bound = 0;
        let mut upper_bound = num_tags;
        let mut best_tree = String::new();
        let mut best_tree_tokens = 0;
        let mut middle = min(max_map_tokens / 25, num_tags);
        let mut iteration = 0;

        while lower_bound <= upper_bound {
            iteration += 1;
            println!("\nIteration {}:", iteration);
            println!("  Bounds: [{}, {}]", lower_bound, upper_bound);
            println!("  Middle: {}", middle);

            // The clone here is very very expensive
            let tree = RepoMap::to_tree(&ranked_tags[..middle].to_vec());
            let num_tokens = self.get_token_count(&tree);

            println!("  Tree tokens: {}", num_tokens);

            if num_tokens < max_map_tokens && num_tokens > best_tree_tokens {
                println!("  New best tree found!");
                println!("    Previous best: {} tokens", best_tree_tokens);
                println!("    New best: {} tokens", num_tokens);
                best_tree.replace_range(.., &tree);
                best_tree_tokens = num_tokens;
            }

            if num_tokens < max_map_tokens {
                println!("  Increasing lower bound");
                lower_bound = middle + 1;
            } else {
                println!("  Decreasing upper bound");
                upper_bound = middle - 1;
            }

            middle = (lower_bound + upper_bound) / 2;

            println!("  Next middle: {}", middle);
        }

        println!("\nSearch completed:");
        println!("  Best tree tokens: {}", best_tree_tokens);
        println!("  Final bounds: [{}, {}]", lower_bound, upper_bound);

        best_tree
    }

    pub async fn get_ranked_tags_map(
        &self,
        max_map_tokens: usize,
        tag_index: &TagIndex,
    ) -> Result<String, RepoMapError> {
        let mut analyser = TagAnalyzer::new(&tag_index);

        println!("[Analyser] Ranking tags...");
        let ranked_tags = analyser.get_ranked_tags().clone();
        println!("[Analyser] tags::len({})", ranked_tags.len());

        println!("[Tree] Finding best tree...");
        let tree = self.find_best_tree(ranked_tags, max_map_tokens);

        Ok(tree)
    }

    pub fn get_tag_snippet(tag: &Tag, max_lines: usize) -> Result<String, RepoMapError> {
        let file_content = std::fs::read(&tag.fname).map_err(|_| RepoMapError::IoError)?;

        let code = String::from_utf8_lossy(&file_content).to_string();
        let lines: Vec<&str> = code.lines().collect();

        let start_line = tag.line.saturating_sub(1); // 0-based index
        let end_line = std::cmp::min(start_line + max_lines, lines.len());

        let snippet: String = lines[start_line..end_line].join("\n");

        Ok(snippet)
    }

    pub fn to_tree(tags: &Vec<Tag>) -> String {
        let mut tags = tags.clone();
        tags.sort_by(|a, b| a.rel_fname.cmp(&b.rel_fname));
        tags.push(Tag::dummy());

        let mut output = String::new();

        let mut cur_fname = "";
        let mut cur_abs_fname = "";

        let mut lois: Option<Vec<usize>> = None;

        for tag in &tags {
            let this_rel_fname = tag.rel_fname.to_str().expect("to_str to work for path");

            // check whether filename has changed, including first iteration
            if this_rel_fname != cur_fname {
                // take() resets the lois to None, inner_lois may be used as value for render_tree
                if let Some(inner_lois) = lois.take() {
                    output.push('\n');
                    output.push_str(&cur_abs_fname);
                    output.push_str(":\n");
                    let file_content = std::fs::read(&cur_abs_fname);
                    if let Err(_) = file_content {
                        continue;
                    }
                    let file_content = file_content.expect("file_content to be present");
                    // read the file content and keep track of it
                    output.push_str(&RepoMap::render_tree(
                        &cur_abs_fname,
                        &cur_fname,
                        &inner_lois,
                        &file_content,
                    ));
                } else if !cur_fname.is_empty() {
                    output.push('\n');
                    output.push_str(&cur_abs_fname);
                    output.push('\n');
                }

                lois = Some(Vec::new());
                cur_abs_fname = tag.fname.to_str().unwrap();
                cur_fname = this_rel_fname;
            }

            // as_mut() is critical here as we want to mutate the original lois
            if let Some(lois) = lois.as_mut() {
                lois.push(tag.line);
            }
        }

        output = output
            .lines()
            .map(|line| {
                if line.len() > 100 {
                    line[..100].to_string()
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<String>>()
            .join("\n");
        output.push('\n');

        output
    }

    pub fn render_tree(
        abs_fname: &str,
        _rel_fname: &str,
        lois: &Vec<usize>,
        file_content: &Vec<u8>,
    ) -> String {
        let mut code = String::from_utf8_lossy(file_content.as_slice()).to_string();
        if !code.ends_with('\n') {
            code.push('\n');
        }

        let ts_parsing = TSLanguageParsing::init();
        let config = ts_parsing.for_file_path(abs_fname).unwrap().clone();

        let tree = config.get_tree_sitter_tree(code.as_bytes()).unwrap();

        let root_node = tree.root_node();

        let cursor = root_node.walk();

        // todo - consider using rel_fname
        let mut context = TreeContext::new(code, abs_fname.to_owned());

        context.init(cursor);

        context.add_lois(lois.clone());

        context.add_context();

        context.format()
    }
}
