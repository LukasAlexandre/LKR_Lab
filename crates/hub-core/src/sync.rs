//! Sync do workspace portátil: SQLite ⇄ bridge ⇄ Git (data/workspace.json).
//!
//! Local-first e explícito: nada aqui roda sozinho em segundo plano e nada faz
//! commit por clique. Três hashes de conteúdo (SHA-256 do JSON canônico, sem
//! carimbos de data) decidem tudo:
//!
//! * LOCAL — o que o SQLite exporta agora;
//! * REMOTO — o arquivo em origin/<branch> (lido sem tocar na árvore de trabalho);
//! * BASE — onde local e arquivo concordaram por último (`sync_state`, desta máquina).
//!
//! |            | remoto = base | remoto ≠ base |
//! |------------|---------------|---------------|
//! | local=base | limpo         | remoto mudou → aplicar |
//! | local≠base | local mudou → publicar | DIVERGIU → nada é sobrescrito |
//!
//! Só a escolha explícita do usuário (`Resolution`) resolve uma divergência. A
//! base e o último hash aplicado só avançam depois do sucesso (apply e base na
//! mesma transação do SQLite; push só depois de o bridge confirmar).
use crate::{
    bridge::{Bridge, BridgeError, RemoteFile, RemoteInfo},
    database::Database,
    portable::{self, PortablePreferences, PortableWorkspace},
    HubResult,
};
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncMeta {
    pub base_hash: Option<String>,
    pub last_applied_hash: Option<String>,
    pub last_synced_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    Clean,
    LocalDirty,
    RemoteChanged,
    Diverged,
    Offline,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    MarkBase,
    Adopt,
    Push,
}

