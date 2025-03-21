/*
We are going to look at the file system and iterate along all the files
which might be present here
*/

use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
};

use anyhow::Result;
use gix::ThreadSafeRepository;
use ignore::WalkBuilder;
use regex::RegexSet;

use super::{
    iterator::{should_index, should_index_entry, FileSource, FileType},
    types::RepoRef,
};

pub const AVG_LINE_LEN: u64 = 30;
pub const MAX_LINE_COUNT: u64 = 20000;
pub const MAX_FILE_LEN: u64 = AVG_LINE_LEN * MAX_LINE_COUNT;

pub struct FileWalker {
    pub file_list: Vec<PathBuf>,
}

impl FileWalker {
    pub fn index_directory(dir: impl AsRef<Path>) -> FileWalker {
        // note: this WILL observe .gitignore files for the respective repos.
        let walker = WalkBuilder::new(&dir)
            .standard_filters(true)
            .hidden(false)
            .filter_entry(should_index_entry)
            .build();

        let file_list = walker
            .filter_map(|de| match de {
                Ok(de) => Some(de),
                Err(_) => None,
            })
            // Preliminarily ignore files that are very large, without reading the contents.
            .filter(|de| matches!(de.metadata(), Ok(meta) if meta.len() < MAX_FILE_LEN))
            .filter_map(|de| std::fs::canonicalize(de.into_path()).ok())
            .collect();

        Self { file_list }
    }
}

impl FileSource for FileWalker {
    fn len(&self) -> usize {
        self.file_list.len()
    }
}

fn human_readable_branch_name(r: &gix::Reference<'_>) -> String {
    use gix::bstr::ByteSlice;
    r.name().shorten().to_str_lossy().to_string()
}

pub enum BranchFilter {
    All,
    Head,
    Select(RegexSet),
}

impl BranchFilter {
    fn filter(&self, is_head: bool, branch: &str) -> bool {
        match self {
            BranchFilter::All => true,
            BranchFilter::Select(patterns) => is_head || patterns.is_match(branch),
            BranchFilter::Head => is_head,
        }
    }
}

impl Default for BranchFilter {
    fn default() -> Self {
        Self::Head
    }
}

pub struct GitWalker {
    _git: ThreadSafeRepository,
    entries: HashMap<(String, FileType, gix::ObjectId), BTreeSet<String>>,
}

impl GitWalker {
    pub fn open_repository(
        reporef: &RepoRef,
        dir: impl AsRef<Path>,
        filter: impl Into<Option<BranchFilter>>,
    ) -> Result<Self> {
        let root_dir = dir.as_ref();
        let branches = filter.into().unwrap_or_default();
        let git = gix::open::Options::isolated()
            .filter_config_section(|_| false)
            .open(dir.as_ref())?;

        let local_git = git.to_thread_local();
        let mut head = local_git.head()?;

        // HEAD name needs to be pinned to the remote pointer
        //
        // Otherwise the local branch will never advance to the
        // remote's branch ref
        //
        // The easiest here is to check by name, and assume the
        // default remote is `origin`, since we don't configure it
        // otherwise.
        let head_name = head.clone().try_into_referent().map(|r| {
            if reporef.is_local() {
                human_readable_branch_name(&r)
            } else {
                format!("origin/{}", human_readable_branch_name(&r))
            }
        });

        let refs = local_git.references()?;
        let trees = if head_name.is_none() && matches!(branches, BranchFilter::Head) {
            // the current checkout is not a branch, so HEAD will not
            // point to a real reference.
            vec![(
                true,
                "HEAD".to_string(),
                head.peel_to_commit_in_place()?.tree()?,
            )]
        } else {
            refs.all()?
                .filter_map(Result::ok)
                // Check if it's HEAD
                // Normalize the name of the branch for further steps
                //
                .map(|r| {
                    let name = human_readable_branch_name(&r);
                    (
                        head_name
                            .as_ref()
                            .map(|head| head == &name)
                            .unwrap_or_default(),
                        name,
                        r,
                    )
                })
                .filter(|(_, name, _)| {
                    if reporef.is_local() {
                        true
                    } else {
                        // Only consider remote branches
                        //
                        name.starts_with("origin/")
                    }
                })
                // Apply branch filters, along whether it's HEAD
                //
                .filter(|(is_head, name, _)| branches.filter(*is_head, name))
                .filter_map(|(is_head, branch, r)| -> Option<_> {
                    Some((
                        is_head,
                        branch,
                        r.into_fully_peeled_id()
                            .ok()?
                            .object()
                            .ok()?
                            .peel_to_tree()
                            .ok()?,
                    ))
                })
                .collect()
        };

        let entries = trees
            .into_iter()
            .flat_map(|(is_head, branch, tree)| {
                let files = tree.traverse().breadthfirst.files().unwrap().into_iter();

                files
                    .map(move |entry| {
                        let strpath = String::from_utf8_lossy(entry.filepath.as_ref());
                        let full_path = root_dir.join(strpath.as_ref());
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

        Ok(Self { _git: git, entries })
    }
}

impl FileSource for GitWalker {
    fn len(&self) -> usize {
        self.entries.len()
    }
}
