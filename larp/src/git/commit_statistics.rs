use std::collections::{HashMap, HashSet};

use crate::{db::sqlite::SqlDb, repo::types::RepoRef};
use anyhow::Context;
use gix::{
    bstr::ByteSlice,
    diff::blob::{sink::Counter, UnifiedDiffBuilder},
    object::blob::diff::Platform,
    objs::tree::EntryMode,
    Commit, Id,
};
use sqlx::Sqlite;
use tracing::{debug, error};

const COMMIT_EXCLUDE_EXTENSIONS_FROM_DIFF: [&str; 5] = ["db", "png", "onnx", "dylib", "lock"];

#[derive(Debug, Default)]
pub struct CommitStatistics {
    author: Option<String>,
    file_insertions: i64,
    file_deletions: i64,
    title: String,
    body: Option<String>,
    git_diff: String,
    files_modified: HashSet<String>,
    line_insertions: u32,
    line_deletions: u32,
    commit_timestamp: i64,
    commit_hash: String,
    // This is the repo-reference which we will use to tag the repository
    // as unique
    repo_ref: String,
}

impl CommitStatistics {
    pub async fn cleanup_for_repo(
        reporef: RepoRef,
        tx: &mut sqlx::Transaction<'_, Sqlite>,
    ) -> anyhow::Result<()> {
        let repo_str = reporef.to_string();
        sqlx::query! {
            "DELETE FROM git_log_statistics \
            WHERE repo_ref = ?",
           repo_str,
        }
        .execute(&mut **tx)
        .await?;
        Ok(())
    }
}

struct GitCommitIterator<'a> {
    commit: Commit<'a>,
    parent: Option<Id<'a>>,
    repo_ref: &'a RepoRef,
}

#[derive(Debug)]
struct CommitError;

impl std::fmt::Display for CommitError {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unreachable!("commit error should not happen");
    }
}

impl std::error::Error for CommitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

impl<'a> Iterator for GitCommitIterator<'a> {
    type Item = CommitStatistics;

    fn next(&mut self) -> Option<Self::Item> {
        let Some(parent_id) = self.parent else {
            return None;
        };

        let parent_commit = parent_id.object().unwrap().into_commit();
        let commit_message = self
            .commit
            .message()
            .unwrap()
            .body()
            .map(|body| body.to_string());
        let commit_title = self.commit.message().unwrap().title.to_string();
        let mut commit_statistics = CommitStatistics {
            body: commit_message,
            title: commit_title,
            ..Default::default()
        };
        commit_statistics.author = self
            .commit
            .author()
            .map(|author| author.name.to_string())
            .ok();
        // This is the commit timestamp from the unix epoch
        commit_statistics.commit_timestamp = self.commit.time().unwrap().seconds;
        commit_statistics.commit_hash = self.commit.id().to_string();
        commit_statistics.repo_ref = self.repo_ref.to_string();

        _ = self
            .commit
            .tree()
            .unwrap()
            .changes()
            .unwrap()
            .track_path()
            .for_each_to_obtain_tree(&parent_commit.tree().unwrap(), |change| {
                let ext = change
                    .location
                    .to_path_lossy()
                    .extension()
                    .map(|ext| ext.to_string_lossy().to_string());

                let location = change.location.to_str_lossy();
                commit_statistics
                    .files_modified
                    .insert(location.to_string());

                match &change.event {
                    // We only want git blobs and nothing else
                    gix::object::tree::diff::change::Event::Addition { entry_mode, id }
                        if matches!(entry_mode, EntryMode::Blob) =>
                    {
                        commit_statistics.file_insertions += 1;
                        add_diff(
                            &location,
                            &ext.as_deref(),
                            "".into(),
                            id.object().unwrap().data.as_bstr().to_str_lossy(),
                            &mut commit_statistics,
                        );
                    }
                    gix::object::tree::diff::change::Event::Deletion { entry_mode, id }
                        if matches!(entry_mode, EntryMode::Blob) =>
                    {
                        commit_statistics.file_deletions += 1;
                        add_diff(
                            &location,
                            &ext.as_deref(),
                            id.object().unwrap().data.as_bstr().to_str_lossy(),
                            "".into(),
                            &mut commit_statistics,
                        );
                    }
                    gix::object::tree::diff::change::Event::Modification {
                        previous_entry_mode,
                        previous_id,
                        entry_mode,
                        id,
                    } if matches!(entry_mode, EntryMode::Blob)
                        && matches!(previous_entry_mode, EntryMode::Blob) =>
                    {
                        let platform = Platform::from_ids(previous_id, id).unwrap();
                        let old = platform.old.data.as_bstr().to_str_lossy();
                        let new = platform.new.data.as_bstr().to_str_lossy();
                        add_diff(&location, &ext.as_deref(), old, new, &mut commit_statistics);
                    }
                    gix::object::tree::diff::change::Event::Rewrite {
                        source_id,
                        entry_mode,
                        id,
                        ..
                    } if matches!(entry_mode, EntryMode::Blob) => {
                        let platform = Platform::from_ids(source_id, id).unwrap();
                        let old = platform.old.data.as_bstr().to_str_lossy();
                        let new = platform.new.data.as_bstr().to_str_lossy();
                        add_diff(&location, &ext.as_deref(), old, new, &mut commit_statistics);
                    }
                    _ => {}
                }

                Ok::<gix::object::tree::diff::Action, CommitError>(
                    gix::object::tree::diff::Action::Continue,
                )
            })
            .unwrap();

        self.commit = parent_commit;
        self.parent = self.commit.parent_ids().next();
        Some(commit_statistics)
    }
}

