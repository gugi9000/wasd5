CREATE TABLE musha_shugyo_journeys (
    user_id INTEGER PRIMARY KEY NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    journey_id TEXT NOT NULL UNIQUE,
    world_month TEXT NOT NULL,
    world_seed TEXT NOT NULL,
    generator_version INTEGER NOT NULL,
    current_turn INTEGER NOT NULL DEFAULT 0 CHECK (current_turn >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE musha_shugyo_actions (
    user_id INTEGER NOT NULL REFERENCES musha_shugyo_journeys(user_id) ON DELETE CASCADE,
    turn INTEGER NOT NULL CHECK (turn >= 0),
    action TEXT NOT NULL,
    PRIMARY KEY (user_id, turn)
);
