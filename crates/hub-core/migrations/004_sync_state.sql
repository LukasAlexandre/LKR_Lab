-- Estado de sync e preferências portáteis (docs/STATE.md).
-- sync_state é metadado DESTA máquina (nunca entra no workspace portátil):
--   base_hash         conteúdo em que local e arquivo versionado concordaram por último
--   last_applied_hash último workspace remoto aplicado no SQLite
CREATE TABLE sync_state(
  id INTEGER PRIMARY KEY CHECK(id=1),
  base_hash TEXT,
  last_applied_hash TEXT,
  last_synced_at TEXT
);
INSERT INTO sync_state(id) VALUES(1);
-- Preferências marcadas como portáteis no desktop (sidebar, densidade, favoritos).
CREATE TABLE portable_preferences(
  id INTEGER PRIMARY KEY CHECK(id=1),
  data TEXT NOT NULL CHECK(json_valid(data))
);
PRAGMA user_version=4;
