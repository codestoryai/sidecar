-- Add migration script here
CREATE TABLE code_snippet_cache (
    tantivy_cache_key TEXT PRIMARY KEY NOT NULL,
    repo_ref TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    file_path TEXT NOT NULL,
    commit_hash TEXT NOT NULL
)