/// Escolha explícita do usuário para uma divergência.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    KeepLocal,
    TakeRemote,
}
impl Resolution {
    pub fn parse(value: &str) -> HubResult<Self> {
        match value {
            "local" => Ok(Self::KeepLocal),
            "remote" => Ok(Self::TakeRemote),
            _ => Err("Resolução desconhecida.".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub state: SyncState,
    pub message: String,
    /// Código estável para a interface (BRIDGE_DOWN, NETWORK, DIVERGED…).
    pub code: Option<String>,
    pub local_hash: String,
    pub remote_hash: Option<String>,
    pub base_hash: Option<String>,
    pub last_synced_at: Option<String>,
    pub checked_remote: bool,
    /// Houve push neste ciclo.
    pub pushed: bool,
    /// Houve apply do remoto neste ciclo (a interface precisa recarregar).
    pub applied: bool,
    /// Preferências para a interface adotar depois de um apply.
    pub preferences: Option<PortablePreferences>,
}

/// Decisão pura da tabela acima.
pub fn decide(
    local_hash: &str,
    local_empty: bool,
    base: Option<&str>,
    remote: &RemoteFile,
    remote_hash: Option<&str>,
) -> (SyncState, Action) {
    match remote {
        RemoteFile::Missing => {
            if local_empty {
                (SyncState::Clean, Action::None)
            } else {
                (SyncState::LocalDirty, Action::Push)
            }
        }
        RemoteFile::Invalid(_) | RemoteFile::Newer(_) => (SyncState::Error, Action::None),
        RemoteFile::Ok(_) => {
            let remote_hash = remote_hash.unwrap_or_default();
            if local_hash == remote_hash {
                let action = if base == Some(local_hash) {
                    Action::None
                } else {
                    Action::MarkBase
                };
                (SyncState::Clean, action)
            } else if base.is_none() {
                if local_empty {
                    (SyncState::RemoteChanged, Action::Adopt)
                } else {
                    (SyncState::Diverged, Action::None)
                }
            } else if base == Some(local_hash) {
                (SyncState::RemoteChanged, Action::Adopt)
            } else if base == Some(remote_hash) {
                (SyncState::LocalDirty, Action::Push)
            } else {
                (SyncState::Diverged, Action::None)
            }
        }
    }
}

fn log(message: &str) {
    // Nunca inclui conteúdo do workspace, caminhos ou credenciais.
    eprintln!("[lkr-sync] {message}");
}

struct Local {
    ws: PortableWorkspace,
    hash: String,
    empty: bool,
    meta: SyncMeta,
}

fn read_local(db: &Mutex<Database>) -> HubResult<Local> {
    let db = db
        .lock()
        .map_err(|_| "Banco temporariamente indisponível")?;
    let ws = db.export_portable()?;
    Ok(Local {
        hash: portable::content_hash(&ws),
        empty: portable::is_empty(&ws),
        meta: db.sync_meta()?,
        ws,
    })
}

fn status_of(local: &Local, state: SyncState, message: &str, code: Option<&str>) -> SyncStatus {
    SyncStatus {
        state,
        message: message.into(),
        code: code.map(str::to_string),
        local_hash: local.hash.clone(),
        remote_hash: None,
        base_hash: local.meta.base_hash.clone(),
        last_synced_at: local.meta.last_synced_at.clone(),
        checked_remote: false,
        pushed: false,
        applied: false,
        preferences: None,
    }
}

/// Só compara com a base: instantâneo, sem rede (usado na abertura e a cada edição).
pub fn local_status(db: &Mutex<Database>) -> HubResult<SyncStatus> {
    let local = read_local(db)?;
    let (state, message) = match local.meta.base_hash.as_deref() {
        Some(base) if base == local.hash => (SyncState::Clean, "Sincronizado"),
        None if local.empty => (SyncState::Clean, "Nada para sincronizar ainda"),
        _ => (
            SyncState::LocalDirty,
            "Alterações locais ainda não sincronizadas",
        ),
    };
    Ok(status_of(&local, state, message, None))
}

fn friendly(error: &BridgeError) -> (SyncState, String, String) {
    match error {
        BridgeError::Unavailable(detail) => {
            log(&format!("bridge indisponível: {detail}"));
            (
                SyncState::Offline,
                "Bridge indisponível. Inicie-o com npm run lab; o workspace local segue funcionando.".into(),
                "BRIDGE_DOWN".into(),
            )
        }
        BridgeError::Rejected { code, message } => {
            let state = if code == "NETWORK" {
                SyncState::Offline
            } else {
                SyncState::Error
            };
            let text = match code.as_str() {
                "NETWORK" => "Sem conexão com o GitHub.".to_string(),
                "AUTH" => "O Git não conseguiu autenticar no GitHub.".to_string(),
                "DIVERGED" => "O repositório Git local e o remoto divergiram; resolva no Git antes de sincronizar.".to_string(),
                _ => message.clone(),
            };
            (state, text, code.clone())
        }
    }
}

fn describe_invalid(file: &RemoteFile) -> Option<(String, &'static str)> {
    match file {
        RemoteFile::Invalid(why) => Some((
            format!(
                "O workspace no repositório é inválido e não será aplicado nem sobrescrito: {why}"
            ),
            "REMOTE_INVALID",
        )),
        RemoteFile::Newer(why) => Some((
            format!("{why} Atualize o LKR LAB; nada foi aplicado."),
            "REMOTE_NEWER",
        )),
        _ => None,
    }
}

/// Consulta o remoto (via bridge) e informa o estado, sem alterar nada.
pub fn status(db: &Mutex<Database>, bridge: &dyn Bridge) -> HubResult<SyncStatus> {
    let local = read_local(db)?;
    let remote = match bridge.remote() {
        Ok(remote) => remote,
        Err(error) => return Ok(offline_or_error(&local, &error)),
    };
    Ok(evaluate(&local, &remote).0)
}

fn offline_or_error(local: &Local, error: &BridgeError) -> SyncStatus {
    let (state, message, code) = friendly(error);
    let mut status = local_status_from(local);
    status.state = state;
    status.message = message;
    status.code = Some(code);
    status
}

fn local_status_from(local: &Local) -> SyncStatus {
    let state = match local.meta.base_hash.as_deref() {
        Some(base) if base == local.hash => SyncState::Clean,
        None if local.empty => SyncState::Clean,
        _ => SyncState::LocalDirty,
    };
    status_of(local, state, "", None)
}

/// Estado + ação para o que o remoto informou.
fn evaluate(local: &Local, remote: &RemoteInfo) -> (SyncStatus, Action, Option<PortableWorkspace>) {
    if !remote.has_remote {
        let status = status_of(
            local,
            SyncState::Error,
            "O repositório não tem remote “origin”: não há para onde sincronizar.",
            Some("NO_REMOTE"),
        );
        return (status, Action::None, None);
    }
    if !remote.fetched {
        let (_, message, code) = remote
            .fetch_error
            .as_ref()
            .map(|(code, message)| {
                friendly(&BridgeError::Rejected {
                    code: code.clone(),
                    message: message.clone(),
                })
            })
            .unwrap_or((
                SyncState::Offline,
                "Sem conexão com o GitHub.".into(),
                "NETWORK".into(),
            ));
        let mut status = local_status_from(local);
        status.state = SyncState::Offline;
        status.message = message;
        status.code = Some(code);
        return (status, Action::None, None);
    }
    if let Some((message, code)) = describe_invalid(&remote.file) {
        return (
            status_of(local, SyncState::Error, &message, Some(code)),
            Action::None,
            None,
        );
    }
    let (remote_hash, remote_ws) = match &remote.file {
        RemoteFile::Ok(ws) => {
            let mut ws = (**ws).clone();
            portable::normalize(&mut ws);
            match portable::validate(&ws) {
                Ok(()) => (Some(portable::content_hash(&ws)), Some(ws)),
                Err(why) => {
                    return (
                        status_of(
                            local,
                            SyncState::Error,
                            &format!("{why}. Nada foi aplicado."),
                            Some("REMOTE_INVALID"),
                        ),
                        Action::None,
                        None,
                    )
                }
            }
        }
        _ => (None, None),
    };
    let (state, action) = decide(
        &local.hash,
        local.empty,
        local.meta.base_hash.as_deref(),
        &remote.file,
        remote_hash.as_deref(),
    );
    let message = match state {
        SyncState::Clean => "Sincronizado",
        SyncState::LocalDirty => "Alterações locais prontas para enviar",
        SyncState::RemoteChanged => "Atualização disponível",
        SyncState::Diverged => "Conflito de sincronização: este computador e o repositório mudaram. Nada foi sobrescrito.",
        _ => "",
    };
    let mut status = status_of(local, state, message, None);
    status.remote_hash = remote_hash;
    status.checked_remote = true;
    (status, action, remote_ws)
}

/// Um ciclo explícito de sync. Nunca escolhe vencedor sozinho numa divergência.
pub fn sync(
    db: &Mutex<Database>,
    bridge: &dyn Bridge,
    resolution: Option<Resolution>,
    running: &AtomicBool,
) -> HubResult<SyncStatus> {
    if running.swap(true, Ordering::SeqCst) {
        return Err("Já existe uma sincronização em andamento.".into());
    }
    let result = run(db, bridge, resolution);
    running.store(false, Ordering::SeqCst);
    result
}

fn run(
    db: &Mutex<Database>,
    bridge: &dyn Bridge,
    resolution: Option<Resolution>,
) -> HubResult<SyncStatus> {
    log("sync started");
    let local = read_local(db)?;
    let remote = match bridge.remote() {
        Ok(remote) => remote,
        Err(error) => return Ok(offline_or_error(&local, &error)),
    };
    let (mut status, mut action, remote_ws) = evaluate(&local, &remote);
    if status.checked_remote {
        log("bridge connected");
    }
    if status.state == SyncState::Diverged {
        match resolution {
            None => {
                log("divergence detected");
                return Ok(status);
            }
            Some(Resolution::TakeRemote) => action = Action::Adopt,
            Some(Resolution::KeepLocal) => action = Action::Push,
        }
    }
    match action {
        Action::None => Ok(status),
        Action::MarkBase => {
            db.lock()
                .map_err(|_| "Banco temporariamente indisponível")?
                .mark_synced(&local.hash)?;
            log("sync completed (already equal)");
            let stamp = last_synced(db)?;
            Ok(refresh(status, local.hash.clone(), stamp))
        }
        Action::Adopt => {
            let Some(ws) = remote_ws else {
                return Ok(status);
            };
            let remote_hash = status.remote_hash.clone().unwrap_or_default();
            log("remote changed");
            let mut guard = db
                .lock()
                .map_err(|_| "Banco temporariamente indisponível")?;
            // O usuário pode ter editado enquanto o bridge trabalhava: não aplicar por cima.
            let now = guard.export_portable()?;
            if portable::content_hash(&now) != local.hash {
                status.state = SyncState::Error;
                status.code = Some("CHANGED_DURING_SYNC".into());
                status.message = "O workspace mudou durante a sincronização. Tente de novo.".into();
                return Ok(status);
            }
            match guard.apply_synced(&ws, &remote_hash) {
                Ok(_) => {
                    log("apply successful");
                    status.state = SyncState::Clean;
                    status.message = "Sincronizado agora (atualização aplicada)".into();
                    status.applied = true;
                    status.preferences = Some(ws.preferences.clone());
                    status.base_hash = Some(remote_hash.clone());
                    status.local_hash = remote_hash;
                    status.last_synced_at = guard.sync_meta()?.last_synced_at;
                    Ok(status)
                }
                Err(why) => {
                    log("apply failed");
                    status.state = SyncState::Error;
                    status.code = Some("APPLY_FAILED".into());
                    status.message = format!("Não foi possível aplicar o workspace: {why}");
                    Ok(status)
                }
            }
        }
        Action::Push => {
            let seen = status.remote_hash.clone();
            let outcome = match bridge.push(&local.ws) {
                Err(BridgeError::Rejected { code, .. }) if code == "REMOTE_AHEAD" => {
                    // O remoto andou por outros arquivos. Atualiza só se der para fazer
                    // fast-forward sem mexer no trabalho do desenvolvedor.
                    if let Err(error) = bridge.update() {
                        return Ok(offline_or_error(&local, &error));
                    }
                    match bridge.remote() {
                        Ok(again) => {
                            let hash = match &again.file {
                                RemoteFile::Ok(ws) => {
                                    let mut ws = (**ws).clone();
                                    portable::normalize(&mut ws);
                                    Some(portable::content_hash(&ws))
                                }
                                _ => None,
                            };
                            if hash != seen {
                                status.state = SyncState::Error;
                                status.code = Some("REMOTE_MOVED".into());
                                status.message =
                                    "O repositório mudou durante a sincronização. Tente de novo."
                                        .into();
                                return Ok(status);
                            }
                        }
                        Err(error) => return Ok(offline_or_error(&local, &error)),
                    }
                    bridge.push(&local.ws)
                }
                other => other,
            };
            match outcome {
                Ok(_) => {
                    db.lock()
                        .map_err(|_| "Banco temporariamente indisponível")?
                        .mark_synced(&local.hash)?;
                    log("sync completed (pushed)");
                    let stamp = last_synced(db)?;
                    let mut done = refresh(status, local.hash.clone(), stamp);
                    done.pushed = true;
                    done.message = "Sincronizado agora".into();
                    Ok(done)
                }
                Err(error) => {
                    log("sync failed");
                    Ok(offline_or_error(&local, &error))
                }
            }
        }
    }
}

fn last_synced(db: &Mutex<Database>) -> HubResult<Option<String>> {
    Ok(db
        .lock()
        .map_err(|_| "Banco temporariamente indisponível")?
        .sync_meta()?
        .last_synced_at)
}

fn refresh(mut status: SyncStatus, hash: String, stamp: Option<String>) -> SyncStatus {
    status.state = SyncState::Clean;
    status.message = "Sincronizado".into();
    status.base_hash = Some(hash.clone());
    status.remote_hash = Some(hash);
    status.last_synced_at = stamp;
    status
}
