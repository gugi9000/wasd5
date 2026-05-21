-- SQLite does not support DROP COLUMN before version 3.35.
-- Recreate the table without the creatine_reminder column.
CREATE TABLE users_old (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  username      TEXT    NOT NULL UNIQUE,
  password_hash TEXT    NOT NULL,
  role          TEXT    NOT NULL DEFAULT 'member',
  created_at    INTEGER NOT NULL,
  email         TEXT
);
INSERT INTO users_old (id, username, password_hash, role, created_at, email)
  SELECT id, username, password_hash, role, created_at, email FROM users;
DROP TABLE users;
ALTER TABLE users_old RENAME TO users;
