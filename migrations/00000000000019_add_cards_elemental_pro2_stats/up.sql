INSERT INTO cards_elemental_stats (category, completed)
SELECT 'pro2', COUNT(*) FROM cards_elemental_games
WHERE json_extract(state, '$.status') IN ('won', 'lost', 'draw')
  AND json_extract(state, '$.opponent') = 'pro2';
