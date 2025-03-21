use once_cell::sync::Lazy;
use regex::Regex;
use smallvec::SmallVec;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub trait FileSource {
    fn len(&self) -> usize;
}

impl RepoDirectoryEntry {
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::File(file) => Some(file.path.as_str()),
            Self::Dir(dir) => Some(dir.path.as_str()),
            Self::Other => None,
        }
    }

    pub fn buffer(&self) -> Option<&str> {
        match self {
            Self::File(file) => Some(file.buffer.as_str()),
            _ => None,
        }
    }

    pub fn is_file(&self) -> bool {
        matches!(self, RepoDirectoryEntry::File(_))
    }
}

#[derive(Debug)]
pub struct RepositoryDirectory {
    pub path: String,
}

#[derive(Debug)]
pub struct RepositoryFile {
    pub path: String,
    pub pathbuf: PathBuf,
    pub buffer: String,
}

#[derive(Debug)]
pub enum RepoDirectoryEntry {
    Dir(RepositoryDirectory),
    File(RepositoryFile),
    Other,
}

#[derive(Hash, Eq, PartialEq)]
pub enum FileType {
    File,
    Directory,
    NotTracked,
}

pub fn should_index_entry(de: &ignore::DirEntry) -> bool {
    should_index(&de.path())
}

pub fn should_index<P: AsRef<Path>>(p: &P) -> bool {
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
        "ttf", "woff2", "fnt", "fon", "otf",
        // documents
        "pdf", "ps", "doc", "dot", "docx", "dotx", "xls", "xlsx", "xlt", "odt", "ott", "ods", "ots", "dvi", "pcl",
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
    ];

    let Some(ext) = path.extension() else {
        return true;
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
