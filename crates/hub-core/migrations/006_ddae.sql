-- DDAE: Sessions, Blocks e Decisions (docs/concepts/machine-registry/CONCEPT-06.md).
-- SQLite é a fonte de verdade em runtime; Markdown é só export/documentação humana.
-- Estado PORTÁTIL (docs/STATE.md): sem caminho absoluto, Machine ID, hostname ou IP.
--
--   id       UUID interno estável (identidade); `number` é o SESSION-NNN humano, único por Project
--   status   active | frozen | stopped | completed (completed é terminal no MVP)
--   progress NÃO é gravado: é derivado de completed / total dos blocks
CREATE TABLE ddae_sessions(
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  number INTEGER NOT NULL CHECK(number >= 1),
  title TEXT NOT NULL CHECK(length(trim(title)) > 0),
  objective TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL CHECK(status IN ('active','frozen','stopped','completed')),
  pause_reason TEXT,
  result TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  completed_at TEXT,
  UNIQUE(project_id, number)
);
-- No máximo UMA Session ACTIVE por Project (garantido pelo banco, não só pela interface).
CREATE UNIQUE INDEX ddae_one_active_per_project ON ddae_sessions(project_id) WHERE status = 'active';

CREATE TABLE ddae_blocks(
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES ddae_sessions(id) ON DELETE CASCADE,
  position INTEGER NOT NULL CHECK(position >= 0),
  title TEXT NOT NULL CHECK(length(trim(title)) > 0),
  status TEXT NOT NULL CHECK(status IN ('pending','in_progress','completed')),
  UNIQUE(session_id, position)
);
-- No máximo UM Block in_progress por Session.
CREATE UNIQUE INDEX ddae_one_in_progress_per_session ON ddae_blocks(session_id) WHERE status = 'in_progress';

CREATE TABLE ddae_decisions(
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES ddae_sessions(id) ON DELETE CASCADE,
  position INTEGER NOT NULL CHECK(position >= 0),
  title TEXT NOT NULL CHECK(length(trim(title)) > 0),
  body TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  UNIQUE(session_id, position)
);

-- COMPLETED é terminal no MVP: nada em uma Session finalizada pode mudar.
CREATE TRIGGER ddae_completed_is_terminal BEFORE UPDATE ON ddae_sessions
WHEN OLD.status = 'completed'
BEGIN
  SELECT RAISE(ABORT, 'Sessão finalizada é terminal e não pode ser alterada.');
END;
PRAGMA user_version=6;
