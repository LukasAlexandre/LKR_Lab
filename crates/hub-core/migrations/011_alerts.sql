-- Alerts & Diagnostics (SESSION-002, Block 09): estado LOCAL DA MÁQUINA.
--
-- Nada daqui entra no workspace portátil, no sync, no Git, no Planejamento nem no DDAE: alertas
-- descrevem ESTA máquina e podem citar nomes de processos, volumes e projetos locais.
--
-- machine_alerts: uma linha por OCORRÊNCIA de um alerta. O `fingerprint` (regra + recurso) é
--   estável: enquanto a condição persiste é a mesma linha (last_seen/seen_count avançam). Quando a
--   condição some a linha vira `resolved` (histórico preservado) e, se ela voltar, nasce uma nova
--   linha com `occurrence` + 1.
-- machine_diagnostic_runs: histórico de diagnósticos explícitos (resumo e cauda curta da saída,
--   nunca a saída inteira).
CREATE TABLE machine_alerts(
  id TEXT PRIMARY KEY,
  fingerprint TEXT NOT NULL,
  rule_id TEXT NOT NULL,
  domain TEXT NOT NULL,
  source TEXT NOT NULL,
  resource TEXT NOT NULL,
  severity TEXT NOT NULL CHECK(severity IN ('info','attention','critical')),
  confidence TEXT NOT NULL CHECK(confidence IN ('low','medium','high')),
  title TEXT NOT NULL,
  summary TEXT NOT NULL,
  reason TEXT NOT NULL,
  next_step TEXT NOT NULL,
  evidence TEXT NOT NULL CHECK(json_valid(evidence)),
  action TEXT CHECK(action IS NULL OR json_valid(action)),
  cta TEXT CHECK(cta IS NULL OR json_valid(cta)),
  status TEXT NOT NULL CHECK(status IN ('active','acknowledged','resolved')),
  first_seen INTEGER NOT NULL,
  last_seen INTEGER NOT NULL,
  acknowledged_at INTEGER,
  resolved_at INTEGER,
  occurrence INTEGER NOT NULL CHECK(occurrence >= 1),
  seen_count INTEGER NOT NULL CHECK(seen_count >= 1)
);
CREATE INDEX idx_machine_alerts_fingerprint ON machine_alerts(fingerprint, occurrence DESC);
CREATE INDEX idx_machine_alerts_status ON machine_alerts(status, last_seen DESC);
-- No máximo uma ocorrência aberta por fingerprint.
CREATE UNIQUE INDEX idx_machine_alerts_one_open ON machine_alerts(fingerprint) WHERE status <> 'resolved';

CREATE TABLE machine_diagnostic_runs(
  id TEXT PRIMARY KEY,
  diagnostic TEXT NOT NULL,
  target TEXT,
  started_at INTEGER NOT NULL,
  finished_at INTEGER,
  result TEXT NOT NULL,
  exit_code INTEGER,
  summary TEXT NOT NULL,
  output_tail TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_machine_diagnostic_runs_started ON machine_diagnostic_runs(started_at DESC);
PRAGMA user_version=11;
