CREATE TABLE minesweeper_games (
    id TEXT PRIMARY KEY NOT NULL,
    mode TEXT NOT NULL,
    rows INTEGER NOT NULL,
    board_columns INTEGER NOT NULL,
    mine_count INTEGER NOT NULL,
    mines TEXT NOT NULL,
    revealed TEXT NOT NULL,
    flags TEXT NOT NULL,
    user_id INTEGER REFERENCES users(id),
    started_at INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'playing',
    elapsed_ms INTEGER
);

CREATE INDEX minesweeper_games_started_at ON minesweeper_games(started_at);

CREATE TABLE minesweeper_scores (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    game_id TEXT NOT NULL UNIQUE,
    mode TEXT NOT NULL,
    player_name TEXT NOT NULL,
    user_id INTEGER REFERENCES users(id),
    elapsed_ms INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX minesweeper_scores_leaderboard
    ON minesweeper_scores(mode, elapsed_ms, created_at);
