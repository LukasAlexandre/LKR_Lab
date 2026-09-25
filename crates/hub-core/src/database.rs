use crate::{models::*, HubResult};
use rusqlite::{params, Connection};
use std::path::Path;
pub struct Database {
    pub conn: Connection,
}
impl Database {
    pub fn open(path: &Path) -> HubResult<Self> {
        let mut conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| e.to_string())?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| e.to_string())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if version > 2 {
            return Err("Banco criado por versão mais recente do aplicativo.".into());
        }
        if version == 0 {
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            tx.execute_batch(include_str!("../migrations/001_initial.sql"))
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
        }
        if version < 2 {
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            tx.execute_batch(include_str!("../migrations/002_knowledge.sql"))
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
        }
        Ok(Self { conn })
    }
    pub fn projects(&self) -> HubResult<Vec<Project>> {
        let mut stmt = self
            .conn
            .prepare("SELECT data FROM projects ORDER BY updated_at DESC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.map(|r| {
            serde_json::from_str(&r.map_err(|e| e.to_string())?).map_err(|e| e.to_string())
        })
        .collect()
    }
    pub fn project(&self, id: &str) -> HubResult<Project> {
        let data: String = self
            .conn
            .query_row("SELECT data FROM projects WHERE id=?1", [id], |r| r.get(0))
            .map_err(|_| "Projeto não encontrado".to_string())?;
        serde_json::from_str(&data).map_err(|e| e.to_string())
    }
    pub fn save(&mut self, id: Option<&str>, input: ProjectInput) -> HubResult<Project> {
        let input = crate::projects::validate(input)?;
        let now: String = self
            .conn
            .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now')", [], |r| {
                r.get(0)
            })
            .map_err(|e| e.to_string())?;
        let old = id.map(|id| self.project(id)).transpose()?;
        let project = Project {
            id: old
                .as_ref()
                .map(|p| p.id.clone())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            slug: crate::projects::slug(&input.name),
            name: input.name,
            description: input.description,
            local_path: input.local_path,
            repository: input.repository,
            stack: input.stack,
            tags: input.tags,
            ports: input.ports,
            commands: input.commands,
            created_at: old.map(|p| p.created_at).unwrap_or_else(|| now.clone()),
            updated_at: now,
        };
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO projects(id,local_path,data,updated_at) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET local_path=excluded.local_path,data=excluded.data,updated_at=excluded.updated_at", params![project.id,project.local_path,serde_json::to_string(&project).map_err(|e|e.to_string())?,project.updated_at]).map_err(|_|"Não foi possível salvar. A pasta pode já estar cadastrada.".to_string())?;
        tx.execute(
            "INSERT INTO activities(project_id,action) VALUES(?1,'Cadastro de projeto atualizado')",
            [&project.id],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(project)
    }
    pub fn delete(&mut self, id: &str, confirmed: bool) -> HubResult<()> {
        if !confirmed {
            return Err("Confirmação explícita necessária".into());
        }
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let count = tx
            .execute("DELETE FROM projects WHERE id=?1", [id])
            .map_err(|e| e.to_string())?;
        if count == 0 {
            return Err("Projeto não encontrado".into());
        }
        tx.execute("INSERT INTO activities(action) VALUES('Projeto removido do cadastro; arquivos preservados')",[]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn activity(&self, id: &str, action: &str) -> HubResult<()> {
        self.conn
            .execute(
                "INSERT INTO activities(project_id,action) VALUES(?1,?2)",
                params![id, action],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn activities(&self) -> HubResult<Vec<Activity>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id,project_id,action,created_at FROM activities ORDER BY id DESC LIMIT 30",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Activity {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    action: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string());
        rows
    }
    pub fn prompts(&self) -> HubResult<Vec<Prompt>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id,title,category,project_id,body FROM prompt_templates ORDER BY title",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Prompt {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    category: r.get(2)?,
                    project_id: r.get(3)?,
                    body: r.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string());
        rows
    }
    pub fn knowledge(&self) -> HubResult<Vec<KnowledgeEntry>> {
        let mut stmt = self.conn.prepare("SELECT id,project_id,title,kind,body,tags,updated_at FROM knowledge ORDER BY updated_at DESC, id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(KnowledgeEntry {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    title: r.get(2)?,
                    kind: r.get(3)?,
                    body: r.get(4)?,
                    tags: r.get(5)?,
                    updated_at: r.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }
    pub fn save_knowledge(&self, mut entry: KnowledgeEntry) -> HubResult<String> {
        entry.title = entry.title.trim().into();
        if entry.title.is_empty()
            || entry.title.len() > 240
            || entry.body.trim().is_empty()
            || entry.body.len() > 128_000
            || entry.tags.len() > 2_000
        {
            return Err("Título e conteúdo obrigatórios; limite de 240 bytes no título, 128 KB no conteúdo e 2 KB em tags.".into());
        }
        if !["note", "decision", "architecture", "bug", "documentation"]
            .contains(&entry.kind.as_str())
        {
            return Err("Tipo de conhecimento inválido.".into());
        }
        if entry.id.is_empty() {
            entry.id = uuid::Uuid::new_v4().to_string();
        }
        self.conn.execute("INSERT INTO knowledge(id,project_id,title,kind,body,tags) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET project_id=excluded.project_id,title=excluded.title,kind=excluded.kind,body=excluded.body,tags=excluded.tags,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')", params![entry.id,entry.project_id,entry.title,entry.kind,entry.body,entry.tags]).map_err(|e| e.to_string())?;
        Ok(entry.id)
    }
    pub fn save_prompt(&self, mut p: Prompt) -> HubResult<String> {
        if p.title.trim().is_empty() || p.body.trim().is_empty() || p.body.len() > 32000 {
            return Err("Título e template obrigatórios; máximo 32 KB.".into());
        }
        if p.id.is_empty() {
            p.id = uuid::Uuid::new_v4().to_string();
        }
        self.conn.execute("INSERT INTO prompt_templates(id,title,category,project_id,body) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET title=excluded.title,category=excluded.category,project_id=excluded.project_id,body=excluded.body",params![p.id,p.title,p.category,p.project_id,p.body]).map_err(|e|e.to_string())?;
        Ok(p.id)
    }
}
