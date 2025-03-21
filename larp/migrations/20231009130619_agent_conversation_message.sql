-- Add migration script here
CREATE TABLE agent_conversation_message (
    message_id TEXT NOT NULL PRIMARY KEY,
    query TEXT NOT NULL,
    answer TEXT,
    created_at INTEGER NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_updated INTEGER NOT NULL,
    session_id TEXT NOT NULL,
    -- Now we want to dump these fields as json so we can load them up when we
    -- need
    steps_taken TEXT NOT NULL,
    agent_state TEXT NOT NULL,
    file_paths TEXT NOT NULL,
    code_spans TEXT NOT NULL,
    user_selected_code_span TEXT NOT NULL,
    open_files TEXT NOT NULL,
    conversation_state TEXT NOT NULL,
    repo_ref TEXT NOT NULL,
    generated_answer_context TEXT NULL,
    user_context TEXT
)