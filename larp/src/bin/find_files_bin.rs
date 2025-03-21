use globset::{Glob, GlobSet, GlobSetBuilder};
use sidecar::agentic::tool::lsp::list_files::list_files;
use std::error::Error;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct FindParams {
    pub search_directory: String,
    pub pattern: String,
    pub includes: Option<Vec<String>>,
    pub excludes: Option<Vec<String>>,
    pub max_depth: Option<i32>,
    pub file_type: Option<String>,
}

pub fn find_files(params: &FindParams, files: &[PathBuf]) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let search_dir = Path::new(&params.search_directory);

    // Compile include globs
    let mut include_patterns = vec![params.pattern.clone()];
    if let Some(includes) = &params.includes {
        include_patterns.extend(includes.clone());
    }
    let include_set = compile_glob_set(&include_patterns)?;

    // Compile exclude globs
    let exclude_set = match &params.excludes {
        Some(excludes) => compile_glob_set(excludes)?,
        None => GlobSet::empty(),
    };

    let mut results = Vec::new();

    for file in files {
        // Compute relative path
        let relative_path = match file.strip_prefix(search_dir) {
            Ok(p) => p,
            Err(_) => continue, // Skip files not under the search directory
        };

        // Check depth
        if let Some(max_depth) = params.max_depth {
            let depth = relative_path.components().count() as i32;
            if depth > max_depth {
                continue;
            }
        }

        // Convert relative path to a string with forward slashes for glob matching
        let rel_path_str = relative_path.to_string_lossy().replace("\\", "/");

        // Check include patterns
        if !include_set.is_match(&rel_path_str) {
            continue;
        }

        // Check exclude patterns
        if exclude_set.is_match(&rel_path_str) {
            continue;
        }

        // Collect the result
        results.push(file.clone());
    }

    Ok(results)
}

fn compile_glob_set(patterns: &[String]) -> Result<GlobSet, Box<dyn Error>> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(pattern)?;
        builder.add(glob);
    }
    Ok(builder.build()?)
}

#[tokio::main]
async fn main() {
    let directory_path = Path::new("/Users/skcd/scratch/sidecar");
    let files = list_files(&directory_path, true, 10_000).0;
    let find_parameters = FindParams {
        search_directory: "/Users/skcd/scratch/sidecar".to_owned(),
        pattern: "**/client*.rs".to_owned(),
        includes: None,
        excludes: None,
        max_depth: None,
        file_type: None,
    };
    let found_files = find_files(&find_parameters, files.as_slice());
    println!("{:?}", found_files);
}
