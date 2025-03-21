-- Add migration script here
CREATE TABLE openai_llm_data (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id Text,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    event_type TEXT,
    prompt TEXT,
    response TEXT
);
