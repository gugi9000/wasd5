CREATE TABLE packages (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  name         TEXT    NOT NULL,
  ordered_date INTEGER NOT NULL,
  received_date INTEGER,
  user_id      INTEGER NOT NULL REFERENCES users(id),
  tracking_id  TEXT
);
