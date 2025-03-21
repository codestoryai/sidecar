-- Add migration script here
CREATE TABLE chunk_cache (
    repo_ref TEXT NOT NULL,
    file_cache_key TEXT NOT NULL,
    file_path TEXT,
    chunk_hash TEXT NOT NULL,
    commit_hash TEXT NOT NULL
);

-- Add columns here to the file_cache table
ALTER TABLE file_cache ADD COLUMN commit_hash TEXT NOT NULL;
ALTER TABLE file_cache ADD COLUMN file_path TEXT NOT NULL;
ALTER TABLE file_cache ADD COLUMN file_content_hash TEXT NOT NULL;
ALTER TABLE file_cache ADD COLUMN semantic_search_hash TEXT NOT NULL;