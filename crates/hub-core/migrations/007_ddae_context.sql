-- DDAE: campos de contexto da Session (resultado desejado, restrições, critérios de conclusão,
-- notas e referências). Listas ficam em JSON; "refs" (e não "references", palavra reservada).
-- Estado PORTÁTIL: referências de caminho são sempre RELATIVAS ao Project.
-- "Ready for AI" NÃO é gravado: é derivado desses campos + blocks (hub-core::ddae::ready_for_ai).
ALTER TABLE ddae_sessions ADD COLUMN desired_outcome TEXT NOT NULL DEFAULT '';
ALTER TABLE ddae_sessions ADD COLUMN constraints TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(constraints));
ALTER TABLE ddae_sessions ADD COLUMN criteria TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(criteria));
ALTER TABLE ddae_sessions ADD COLUMN notes TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(notes));
ALTER TABLE ddae_sessions ADD COLUMN refs TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(refs));
PRAGMA user_version=7;
