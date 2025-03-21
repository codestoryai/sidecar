-- Add migration script here
CREATE TABLE file_cache (
    tantivy_cache_key TEXT PRIMARY KEY NOT NULL,
    repo_ref TEXT NOT NULL
);