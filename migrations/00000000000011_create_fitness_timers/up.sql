CREATE TABLE fitness_timers (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    owner_id INTEGER NOT NULL REFERENCES users(id),
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    workout TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
