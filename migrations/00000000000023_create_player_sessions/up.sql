CREATE TABLE player_sessions (
    token_hash TEXT PRIMARY KEY NOT NULL,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    session_version BIGINT NOT NULL,
    expires_at BIGINT NOT NULL
);
CREATE INDEX player_sessions_user_id ON player_sessions(user_id);
