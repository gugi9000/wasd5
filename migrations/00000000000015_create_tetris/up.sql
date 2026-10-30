CREATE TABLE tetris_runs (
    id TEXT PRIMARY KEY NOT NULL,
    user_id INTEGER REFERENCES users(id),
    created_at INTEGER NOT NULL,
    submitted_at INTEGER
);

CREATE INDEX tetris_runs_created_at ON tetris_runs(created_at);

CREATE TABLE tetris_scores (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    run_id TEXT NOT NULL UNIQUE REFERENCES tetris_runs(id),
    player_name TEXT NOT NULL,
    user_id INTEGER REFERENCES users(id),
    score BIGINT NOT NULL,
    lines INTEGER NOT NULL,
    level INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX tetris_scores_leaderboard ON tetris_scores(score DESC, created_at ASC);
