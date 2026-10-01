-- Identidade do projeto != caminho local (docs/STATE.md).
-- O cadastro (projects) passa a guardar só dados portáteis; a pasta de cada
-- projeto nesta máquina vira um vínculo separado (project_bindings).
-- Executada com foreign_keys=OFF pelo Database::open (reconstrução de tabela
-- seguindo o procedimento oficial do SQLite), seguida de foreign_key_check.
CREATE TABLE project_bindings(
  project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
  local_path TEXT NOT NULL UNIQUE,
  bound_at TEXT NOT NULL DEFAULT(strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
INSERT INTO project_bindings(project_id, local_path) SELECT id, local_path FROM projects;
CREATE TABLE projects_portable(
  id TEXT PRIMARY KEY,
  data TEXT NOT NULL CHECK(json_valid(data)),
  updated_at TEXT NOT NULL
);
INSERT INTO projects_portable(id, data, updated_at)
  SELECT id, json_remove(data, '$.localPath'), updated_at FROM projects;
DROP TABLE projects;
ALTER TABLE projects_portable RENAME TO projects;
PRAGMA user_version=3;
