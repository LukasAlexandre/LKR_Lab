-- Worktrees do LKR LAB (Concept 08): camada operacional sobre os Git worktrees REAIS.
--
-- Três camadas que NÃO são a mesma coisa:
--   1. Git worktree  : o objeto do Git (path, HEAD, branch...). Nunca é gravado aqui.
--   2. managed_worktrees : metadata PORTÁTIL (UUID, nome, estado operacional, vínculo com Session/Block)
--   3. worktree_bindings : o path ABSOLUTO desta máquina (classe C: nunca portátil)
--
-- Identidade = `id` (UUID v4 do LKR LAB). Nem path, nem branch, nem pasta, nem HEAD.
-- `repository_locator`/`branch_hint`/`detached_head_hint` são DICAS para reconhecer o worktree em
-- outra máquina; não são identidade.
-- O estado operacional (active|frozen|stopped|completed) é metadata do LKR LAB: não vem de Git nem
-- de runtime, é independente do estado da Session e `completed` é terminal. Não há índice único de
-- active: vários worktrees ACTIVE por Project e por Session são válidos.
CREATE TABLE managed_worktrees(
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  display_name TEXT NOT NULL CHECK(length(trim(display_name)) > 0),
  description TEXT NOT NULL DEFAULT '',
  operational_status TEXT NOT NULL DEFAULT 'active'
    CHECK(operational_status IN ('active','frozen','stopped','completed')),
  state_reason TEXT,
  result TEXT,
  repository_locator TEXT CHECK(repository_locator IS NULL OR json_valid(repository_locator)),
  branch_hint TEXT,
  detached_head_hint TEXT,
  -- Sem chave estrangeira: a consistência é verificada pelos gatilhos abaixo (e pelo core), porque o
  -- workspace portátil substitui o DDAE inteiro e uma FK com SET NULL quebraria "block exige session".
  session_id TEXT,
  block_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  completed_at TEXT,
  -- completed <=> completed_at; block exige session.
  CHECK((operational_status = 'completed') = (completed_at IS NOT NULL)),
  CHECK(block_id IS NULL OR session_id IS NOT NULL)
);
CREATE INDEX managed_worktrees_project ON managed_worktrees(project_id);
CREATE INDEX managed_worktrees_session ON managed_worktrees(session_id);

-- Binding LOCAL (State Class C): nunca entra no workspace portátil, no hash nem em eventos.
CREATE TABLE worktree_bindings(
  worktree_id TEXT PRIMARY KEY REFERENCES managed_worktrees(id) ON DELETE CASCADE,
  local_path TEXT NOT NULL UNIQUE,
  bound_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

-- Histórico semântico PORTÁTIL do worktree (append-only). Sem path, Machine ID, host, IP ou PID.
CREATE TABLE worktree_events(
  id TEXT PRIMARY KEY,
  worktree_id TEXT NOT NULL REFERENCES managed_worktrees(id) ON DELETE CASCADE,
  event_type TEXT NOT NULL CHECK(length(event_type) > 0),
  payload TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(payload)),
  created_at TEXT NOT NULL
);
CREATE INDEX worktree_events_worktree ON worktree_events(worktree_id, created_at);
-- Um evento nunca é alterado; só some com o worktree (CASCADE, que a UI deste corte não expõe) ou
-- ao substituir tudo pelo workspace. O MVP NÃO oferece "excluir worktree do LKR LAB": o ciclo
-- FINALIZADO preserva o histórico.
CREATE TRIGGER worktree_events_no_update BEFORE UPDATE ON worktree_events
BEGIN
  SELECT RAISE(ABORT, 'worktree_events é append-only.');
END;

-- COMPLETED é terminal: o estado não volta para outro.
CREATE TRIGGER managed_worktrees_completed_is_terminal BEFORE UPDATE ON managed_worktrees
WHEN OLD.operational_status = 'completed' AND NEW.operational_status <> 'completed'
BEGIN
  SELECT RAISE(ABORT, 'Worktree finalizado é terminal e não volta a outro estado.');
END;

-- A Session precisa ser do MESMO Project; o Block, da MESMA Session (também por SQL direto).
CREATE TRIGGER managed_worktrees_session_insert BEFORE INSERT ON managed_worktrees
WHEN NEW.session_id IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'A Session do worktree precisa pertencer ao mesmo projeto.')
  WHERE NOT EXISTS (SELECT 1 FROM ddae_sessions WHERE id = NEW.session_id AND project_id = NEW.project_id);
END;
CREATE TRIGGER managed_worktrees_session_update BEFORE UPDATE OF session_id, project_id ON managed_worktrees
WHEN NEW.session_id IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'A Session do worktree precisa pertencer ao mesmo projeto.')
  WHERE NOT EXISTS (SELECT 1 FROM ddae_sessions WHERE id = NEW.session_id AND project_id = NEW.project_id);
END;
CREATE TRIGGER managed_worktrees_block_insert BEFORE INSERT ON managed_worktrees
WHEN NEW.block_id IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'O bloco do worktree precisa pertencer à Session vinculada.')
  WHERE NOT EXISTS (SELECT 1 FROM ddae_blocks WHERE id = NEW.block_id AND session_id = NEW.session_id);
END;
CREATE TRIGGER managed_worktrees_block_update BEFORE UPDATE OF block_id, session_id ON managed_worktrees
WHEN NEW.block_id IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'O bloco do worktree precisa pertencer à Session vinculada.')
  WHERE NOT EXISTS (SELECT 1 FROM ddae_blocks WHERE id = NEW.block_id AND session_id = NEW.session_id);
END;
PRAGMA user_version=9;
