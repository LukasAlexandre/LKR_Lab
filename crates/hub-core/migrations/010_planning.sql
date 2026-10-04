-- Planejamento do LKR LAB (Concept 09): fila ordenada de features AINDA NÃO iniciadas de um Project.
--
-- Planning Item != Session != Block != Worktree != issue do Git. Responde "o que vem depois?";
-- o DDAE responde "o que estamos executando agora?".
--
-- Só dois estados são GRAVADOS (`open`, `cancelled`). Planejado / Em execução / Concluído são
-- DERIVADOS da Session vinculada e nunca persistidos.
-- O vínculo mora na SESSION (`ddae_sessions.planning_item_id`): 1 item : 0..1 Session.
-- Estado PORTÁTIL (classe B, docs/STATE.md): sem path, Machine ID, hostname ou IP.
-- Sem prioridade: a ordem manual (`position`, com espaçamento) é a prioridade prática.
CREATE TABLE planning_items(
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  title TEXT NOT NULL CHECK(length(trim(title)) > 0),
  description TEXT NOT NULL DEFAULT '',
  position INTEGER NOT NULL CHECK(position >= 1),
  stored_status TEXT NOT NULL DEFAULT 'open' CHECK(stored_status IN ('open','cancelled')),
  cancel_reason TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  cancelled_at TEXT,
  UNIQUE(project_id, position),
  -- cancelled <=> cancelled_at; o motivo só existe em item cancelado.
  CHECK((stored_status = 'cancelled') = (cancelled_at IS NOT NULL)),
  CHECK(cancel_reason IS NULL OR stored_status = 'cancelled')
);
CREATE INDEX planning_items_project ON planning_items(project_id, position);

-- Histórico semântico PORTÁTIL (append-only). Reordenar NÃO gera evento (a posição é estado).
CREATE TABLE planning_events(
  id TEXT PRIMARY KEY,
  item_id TEXT NOT NULL REFERENCES planning_items(id) ON DELETE CASCADE,
  event_type TEXT NOT NULL CHECK(length(event_type) > 0),
  payload TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(payload)),
  created_at TEXT NOT NULL
);
CREATE INDEX planning_events_item ON planning_events(item_id, created_at);
CREATE TRIGGER planning_events_no_update BEFORE UPDATE ON planning_events
BEGIN
  SELECT RAISE(ABORT, 'planning_events é append-only.');
END;

-- Vínculo Planning Item -> Session, do lado da Session (sem FK, como o worktree: o workspace
-- portátil substitui o DDAE inteiro; a consistência é imposta pelos gatilhos abaixo).
ALTER TABLE ddae_sessions ADD COLUMN planning_item_id TEXT;
-- Um Planning Item não vincula duas Sessions (a Session só tem uma coluna: não vincula dois itens).
CREATE UNIQUE INDEX ddae_sessions_planning_item ON ddae_sessions(planning_item_id) WHERE planning_item_id IS NOT NULL;

-- O item precisa existir, ser do MESMO Project da Session e não estar cancelado (também por SQL direto).
CREATE TRIGGER ddae_sessions_planning_insert BEFORE INSERT ON ddae_sessions
WHEN NEW.planning_item_id IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'O item de planejamento precisa existir, pertencer ao mesmo projeto e não estar cancelado.')
  WHERE NOT EXISTS (
    SELECT 1 FROM planning_items
    WHERE id = NEW.planning_item_id AND project_id = NEW.project_id AND stored_status = 'open'
  );
END;
CREATE TRIGGER ddae_sessions_planning_update BEFORE UPDATE OF planning_item_id, project_id ON ddae_sessions
WHEN NEW.planning_item_id IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'O item de planejamento precisa existir, pertencer ao mesmo projeto e não estar cancelado.')
  WHERE NOT EXISTS (
    SELECT 1 FROM planning_items
    WHERE id = NEW.planning_item_id AND project_id = NEW.project_id AND stored_status = 'open'
  );
END;

-- O vínculo é gravado só na CRIAÇÃO da Session e nunca muda depois: nada de re-apontar uma Session
-- para outro item, nem de dar um item retroativo a uma Session que não nasceu do Planejamento
-- (a SESSION-001 legada). O workspace portátil apaga e reinsere as Sessions, então não é afetado.
CREATE TRIGGER ddae_sessions_planning_is_write_once BEFORE UPDATE OF planning_item_id ON ddae_sessions
WHEN NEW.planning_item_id IS NOT OLD.planning_item_id
BEGIN
  SELECT RAISE(ABORT, 'O vínculo da Session com o item de planejamento é definido na criação e não muda.');
END;

-- Item com Session não pode ser cancelado nem mudar de Project.
CREATE TRIGGER planning_items_cancel_needs_no_session BEFORE UPDATE OF stored_status, project_id ON planning_items
WHEN (NEW.stored_status = 'cancelled' OR NEW.project_id <> OLD.project_id)
  AND EXISTS (SELECT 1 FROM ddae_sessions WHERE planning_item_id = OLD.id)
BEGIN
  SELECT RAISE(ABORT, 'Item de planejamento com Session vinculada não pode ser cancelado nem mudar de projeto.');
END;
PRAGMA user_version=10;
