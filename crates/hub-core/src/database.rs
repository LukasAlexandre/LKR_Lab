use crate::{
    inspect::{self, Registration, RegistrationStatus},
    locator::{self, RepositoryLocator},
    models::*,
    portable::{ApplySummary, PortablePreferences, PortableProject, PortableWorkspace},
    sync::SyncMeta,
    HubResult,
};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::{collections::HashSet, path::Path};
pub struct Database {
    pub conn: Connection,
}

/// Pedido de cadastro já inspecionado (a pasta e o locator vêm do backend, não da interface).
#[derive(Debug, Clone)]
pub struct RegisterRequest {
    pub folder: std::path::PathBuf,
    pub locator: Option<RepositoryLocator>,
    pub repository: String,
    pub stack: Vec<String>,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterResult {
    pub registered: bool,
    pub project: Option<ProjectEntry>,
    pub registration: Registration,
}

fn inspect_same_folder(bound: &str, folder: &Path) -> bool {
    !bound.is_empty() && inspect::same_folder_path(bound, folder)
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
        if version > 5 {
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
        if version < 3 {
            migrate_bindings(&mut conn)?;
        }
        if version < 4 {
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            tx.execute_batch(include_str!("../migrations/004_sync_state.sql"))
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
        }
        if version < 5 {
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            tx.execute_batch(include_str!("../migrations/005_machine.sql"))
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
        }
        Ok(Self { conn })
    }
    pub fn projects(&self) -> HubResult<Vec<Project>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{PROJECT_SELECT} ORDER BY p.updated_at DESC"))
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], read_project)
            .map_err(|e| e.to_string())?;
        rows.map(|r| r.map_err(|e| e.to_string())?).collect()
    }
    pub fn project(&self, id: &str) -> HubResult<Project> {
        self.conn
            .query_row(
                &format!("{PROJECT_SELECT} WHERE p.id=?1"),
                [id],
                read_project,
            )
            .map_err(|_| "Projeto não encontrado".to_string())?
    }
    pub fn save(&mut self, id: Option<&str>, input: ProjectInput) -> HubResult<Project> {
        let input = crate::projects::validate(input)?;
        if id.is_none() && input.local_path.is_empty() {
            return Err("Selecione a pasta do projeto.".into());
        }
        let now: String = self
            .conn
            .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now')", [], |r| {
                r.get(0)
            })
            .map_err(|e| e.to_string())?;
        let old = id.map(|id| self.project(id)).transpose()?;
        // O locator é derivado da PASTA pelo backend (Git só leitura); nunca vem da interface.
        let folder_locator: Option<RepositoryLocator> = if input.local_path.is_empty() {
            None
        } else {
            locator::locator_of(Path::new(&input.local_path))?.locator
        };
        match &old {
            None => {
                // Cadastro novo por este caminho: mesma proteção contra duplicata do `register`.
                let known = self.projects_for_matching()?;
                let found = inspect::classify_registration(
                    Path::new(&input.local_path),
                    folder_locator.as_ref(),
                    &known,
                );
                if found.status != RegistrationStatus::New {
                    return Err(found.message);
                }
            }
            Some(existing) => {
                if let (Some(want), Some(have)) = (&existing.locator, &folder_locator) {
                    if want != have {
                        return Err(format!(
                            "A pasta é de outro repositório ou subprojeto ({}); {} espera {}. Nada foi alterado.",
                            have.display(),
                            existing.name,
                            want.display()
                        ));
                    }
                }
            }
        }
        let locator = old
            .as_ref()
            .and_then(|p| p.locator.clone())
            .or(folder_locator);
        let project = Project {
            id: old
                .as_ref()
                .map(|p| p.id.clone())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            slug: crate::projects::slug(&input.name),
            name: input.name,
            description: input.description,
            local_path: if input.local_path.is_empty() {
                old.as_ref()
                    .map(|p| p.local_path.clone())
                    .unwrap_or_default()
            } else {
                input.local_path
            },
            locator,
            repository: input.repository,
            stack: input.stack,
            tags: input.tags,
            ports: input.ports,
            commands: input.commands,
            created_at: old.map(|p| p.created_at).unwrap_or_else(|| now.clone()),
            updated_at: now,
        };
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        upsert_project(&tx, &project)?;
        if !project.local_path.is_empty() {
            upsert_binding(&tx, &project.id, &project.local_path)?;
        }
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
    /// "Localizar": vincula um projeto do workspace a uma pasta desta máquina.
    ///
    /// Nunca sobrescreve em silêncio: pasta já vinculada a outro projeto, ou projeto que já tem
    /// vínculo VÁLIDO em outra pasta, são recusados com explicação (vínculo antigo que não existe
    /// mais pode ser substituído). A pasta precisa ser o mesmo repositório E o mesmo caminho dentro
    /// dele (locator); projeto antigo só com `repository` compara o remote; sem nada para
    /// conferir, só vincula com confirmação explícita.
    pub fn bind(&mut self, id: &str, path: &str, confirmed: bool) -> HubResult<BindResult> {
        let project = self.project(id)?;
        let folder = crate::runtime::plain_path(crate::projects::canonical(path)?);
        let local_path = folder.to_string_lossy().to_string();
        let known = self.projects()?;
        if let Some(other) = known
            .iter()
            .find(|p| p.id != project.id && inspect_same_folder(&p.local_path, &folder))
        {
            return Err(format!(
                "Esta pasta já está vinculada ao projeto “{}”. Nada foi alterado.",
                other.name
            ));
        }
        if inspect_same_folder(&project.local_path, &folder) {
            return Ok(BindResult {
                bound: true,
                needs_confirmation: false,
                message: format!("{} já está vinculado a esta pasta.", project.name),
            });
        }
        if crate::projects::location(&project) == Location::Available {
            return Err(format!(
                "{} já está vinculado a outra pasta válida nesta máquina ({}); o vínculo não é trocado automaticamente. Nada foi alterado.",
                project.name, project.local_path
            ));
        }
        let found = locator::locator_of(&folder)?;
        match (&project.locator, &found.locator) {
            (Some(want), Some(have)) if want == have => {}
            (Some(want), Some(have)) => {
                return Err(format!(
                    "A pasta é de outro repositório ou subprojeto ({}); {} espera {}. Nada foi vinculado.",
                    have.display(),
                    project.name,
                    want.display()
                ));
            }
            (Some(want), None) => {
                return Err(format!(
                    "A pasta não tem um remote Git reconhecível; {} espera {}. Nada foi vinculado.",
                    project.name,
                    want.display()
                ));
            }
            (None, have) if !project.repository.is_empty() => {
                let wanted = locator::normalize_remote(&project.repository).map(|n| n.canonical);
                match (wanted, have) {
                    (Some(want), Some(have)) if want == have.remote => {}
                    (_, Some(have)) => {
                        return Err(format!(
                            "A pasta é de outro repositório ({}); {} espera {}. Nada foi vinculado.",
                            have.remote, project.name, project.repository
                        ));
                    }
                    (_, None) => {
                        return Err(format!(
                            "A pasta não tem um remote Git reconhecível; {} espera {}. Nada foi vinculado.",
                            project.name, project.repository
                        ));
                    }
                }
            }
            (None, have) if !confirmed => {
                let hint = have
                    .as_ref()
                    .map(|l| format!(" A pasta aponta para {}.", l.display()))
                    .unwrap_or_default();
                return Ok(BindResult {
                    bound: false,
                    needs_confirmation: true,
                    message: format!("{} não tem repositório cadastrado, então não dá para conferir se esta é a pasta certa.{hint}", project.name),
                });
            }
            (None, _) => {}
        }
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        // Vincular é só local: não altera o conteúdo portátil (nem o hash de sync).
        upsert_binding(&tx, id, &local_path)?;
        tx.execute(
            "INSERT INTO activities(project_id,action) VALUES(?1,'Projeto localizado nesta máquina')",
            [id],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(BindResult {
            bound: true,
            needs_confirmation: false,
            message: format!("{} vinculado a {local_path}.", project.name),
        })
    }
    /// Cadastra um projeto NOVO a partir de uma pasta já inspecionada. O backend é a autoridade:
    /// reclassifica contra o que há no banco AGORA (esta chamada roda sob a trava do banco, então
    /// um cadastro concorrente não passa), e só cria um UUID quando o resultado é `New`.
    /// Qualquer outro resultado volta sem criar nada, para a interface mostrar o caminho certo
    /// (Localizar / Abrir) em vez de duplicar.
    pub fn register(&mut self, request: RegisterRequest) -> HubResult<RegisterResult> {
        let (name, description) =
            inspect::validate_registration_fields(&request.name, &request.description)?;
        let known = self.projects_for_matching()?;
        let registration =
            inspect::classify_registration(&request.folder, request.locator.as_ref(), &known);
        if registration.status != RegistrationStatus::New {
            return Ok(RegisterResult {
                registered: false,
                project: None,
                registration,
            });
        }
        let now: String = self
            .conn
            .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now')", [], |r| {
                r.get(0)
            })
            .map_err(|e| e.to_string())?;
        let project = Project {
            id: uuid::Uuid::new_v4().to_string(),
            slug: crate::projects::slug(&name),
            name,
            description,
            local_path: request.folder.to_string_lossy().to_string(),
            locator: request.locator,
            repository: request.repository,
            stack: request.stack,
            tags: vec![],
            ports: vec![],
            commands: vec![],
            created_at: now.clone(),
            updated_at: now,
        };
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        upsert_project(&tx, &project)?;
        upsert_binding(&tx, &project.id, &project.local_path)?;
        tx.execute(
            "INSERT INTO activities(project_id,action) VALUES(?1,'Projeto cadastrado nesta máquina')",
            [&project.id],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(RegisterResult {
            registered: true,
            project: Some(crate::projects::entry(project)),
            registration,
        })
    }
    /// Projetos para classificar um cadastro: os criados antes do locator ganham, SÓ EM MEMÓRIA,
    /// o da pasta vinculada (Git só leitura). Nada é gravado: o conteúdo portátil (e o hash de
    /// sync) só muda por ação explícita do usuário.
    pub fn projects_for_matching(&self) -> HubResult<Vec<Project>> {
        let mut known = self.projects()?;
        for project in &mut known {
            if project.locator.is_none()
                && crate::projects::location(project) == Location::Available
            {
                if let Ok(found) = locator::locator_of(Path::new(&project.local_path)) {
                    project.locator = found.locator;
                }
            }
        }
        Ok(known)
    }
    /// Dados portáteis do SQLite: projetos sem caminho, prompts e knowledge.
    pub fn export_portable(&self) -> HubResult<PortableWorkspace> {
        let mut ws = PortableWorkspace {
            version: crate::portable::SCHEMA_VERSION,
            projects: self.projects()?.iter().map(PortableProject::from).collect(),
            prompts: self.prompts()?,
            knowledge: self.knowledge()?,
            preferences: self.preferences()?,
        };
        crate::portable::normalize(&mut ws);
        Ok(ws)
    }
    pub fn preferences(&self) -> HubResult<PortablePreferences> {
        let data: Option<String> = self
            .conn
            .query_row(
                "SELECT data FROM portable_preferences WHERE id=1",
                [],
                |r| r.get(0),
            )
            .ok();
        match data {
            Some(text) => serde_json::from_str(&text).map_err(|e| e.to_string()),
            None => Ok(PortablePreferences::default()),
        }
    }
    /// Guarda as preferências portáteis enviadas pela interface (normalizadas).
    pub fn save_preferences(
        &mut self,
        prefs: PortablePreferences,
    ) -> HubResult<PortablePreferences> {
        let mut ws = PortableWorkspace {
            version: crate::portable::SCHEMA_VERSION,
            projects: vec![],
            prompts: vec![],
            knowledge: vec![],
            preferences: prefs,
        };
        crate::portable::normalize(&mut ws);
        let prefs = ws.preferences;
        if self.preferences()? != prefs {
            set_preferences(&self.conn, &prefs)?;
        }
        Ok(prefs)
    }
    pub fn sync_meta(&self) -> HubResult<SyncMeta> {
        self.conn
            .query_row(
                "SELECT base_hash,last_applied_hash,last_synced_at FROM sync_state WHERE id=1",
                [],
                |r| {
                    Ok(SyncMeta {
                        base_hash: r.get(0)?,
                        last_applied_hash: r.get(1)?,
                        last_synced_at: r.get(2)?,
                    })
                },
            )
            .map_err(|e| e.to_string())
    }
    /// Marca que local e arquivo versionado concordam em `hash` (push concluído
    /// ou conteúdo já igual). Só é chamado depois do sucesso.
    pub fn mark_synced(&self, hash: &str) -> HubResult<()> {
        self.conn
            .execute(
                "UPDATE sync_state SET base_hash=?1,last_synced_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=1",
                [hash],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    /// Torna o SQLite igual ao workspace portátil, numa transação. Vínculos de
    /// pasta dos projetos que continuam no workspace são preservados; atividades
    /// (histórico desta máquina) não são tocadas.
    pub fn apply_portable(&mut self, ws: &PortableWorkspace) -> HubResult<ApplySummary> {
        self.apply_inner(ws, None)
    }
    /// Aplica o workspace vindo do arquivo versionado e, na MESMA transação,
    /// registra `hash` como último aplicado e base de sync. Se algo falhar, nada muda.
    pub fn apply_synced(&mut self, ws: &PortableWorkspace, hash: &str) -> HubResult<ApplySummary> {
        self.apply_inner(ws, Some(hash))
    }
    fn apply_inner(
        &mut self,
        ws: &PortableWorkspace,
        applied: Option<&str>,
    ) -> HubResult<ApplySummary> {
        crate::portable::validate(ws)?;
        let now: String = self
            .conn
            .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now')", [], |r| {
                r.get(0)
            })
            .map_err(|e| e.to_string())?;
        let or_now = |value: &str| {
            if value.is_empty() {
                now.clone()
            } else {
                value.to_string()
            }
        };
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        for p in &ws.projects {
            let project = Project {
                id: p.id.clone(),
                name: p.name.trim().to_string(),
                slug: crate::projects::slug(&p.name),
                description: p.description.clone(),
                local_path: String::new(),
                locator: p.locator.clone(),
                repository: p.repository.clone(),
                stack: p.stack.clone(),
                tags: p.tags.clone(),
                ports: p.ports.clone(),
                commands: p.commands.clone(),
                created_at: or_now(&p.created_at),
                updated_at: or_now(&p.updated_at),
            };
            upsert_project(&tx, &project)?;
        }
        delete_absent(
            &tx,
            "prompt_templates",
            ws.prompts.iter().map(|p| p.id.as_str()),
        )?;
        for p in &ws.prompts {
            tx.execute("INSERT INTO prompt_templates(id,title,category,project_id,body) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET title=excluded.title,category=excluded.category,project_id=excluded.project_id,body=excluded.body", params![p.id, p.title, p.category, p.project_id, p.body]).map_err(|e| e.to_string())?;
        }
        delete_absent(&tx, "knowledge", ws.knowledge.iter().map(|k| k.id.as_str()))?;
        for k in &ws.knowledge {
            tx.execute("INSERT INTO knowledge(id,project_id,title,kind,body,tags,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET project_id=excluded.project_id,title=excluded.title,kind=excluded.kind,body=excluded.body,tags=excluded.tags,updated_at=excluded.updated_at", params![k.id, k.project_id, k.title, k.kind, k.body, k.tags, or_now(&k.updated_at)]).map_err(|e| e.to_string())?;
        }
        // Por último: projetos que saíram do workspace (o vínculo vai junto, a pasta nunca).
        let removed_projects =
            delete_absent(&tx, "projects", ws.projects.iter().map(|p| p.id.as_str()))?;
        set_preferences(&tx, &ws.preferences)?;
        if let Some(hash) = applied {
            tx.execute(
                "UPDATE sync_state SET base_hash=?1,last_applied_hash=?1,last_synced_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=1",
                [hash],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.execute(
            "INSERT INTO activities(action) VALUES('Workspace atualizado a partir de data/workspace.json')",
            [],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(ApplySummary {
            projects: ws.projects.len(),
            prompts: ws.prompts.len(),
            knowledge: ws.knowledge.len(),
            removed_projects,
        })
    }
}

fn set_preferences(conn: &Connection, prefs: &PortablePreferences) -> HubResult<()> {
    conn.execute(
        "INSERT INTO portable_preferences(id,data) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
        [serde_json::to_string(prefs).map_err(|e| e.to_string())?],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

const PROJECT_SELECT: &str =
    "SELECT p.data, b.local_path FROM projects p LEFT JOIN project_bindings b ON b.project_id = p.id";

fn read_project(r: &rusqlite::Row) -> rusqlite::Result<HubResult<Project>> {
    let data: String = r.get(0)?;
    let path: Option<String> = r.get(1)?;
    Ok(serde_json::from_str::<Project>(&data)
        .map(|mut p| {
            p.local_path = path.unwrap_or_default();
            p
        })
        .map_err(|e| e.to_string()))
}

/// O cadastro guarda só dados portáteis: o caminho fica em project_bindings.
fn upsert_project(conn: &Connection, project: &Project) -> HubResult<()> {
    let mut data = serde_json::to_value(project).map_err(|e| e.to_string())?;
    if let Some(map) = data.as_object_mut() {
        map.remove("localPath");
    }
    conn.execute(
        "INSERT INTO projects(id,data,updated_at) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET data=excluded.data,updated_at=excluded.updated_at",
        params![project.id, data.to_string(), project.updated_at],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn upsert_binding(conn: &Connection, id: &str, local_path: &str) -> HubResult<()> {
    conn.execute(
        "INSERT INTO project_bindings(project_id,local_path) VALUES(?1,?2) ON CONFLICT(project_id) DO UPDATE SET local_path=excluded.local_path,bound_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')",
        params![id, local_path],
    )
    .map_err(|_| "Esta pasta já está vinculada a outro projeto.".to_string())?;
    Ok(())
}

/// Remove linhas cujo id não está em `keep`. `table` é sempre um literal deste arquivo.
fn delete_absent<'a>(
    conn: &Connection,
    table: &'static str,
    keep: impl Iterator<Item = &'a str>,
) -> HubResult<usize> {
    let keep: HashSet<&str> = keep.collect();
    let existing: Vec<String> = conn
        .prepare(&format!("SELECT id FROM {table}"))
        .and_then(|mut stmt| {
            stmt.query_map([], |r| r.get(0))?
                .collect::<Result<Vec<String>, _>>()
        })
        .map_err(|e| e.to_string())?;
    let mut removed = 0;
    for id in existing.iter().filter(|id| !keep.contains(id.as_str())) {
        removed += conn
            .execute(&format!("DELETE FROM {table} WHERE id=?1"), [id])
            .map_err(|e| e.to_string())?;
    }
    Ok(removed)
}

/// Migration 003: reconstrói `projects` sem o caminho (procedimento oficial do
/// SQLite: foreign_keys desligado só durante a transação, conferência no fim).
fn migrate_bindings(conn: &mut Connection) -> HubResult<()> {
    conn.pragma_update(None, "foreign_keys", "OFF")
        .map_err(|e| e.to_string())?;
    let result = (|| -> HubResult<()> {
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute_batch(include_str!("../migrations/003_project_bindings.sql"))
            .map_err(|e| e.to_string())?;
        let broken = tx
            .prepare("PRAGMA foreign_key_check")
            .and_then(|mut stmt| {
                let rows = stmt.query_map([], |_| Ok(()))?;
                Ok(rows.count())
            })
            .map_err(|e| e.to_string())?;
        if broken > 0 {
            return Err(
                "Migração de vínculos encontrou referências inválidas; nada foi alterado.".into(),
            );
        }
        tx.commit().map_err(|e| e.to_string())
    })();
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| e.to_string())?;
    result
}
