-- DDAE: detalhe da Session (Concept 07).
--
-- ddae_events: histórico SEMÂNTICO e PORTÁTIL da feature (State Class B): append-only, com UUID
-- próprio. Não é telemetria da workstation (isso continua em `activities`, local).
--   event_type  SESSION_CREATED | BLOCK_COMPLETED | CRITERION_ADDED | ... (hub-core::ddae::EventType)
--   payload     objeto JSON pequeno com fatos da própria Session (nunca caminho, Machine ID, host, IP, PID)
--   block_id    opcional; SEM chave estrangeira: o evento sobrevive à remoção de um bloco pendente
-- Critérios de conclusão continuam em JSON (migration 007) mas passam a {id, text, completed};
-- o formato antigo (lista de strings) é lido e convertido pelo hub-core.
CREATE TABLE ddae_events(
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES ddae_sessions(id) ON DELETE CASCADE,
  block_id TEXT,
  event_type TEXT NOT NULL CHECK(length(event_type) > 0),
  payload TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(payload)),
  created_at TEXT NOT NULL
);
CREATE INDEX ddae_events_session ON ddae_events(session_id, created_at);
CREATE INDEX ddae_events_type ON ddae_events(event_type);
-- Append-only: um evento nunca é alterado; só some junto com a Session (CASCADE) ou ao
-- substituir o DDAE inteiro pelo workspace.
CREATE TRIGGER ddae_events_no_update BEFORE UPDATE ON ddae_events
BEGIN
  SELECT RAISE(ABORT, 'ddae_events é append-only.');
END;

ALTER TABLE ddae_blocks ADD COLUMN description TEXT NOT NULL DEFAULT '';
-- Decisão associada (opcionalmente) a um bloco; remover o bloco pendente apenas solta o vínculo.
ALTER TABLE ddae_decisions ADD COLUMN block_id TEXT REFERENCES ddae_blocks(id) ON DELETE SET NULL;
PRAGMA user_version=8;
