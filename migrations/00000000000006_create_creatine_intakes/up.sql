CREATE TABLE creatine_intakes (
    id           INTEGER PRIMARY KEY NOT NULL,
    user_id      INTEGER NOT NULL,
    date         TEXT    NOT NULL,          -- "YYYY-MM-DD"
    amount_grams REAL    NOT NULL DEFAULT 5.0,
    recorded_at  BIGINT  NOT NULL,
    UNIQUE(user_id, date)
);
