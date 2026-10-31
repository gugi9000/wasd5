CREATE TABLE cards_elemental_results (
    game_id TEXT PRIMARY KEY NOT NULL,
    category TEXT NOT NULL,
    summary TEXT NOT NULL,
    completed_at BIGINT NOT NULL
);

CREATE INDEX cards_elemental_results_category ON cards_elemental_results(category);

CREATE TRIGGER cards_elemental_solo_result
AFTER UPDATE OF state ON cards_elemental_games
WHEN json_extract(OLD.state, '$.status') = 'playing'
 AND json_extract(NEW.state, '$.status') IN ('won', 'lost', 'draw')
BEGIN
    INSERT INTO cards_elemental_results VALUES (
        NEW.id, COALESCE(json_extract(NEW.state, '$.opponent'), 'tosser'),
        json_object(
            'rounds', json_extract(NEW.state, '$.round') - 1,
            'winner', CASE json_extract(NEW.state, '$.status') WHEN 'won' THEN 0 WHEN 'lost' THEN 1 ELSE NULL END,
            'winnings', json_array(json_extract(NEW.state, '$.your_winnings'), json_extract(NEW.state, '$.server_winnings'))
        ), NEW.updated_at
    );
END;

CREATE TRIGGER cards_elemental_online_result
AFTER UPDATE OF status ON cards_elemental_online_games
WHEN OLD.status = 'playing' AND NEW.status IN ('won', 'draw')
BEGIN
    INSERT INTO cards_elemental_results VALUES (
        NEW.id, CASE NEW.visibility WHEN 'public' THEN 'pvp' ELSE 'private' END,
        json_object(
            'rounds', json_extract(NEW.state, '$.round') - 1,
            'winner', json_extract(NEW.state, '$.winner'),
            'winnings', json_extract(NEW.state, '$.winnings')
        ), NEW.updated_at
    );
END;

INSERT INTO cards_elemental_results
SELECT id, COALESCE(json_extract(state, '$.opponent'), 'tosser'),
    json_object(
        'rounds', json_extract(state, '$.round') - 1,
        'winner', CASE json_extract(state, '$.status') WHEN 'won' THEN 0 WHEN 'lost' THEN 1 ELSE NULL END,
        'winnings', json_array(json_extract(state, '$.your_winnings'), json_extract(state, '$.server_winnings'))
    ), updated_at
FROM cards_elemental_games
WHERE json_extract(state, '$.status') IN ('won', 'lost', 'draw');

INSERT INTO cards_elemental_results
SELECT id, CASE visibility WHEN 'public' THEN 'pvp' ELSE 'private' END,
    json_object(
        'rounds', json_extract(state, '$.round') - 1,
        'winner', json_extract(state, '$.winner'),
        'winnings', json_extract(state, '$.winnings')
    ), updated_at
FROM cards_elemental_online_games WHERE status IN ('won', 'draw');
