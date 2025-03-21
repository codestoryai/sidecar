-- Add migration script here
CREATE TABLE git_log_statistics (
    repo_ref TEXT NOT NULL,
    commit_hash TEXT NOT NULL,
    author_email TEXT,
    -- This will be in unix timestamp after epoch
    commit_timestamp INTEGER NOT NULL,
    -- This is saved as a list of strings which are joined by a delimiter which
    -- is a , so we can parse it easily
    files_changed TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    lines_insertions INTEGER NOT NULL,
    lines_deletions INTEGER NOT NULL,
    git_diff TEXT NOT NULL,
    file_insertions INTEGER NOT NULL,
    file_deletions INTEGER NOT NULL,
    PRIMARY KEY (repo_ref, commit_hash)
);

CREATE TABLE file_git_commit_statistics (
    repo_ref TEXT NOT NULL,
    -- This is always relative to the repo root
    file_path TEXT NOT NULL,
    commit_hash TEXT NOT NULL,
    commit_timestamp INTEGER NOT NULL,
    -- The primary key here is on top of the repo_ref and the
    -- file_path as well, so we can get all the commits it is part
    --- of
    PRIMARY KEY (repo_ref, file_path, commit_hash)
);