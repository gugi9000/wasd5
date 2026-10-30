ALTER TABLE minesweeper_games
ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;

UPDATE minesweeper_games
SET created_at = started_at
WHERE created_at = 0;