fn add_diff(
    location: &str,
    extension: &Option<&str>,
    old: std::borrow::Cow<'_, str>,
    new: std::borrow::Cow<'_, str>,
    commit_statistics: &mut CommitStatistics,
) {
    if extension
        .map(|extension| COMMIT_EXCLUDE_EXTENSIONS_FROM_DIFF.contains(&extension))
        .unwrap_or(false)
    {
        return;
    }
    let input = gix::diff::blob::intern::InternedInput::new(old.as_ref(), new.as_ref());
    commit_statistics.git_diff += &format!(
        r#"diff --git a/{location} b/{location}"
--- a/{location}
--- b/{location}
"#
    );
    let diff = gix::diff::blob::diff(
        gix::diff::blob::Algorithm::Histogram,
        &input,
        Counter::new(UnifiedDiffBuilder::new(&input)),
    );

    if let Some(_) = extension {
        // Here we have to guard against the extensions which we know we don't
        // care about
        commit_statistics.line_insertions += &diff.removals;
        commit_statistics.line_deletions += &diff.insertions;
    }

    commit_statistics.git_diff += diff.wrapped.as_str();
    commit_statistics.git_diff += "\n";
}

fn get_commit_statistics_for_local_checkout(
    repo_ref: RepoRef,
) -> anyhow::Result<Vec<CommitStatistics>> {
    // This only works for the local path right now, but thats fine
    let repo = gix::open(repo_ref.local_path().expect("local path to be present"))?;
    let head_commit = repo
        .head()
        .context("invalid branch name")?
        .into_fully_peeled_id()
        .context("git errors")?
        .context("git errors")?
        .object()
        .context("git errors")?
        .into_commit();
    let parent = head_commit.parent_ids().next();
    Ok(GitCommitIterator {
        commit: head_commit,
        parent,
        repo_ref: &repo_ref,
    }
    .take(300)
    .collect::<Vec<_>>())
}

