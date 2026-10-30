CREATE TABLE cards_elemental_stats (
    category TEXT PRIMARY KEY NOT NULL,
    completed BIGINT NOT NULL DEFAULT 0 CHECK (completed >= 0)
);

INSERT INTO cards_elemental_stats (category, completed) VALUES
    ('pvp', 0), ('private', 0), ('tosser', 0), ('pro', 0);

UPDATE cards_elemental_stats SET completed = (
    SELECT COUNT(*) FROM cards_elemental_games
    WHERE json_extract(state, '$.status') IN ('won', 'lost', 'draw')
      AND COALESCE(json_extract(state, '$.opponent'), 'tosser') = category
) WHERE category IN ('tosser', 'pro');

UPDATE cards_elemental_stats SET completed = (
    SELECT COUNT(*) FROM cards_elemental_online_games
    WHERE status IN ('won', 'draw')
      AND visibility = CASE category WHEN 'pvp' THEN 'public' ELSE 'private' END
) WHERE category IN ('pvp', 'private');

CREATE TRIGGER cards_elemental_solo_completed
AFTER UPDATE OF state ON cards_elemental_games
WHEN json_extract(OLD.state, '$.status') = 'playing'
 AND json_extract(NEW.state, '$.status') IN ('won', 'lost', 'draw')
BEGIN
    UPDATE cards_elemental_stats SET completed = completed + 1
    WHERE category = COALESCE(json_extract(NEW.state, '$.opponent'), 'tosser');
END;

CREATE TRIGGER cards_elemental_online_completed
AFTER UPDATE OF status ON cards_elemental_online_games
WHEN OLD.status = 'playing' AND NEW.status IN ('won', 'draw')
BEGIN
    UPDATE cards_elemental_stats SET completed = completed + 1
    WHERE category = CASE NEW.visibility WHEN 'public' THEN 'pvp' ELSE 'private' END;
END;
