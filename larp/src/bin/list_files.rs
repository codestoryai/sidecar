/// This contains the binary responsible for running the agents as a farm
/// Dead simple where the inputs are the input to the git repository containing the input
/// and the problem statement, keeping it super simple and limited
use clap::Parser;

use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
};

use ignore::WalkBuilder;

/// Define the command-line arguments
#[derive(Parser, Debug)]
#[command(
    author = "skcd",
    version = "1.0",
    about = "Agent binary sidecar runner"
)]
struct CliArgs {
    /// Git directory name
    #[arg(long)]
    directory_path: PathBuf,

    #[arg(long)]
    recursive: bool,
}

/// Handwaving this number into existence, no promises offered here and this is just
/// a rough estimation of the context window
const FILES_LIMIT: usize = 250;

fn is_root_or_home(dir_path: &Path) -> bool {
    // Get root directory
    let root_dir = if cfg!(windows) {
        dir_path
            .components()
            .next()
            .map(|c| PathBuf::from(c.as_os_str()))
    } else {
        Some(PathBuf::from("/"))
    };
    let is_root = root_dir.map_or(false, |r| dir_path == r.as_path());

    // Get home directory
    let home_dir = dirs::home_dir();
    let is_home = home_dir.map_or(false, |h| dir_path == h.as_path());

    is_root || is_home
}

pub fn list_files(dir_path: &Path, recursive: bool, limit: usize) -> (Vec<PathBuf>, bool) {
    // Check if dir_path is root or home directory
    if is_root_or_home(dir_path) {
        return (vec![dir_path.to_path_buf()], false);
    }

    let mut results = Vec::new();
    let mut limit_reached = false;

    // Start time for timeout
    let start_time = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(10); // Timeout after 10 seconds

    // BFS traversal
    let mut queue = VecDeque::new();
    queue.push_back(dir_path.to_path_buf());

    // Keep track of visited directories to avoid loops
    let mut visited_dirs = HashSet::new();

    // Define the ignore list
    let ignore_names: HashSet<&str> = [
        // js/ts pulled in files
        "node_modules",
        // cache from python
        "__pycache__",
        // env and venv belong to python
        "env",
        "venv",
        // rust like garbage which we don't want to look at
        "target",
        ".target",
        "build",
        // output directories for compiled code
        "dist",
        "out",
        "bundle",
        "vendor",
        // ignore tmp and temp which are common placeholders for temporary files
        "tmp",
        "temp",
        "deps",
        "pkg",
    ]
    .iter()
    .cloned()
    .collect();

    while let Some(current_dir) = queue.pop_front() {
        // Check for timeout
        if start_time.elapsed() > timeout {
            eprintln!("Traversal timed out, returning partial results");
            break;
        }

        // Check if we've reached the limit
        if results.len() >= limit {
            limit_reached = true;
            break;
        }

        // Check if we have visited this directory before
        if !visited_dirs.insert(current_dir.clone()) {
            continue; // Skip already visited directories
        }

        // Build a walker for the current directory
        let mut builder = WalkBuilder::new(&current_dir);
        builder
            // Apply .gitignore and other standard ignore files
            .standard_filters(true)
            // Do not ignore hidden files/directories
            .hidden(false)
            // Only immediate entries
            .max_depth(Some(1))
            // Follow symbolic links
            .follow_links(true);

        // For non-recursive traversal, disable standard filters
        if !recursive {
            builder.standard_filters(false);
        }

        // Clone ignore_names for the closure
        let ignore_names = ignore_names.clone();

        // Set filter_entry to skip ignored directories and files
        builder.filter_entry(move |entry| {
            if let Some(name) = entry.file_name().to_str() {
                // Skip ignored names
                if ignore_names.contains(name) {
                    return false;
                }
                // Do not traverse into hidden directories but include them in the results
                if entry.depth() > 0 && name.starts_with('.') {
                    if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                        // Skip traversal into hidden directories
                        return false;
                    }
                }
            }
            true
        });

        let walk = builder.build();

        for result in walk {
            match result {
                Ok(entry) => {
                    let path = entry.path().to_path_buf();
                    // Skip the directory itself
                    if path == current_dir {
                        continue;
                    }
                    // Check if we've reached the limit
                    if results.len() >= limit {
                        limit_reached = true;
                        break;
                    }
                    results.push(path.clone());
                    // If recursive and it's a directory, enqueue it
                    if recursive && path.is_dir() {
                        queue.push_back(path);
                    }
                }
                Err(err) => eprintln!("Error: {}", err),
            }
        }
        if limit_reached {
            break;
        }
    }
    (results, limit_reached)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = CliArgs::parse();
    let output = list_files(&args.directory_path, args.recursive, FILES_LIMIT);
    output.0.into_iter().for_each(|path| {
        // create the string over here
        println!("{}", path.as_os_str().to_string_lossy().to_string());
    });
    Ok(())
}
