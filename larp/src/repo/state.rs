use clap::Args;
use ignore::WalkBuilder;
use rand::Rng;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;
use std::fmt::Debug;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use super::types::{RepoRef, Repository};

pub type RepositoryPool = Arc<scc::HashMap<RepoRef, Repository>>;

// ❓ can't understand where this comes from

include!(concat!(env!("OUT_DIR"), "/version_hash.rs"));

#[derive(thiserror::Error, Debug)]
pub enum RepoError {
    #[error("No source file found")]
    NoSourceGiven,
    #[error("local repository must have an absolute path")]
    NonAbsoluteLocal,
    #[error("paths can't contain `..` or `.`")]
    InvalidPath,
    #[error("indexing error")]
    Anyhow {
        #[from]
        error: anyhow::Error,
    },
    #[error("IO error: {error}")]
    IO {
        #[from]
        error: std::io::Error,
    },
    #[error("serde error: {error}")]
    Decode {
        #[from]
        error: serde_json::Error,
    },
    #[error("Invalid backend found")]
    InvalidBackend,
}

#[derive(Serialize, Deserialize, Args, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct StateSource {
    #[serde(default)]
    directory: Option<PathBuf>,
    // state file where we store the status of each repository
    #[serde(default)]
    repo_state_file: Option<PathBuf>,

    #[serde(default)]
    binary_version_hash: Option<PathBuf>,
}

impl StateSource {
    pub fn set_default_dir(&mut self, dir: &Path) {
        std::fs::create_dir_all(dir).expect("the index folder can't be created");

        self.repo_state_file
            .get_or_insert_with(|| dir.join("repo_state"));

        self.binary_version_hash
            .get_or_insert_with(|| dir.join("binary_version_hash"));

        self.directory.get_or_insert_with(|| {
            let target = dir.join("local_cache");
            std::fs::create_dir_all(&target).unwrap();

            target
        });
    }

    pub fn initialize_pool(&self) -> Result<RepositoryPool, RepoError> {
        use std::fs::canonicalize;

        match (self.directory.as_ref(), self.repo_state_file.as_ref()) {
            // Load RepositoryPool from path
            (None, Some(path)) => read_file_or_default(path).map(Arc::new),

            // Initialize RepositoryPool from repos under `root`
            (Some(root), None) => {
                let out = scc::HashMap::default();
                for reporef in gather_repo_roots(root, None) {
                    let repo = Repository::local_from(&reporef);
                    _ = out.insert(reporef, repo);
                }

                let pool = Arc::new(out);
                self.save_pool(pool.clone())?;
                Ok(pool)
            }
            // Update RepositoryPool with repos under `root`
            (Some(root), Some(path)) => {
                // Load RepositoryPool from path
                let state: RepositoryPool = Arc::new(read_file_or_default(path)?);

                let current_repos = gather_repo_roots(root, None);
                let root = canonicalize(root)?;

                // mark repositories from the index which are no longer present
                state.for_each(|k, repo| {
                    if let Some(path) = k.local_path() {
                        // Clippy suggestion causes the code to break, revisit after 1.66
                        if path.starts_with(&root) && !current_repos.contains(k) {
                            repo.mark_removed();
                        }
                    }

                    // in case the app terminated during indexing, make sure to re-queue it
                    if !repo.sync_status.indexable() {
                        repo.mark_queued();
                    }
                });

                // then add anything new that's appeared
                let mut per_path = std::collections::HashMap::new();
                state.scan(|k, v| {
                    per_path.insert(v.disk_path.to_string_lossy().to_string(), k.clone());
                });

                for reporef in current_repos {
                    // skip all paths that are already in the index,
                    // bearing in mind they may not be local repos
                    if per_path.contains_key(reporef.name()) {
                        continue;
                    }

                    state
                        .entry(reporef.to_owned())
                        .or_insert_with(|| Repository::local_from(&reporef));
                }

                self.save_pool(state.clone())?;
                Ok(state)
            }
            (None, None) => Err(RepoError::NoSourceGiven),
        }
    }

    pub fn save_pool(&self, pool: RepositoryPool) -> Result<(), RepoError> {
        match self.repo_state_file {
            None => Err(RepoError::NoSourceGiven),
            Some(ref path) => pretty_write_file(path, pool.as_ref()),
        }
    }

    pub fn index_version_mismatch(&self) -> bool {
        let current: String =
            read_file_or_default(self.binary_version_hash.as_ref().unwrap()).unwrap();

        !current.is_empty() && current != BINARY_VERSION_HASH
    }

    pub fn save_index_version(&self) -> Result<(), RepoError> {
        pretty_write_file(
            self.binary_version_hash.as_ref().unwrap(),
            BINARY_VERSION_HASH,
        )
    }
}

pub fn read_file_or_default<T: Default + DeserializeOwned>(
    path: &Path,
) -> anyhow::Result<T, RepoError> {
    if !path.exists() {
        return Ok(Default::default());
    }

    let file = std::fs::File::open(path)?;
    Ok(serde_json::from_reader::<_, T>(file)?)
}

fn gather_repo_roots(
    path: impl AsRef<Path>,
    exclude: Option<PathBuf>,
) -> std::collections::HashSet<RepoRef> {
    const RECOGNIZED_VCS_DIRS: &[&str] = &[".git"];

    let repos = Arc::new(scc::HashSet::new());

    WalkBuilder::new(path)
        .ignore(true)
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false)
        .filter_entry(move |entry| {
            exclude
                .as_ref()
                .and_then(|path| {
                    std::fs::canonicalize(entry.path())
                        .ok()
                        .map(|canonical_path| !canonical_path.starts_with(path))
                })
                .unwrap_or(true)
        })
        .build_parallel()
        .run(|| {
            let repos = repos.clone();
            Box::new(move |entry| {
                use ignore::WalkState::*;

                let Ok(de) = entry else {
                    return Continue;
                };

                let Some(ft) = de.file_type() else {
                    return Continue;
                };

                if ft.is_dir()
                    && RECOGNIZED_VCS_DIRS.contains(&de.file_name().to_string_lossy().as_ref())
                {
                    _ = repos.insert(RepoRef::from(
                        &std::fs::canonicalize(
                            de.path().parent().expect("/ shouldn't be a git repo"),
                        )
                        .expect("repo root is both a dir and exists"),
                    ));

                    // we've already taken this repo, do not search subdirectories
                    return Skip;
                }

                Continue
            })
        });

    let mut output = std::collections::HashSet::default();
    repos.scan(|entry| {
        output.insert(entry.clone());
    });

    output
}

pub fn pretty_write_file<T: Serialize + ?Sized>(
    path: impl AsRef<Path>,
    val: &T,
) -> Result<(), RepoError> {
    let (tmpfile, file) = {
        let mut tries = 0;
        const MAX_TRIES: u8 = 10;

        loop {
            let tmpfile = path
                .as_ref()
                .with_extension(format!("new.{}", rand::thread_rng().gen_range(0..=99999)));

            let file = std::fs::File::options()
                .write(true)
                .create_new(true)
                .open(&tmpfile);

            match file {
                Ok(f) => break (tmpfile, f),
                Err(e) => {
                    if tries == MAX_TRIES {
                        // this will always be an error
                        // would have broken just before
                        return Err(e.into());
                    }

                    tries += 1;
                }
            }
        }
    };

    serde_json::to_writer_pretty(file, val)?;
    std::fs::rename(tmpfile, path)?;
    Ok(())
}