// This is the main function which is exposed to the indexing backend, we are
// going to rely on it to get the statistical information about the various
// files and use that to power the cosine similarity
pub async fn git_commit_statistics(repo_ref: RepoRef, db: SqlDb) -> anyhow::Result<()> {
    // First we cleanup whatever is there in there about the repo
    let start_time = std::time::Instant::now();
    debug!(
        "getting git commit statistics for repo: {}",
        repo_ref.to_string()
    );
    let commit_statistics = {
        let cloned_repo_ref = repo_ref.clone();
        tokio::task::spawn_blocking(|| get_commit_statistics_for_local_checkout(cloned_repo_ref))
            .await
            .context("tokio::thread error")?
    }
    .context("commit_fetch failed")?;
    debug!(
        "finished git commit statistics for repo: {}, took time: {}, found: {}",
        repo_ref.to_string(),
        start_time.elapsed().as_secs(),
        commit_statistics.len(),
    );

    // start a new transaction right now
    let mut tx = db.begin().await?;
    CommitStatistics::cleanup_for_repo(repo_ref.clone(), &mut tx).await?;

    // First insert all the commit statistics to the sqlite db
    // we do this one after the other because of the way transactions work
    for commit_statistic in commit_statistics.iter() {
        let repo_str = repo_ref.to_string();
        let files_modified_list = commit_statistic
            .files_modified
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let _ = sqlx::query! {
            "insert into git_log_statistics (repo_ref, commit_hash, author_email, commit_timestamp, files_changed, title, body, lines_insertions, lines_deletions, git_diff, file_insertions, file_deletions) \
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            repo_str,
            commit_statistic.commit_hash,
            commit_statistic.author,
            commit_statistic.commit_timestamp,
            files_modified_list,
            commit_statistic.title,
            commit_statistic.body,
            commit_statistic.line_insertions,
            commit_statistic.line_deletions,
            commit_statistic.git_diff,
            commit_statistic.file_insertions,
            commit_statistic.file_deletions,
        }.execute(&mut *tx).await?;
    }

    // Second push the file statistics for each file to the db
    // we do this one at a time again because of the way transactions work
    for commit_statistic in commit_statistics.into_iter() {
        let repo_str = repo_ref.to_string();
        for file_path in commit_statistic.files_modified.iter() {
            sqlx::query! {
                "insert into file_git_commit_statistics (repo_ref, file_path, commit_hash, commit_timestamp) \
                VALUES (?, ?, ?, ?)",
                repo_str, file_path, commit_statistic.commit_hash, commit_statistic.commit_timestamp,
            }.execute(&mut *tx).await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct GitLogScore {
    pub repo_ref: RepoRef,
    pub file_to_score: HashMap<String, f32>,
}

#[derive(Debug, Clone)]
pub struct GitLogFileInformation {
    file_path: String,
    commit_hash: String,
    commit_timestamp: i64,
}

impl GitLogFileInformation {
    pub fn commit_hash(&self) -> &str {
        &self.commit_hash
    }
}

impl GitLogScore {
    pub fn get_score_for_file(&self, relative_file_path: &str) -> f32 {
        // If we don't have data about the file we should just mark it with the
        // lowest score since it's probably an older file or not relevant given
        // we are querying the last 300 commits
        // If it's not present we just return 1/len(files_to_score) as the weight here
        self.file_to_score
            .get(relative_file_path)
            .map(|v| v.clone())
            .unwrap_or((1.0 as f32) / (self.file_to_score.len() as f32))
    }

    pub async fn generate_git_log_score(repo_ref: RepoRef, db: SqlDb) -> Self {
        // First we want to load all the file statistics for the repo which are
        // located in the git-log
        let repo_ref_str = repo_ref.to_string();
        let results = sqlx::query_as! {
            GitLogFileInformation,
            "SELECT file_path, commit_hash, commit_timestamp FROM file_git_commit_statistics WHERE repo_ref = ?",
            repo_ref_str,
        }.fetch_all(db.as_ref()).await.context("failed to fetch from sql db");

        if let Err(e) = results {
            // If there is an error, return the empty module
            error!("failed to fetch from sql db: {}", e);
            return Self {
                repo_ref: repo_ref.clone(),
                file_to_score: HashMap::new(),
            };
        }

        let results_vec = results.expect("err check above to work");
        let mut num_commits_for_file: HashMap<String, usize> = HashMap::new();
        let mut last_commit_timestamp_for_file: HashMap<String, i64> = HashMap::new();
        for git_file_statistic in results_vec.into_iter() {
            let file_path = git_file_statistic.file_path;
            let commit_timestamp = git_file_statistic.commit_timestamp;
            // update the commit count for the file
            num_commits_for_file
                .entry(file_path.clone())
                .and_modify(|count| *count += 1)
                .or_insert(1);
            // Now we check the timestamp and keep the latest one
            last_commit_timestamp_for_file
                .entry(file_path.clone())
                .and_modify(|timestamp| {
                    if *timestamp < commit_timestamp {
                        *timestamp = commit_timestamp;
                    }
                })
                .or_insert(commit_timestamp);
        }

        // Now we want to convert the values into a percentile value so its relative
        // to the total score
        let commit_score_for_file = get_sorted_position_as_score(num_commits_for_file);
        let last_commit_timestamp_score_for_file =
            get_sorted_position_as_score(last_commit_timestamp_for_file);

        // Now we try to get the combined score for all the files
        let files = commit_score_for_file.keys().collect::<HashSet<_>>();
        // Now go over both generated scores and just add them up, if its not
        // present use the 1 / len(files) as the score
        let mut file_to_score: HashMap<String, usize> = HashMap::new();
        for file in files.iter() {
            let commit_score = commit_score_for_file
                .get(*file)
                .map(|v| v.clone())
                .unwrap_or(1);
            let last_commit_timestamp_score = last_commit_timestamp_score_for_file
                .get(*file)
                .map(|v| v.clone())
                .unwrap_or(1);
            // The 1 here is the score coming from the line count
            file_to_score.insert(
                file.to_string(),
                1 + commit_score + last_commit_timestamp_score,
            );
        }

        // Now we sort the score and then make it relative with position / len(files)
        // which gives us the percentile score
        Self {
            repo_ref: repo_ref.clone(),
            file_to_score: get_sorted_position_as_score(file_to_score)
                .into_iter()
                .map(|(key, value)| (key, (value as f32 / files.len() as f32)))
                .collect(),
        }
    }
}

fn get_sorted_position_as_score<T: std::clone::Clone + Ord + std::hash::Hash>(
    hashed_values: HashMap<String, T>,
) -> HashMap<String, usize> {
    let mut values: Vec<T> = hashed_values
        .values()
        .map(|v| v.clone())
        .into_iter()
        .collect();
    // Now that values are sorted we can go about creating the percentile score
    values.sort();
    let hashed_values_percentile: HashMap<T, usize> = values
        .into_iter()
        .enumerate()
        .map(|(index, value)| (value, index))
        .collect();
    // Now we assign the percentile score to the initial hashed values
    hashed_values
        .into_iter()
        .map(|(key, value)| {
            (
                key,
                hashed_values_percentile
                    .get(&value)
                    .map(|v| v.clone())
                    .expect("value should be present"),
            )
        })
        .collect()
}
