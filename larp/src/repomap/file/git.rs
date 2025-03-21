use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
};

use once_cell::sync::Lazy;
use regex::Regex;
use smallvec::SmallVec;

use super::errors::FileError;

#[derive(Hash, Eq, PartialEq)]
enum FileType {
    File,
    Directory,
    NotTracked,
}

pub struct GitWalker {}

impl GitWalker {
    pub fn read_files(&self, directory: &Path) -> Result<HashMap<String, Vec<u8>>, FileError> {
        let git = gix::open::Options::isolated()
            .filter_config_section(|_| false)
            .open(directory);

        if let Err(_) = git {
            // load from local fs using recursive calls
            let mut files = HashMap::new();
            println!(
                "git_walker::reading_files_locally::without_git({:?})",
                &directory
            );
            let _ = self.get_files_recursive(directory, &mut files);
            return Ok(files);
        }

        let git = git.expect("if let Err to hold");
        let local_git = git.to_thread_local();
        let mut head = local_git.head().expect("get this");
        let trees = vec![(
            true,
            "HEAD".to_owned(),
            head.peel_to_commit_in_place()
                .expect("to work")
                .tree()
                .expect("to work"),
        )];

        let directory_ref: &Path = directory.as_ref();

        let entries = trees
            .into_iter()
            .flat_map(|(is_head, branch, tree)| {
                let files = tree.traverse().breadthfirst.files().unwrap().into_iter();

                files
                    .map(move |entry| {
                        let strpath = String::from_utf8_lossy(entry.filepath.as_ref());
                        let full_path = directory_ref.join(strpath.as_ref());
                        (
                            is_head,
                            branch.clone(),
                            full_path.to_string_lossy().to_string(),
                            entry.mode,
                            entry.oid,
                        )
                    })
                    .filter(|(_, _, path, _, _)| should_index(path))
            })
            .fold(
                HashMap::new(),
                |mut acc, (is_head, branch, file, mode, oid)| {
                    let kind = if mode.is_tree() {
                        FileType::Directory
                    } else if mode.is_blob() {
                        FileType::File
                    } else {
                        FileType::NotTracked
                    };

                    let branches = acc.entry((file, kind, oid)).or_insert_with(BTreeSet::new);
                    if is_head {
                        branches.insert("HEAD".to_string());
                    }

                    branches.insert(branch);
                    acc
                },
            );

        // Now we want to read the content of all the files in parallel
        {
            let start_time = std::time::Instant::now();
            use rayon::prelude::*;
            let values = entries
                .into_par_iter()
                .filter_map(|((path, kind, oid), _)| {
                    let git = git.to_thread_local();
                    let Ok(Some(_)) = git.try_find_object(oid) else {
                        return None;
                    };

                    let entry = match kind {
                        FileType::File => match std::fs::read(path.to_owned()) {
                            Ok(content) => Some((path, content)),
                            Err(_) => None,
                        },
                        FileType::Directory => None,
                        FileType::NotTracked => None,
                    };
                    entry
                })
                .collect::<HashMap<_, _>>();
            println!("Time taken: {}", start_time.elapsed().as_micros());
            Ok(values)
        }
    }

    pub fn find_file(&self, directory: &Path, target: &str) -> Option<PathBuf> {
        let files = self.read_files(directory).ok()?;
        files
            .keys()
            .find(|path| path.ends_with(target))
            .map(PathBuf::from)
    }

    fn get_files_recursive(
        &self,
        dir: &Path,
        files: &mut HashMap<String, Vec<u8>>,
    ) -> Result<(), FileError> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                let file_conetent = std::fs::read(path.to_owned())?;
                // Add the file content over here
                files.insert(
                    path.to_str().expect("to not fail").to_owned(),
                    file_conetent,
                );
            } else if path.is_dir() {
                self.get_files_recursive(&path, files)?;
            }
        }
        Ok(())
    }
}

fn should_index<P: AsRef<Path>>(p: &P) -> bool {
    let path = p.as_ref();

    if path.components().any(|c| c.as_os_str() == ".git") {
        return false;
    }

    #[rustfmt::skip]
    const EXT_BLACKLIST: &[&str] = &[
        // graphics
        "png", "jpg", "jpeg", "ico", "bmp", "bpg", "eps", "pcx", "ppm", "tga", "tiff", "wmf", "xpm",
        "svg", "riv",
        // fonts
        "ttf", "woff2", "fnt", "fon", "otf", "woff", "eot",
        // documents
        "pdf", "ps", "doc", "dot", "docx", "dotx", "xls", "xlsx", "xlt", "odt", "ott", "ods", "ots", "dvi", "pcl", "mo", "po",
        // media
        "mp3", "ogg", "ac3", "aac", "mod", "mp4", "mkv", "avi", "m4v", "mov", "flv",
        // compiled
        "jar", "pyc", "war", "ear",
        // compression
        "tar", "gz", "bz2", "xz", "7z", "bin", "apk", "deb", "rpm",
        // executable
        "com", "exe", "out", "coff", "obj", "dll", "app", "class",
        // misc.
        "log", "wad", "bsp", "bak", "sav", "dat", "lock",
        // dylib
        "dylib",
        // onnx file
        "onnx",
        // .so files
        "so",
    ];

    let Some(ext) = path.extension() else {
        // if we have no extension, then do not index
        // this gets rid of binary blobs
        return false;
    };

    let ext = ext.to_string_lossy();
    if EXT_BLACKLIST.contains(&&*ext) {
        return false;
    }

    static VENDOR_PATTERNS: Lazy<HashMap<&'static str, SmallVec<[Regex; 1]>>> = Lazy::new(|| {
        let patterns: &[(&[&str], &[&str])] = &[
            (
                &["go", "proto"],
                &["^(vendor|third_party)/.*\\.\\w+$", "\\w+\\.pb\\.go$"],
            ),
            (
                &["js", "jsx", "ts", "tsx", "css", "md", "json", "txt", "conf"],
                &["^(node_modules|vendor|dist)/.*\\.\\w+$"],
            ),
        ];

        patterns
            .iter()
            .flat_map(|(exts, rxs)| exts.iter().map(move |&e| (e, rxs)))
            .map(|(ext, rxs)| {
                let regexes = rxs
                    .iter()
                    .filter_map(|source| match Regex::new(source) {
                        Ok(r) => Some(r),
                        Err(_) => None,
                    })
                    .collect();

                (ext, regexes)
            })
            .collect()
    });

    match VENDOR_PATTERNS.get(&*ext) {
        None => true,
        Some(rxs) => !rxs.iter().any(|r| r.is_match(&path.to_string_lossy())),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::should_index;

    #[test]
    fn test_parsing_bin_file() {
        let path = Path::new("/Users/skcd/scratch/sidecar/sidecar/qdrant/qdrant_mac");
        assert!(!should_index(&path));
    }

    #[test]
    fn test_onxx_file() {
        let path = Path::new(
            "/Users/skcd/scratch/sidecar/sidecar/models/all-MiniLM-L6-v2/onnx/model.onnx",
        );
        assert!(!should_index(&path));
    }
}
