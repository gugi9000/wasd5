CREATE TABLE cards_elemental_games (
    id TEXT PRIMARY KEY NOT NULL,
    state TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX cards_elemental_games_created_at ON cards_elemental_games(created_at);
