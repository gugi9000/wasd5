CREATE TABLE cards_elemental_online_games (
    id TEXT PRIMARY KEY NOT NULL,
    visibility TEXT NOT NULL CHECK (visibility IN ('public', 'private')),
    status TEXT NOT NULL CHECK (status IN ('waiting', 'playing', 'won', 'draw')),
    state TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX cards_elemental_online_games_open
    ON cards_elemental_online_games(visibility, status, created_at);
