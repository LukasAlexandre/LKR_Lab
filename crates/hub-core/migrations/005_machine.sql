-- Machine Registry (docs/concepts/machine-registry/CONCEPT-01.md).
-- Estado DESTA máquina: nunca entra no workspace portátil (docs/STATE.md, classe C).
-- Uma instalação = um computador, por isso a tabela tem no máximo uma linha.
--   machine_id        identidade estável gerada pelo LKR LAB no cadastro (UUID v4);
--                     IP, hostname e hardware são atributos e nunca a substituem
--   snapshot          última detecção passiva (JSON), substituída a cada atualização
--   last_detected_at  instante da detecção, em ms desde a época Unix (validade de 6h)
CREATE TABLE machine(
  id INTEGER PRIMARY KEY CHECK(id=1),
  machine_id TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  usage TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  snapshot TEXT CHECK(snapshot IS NULL OR json_valid(snapshot)),
  last_detected_at INTEGER,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
PRAGMA user_version=5;
