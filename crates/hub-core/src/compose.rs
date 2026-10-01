//! Docker / Compose do projeto.
//!
//! DOCKERFILE SOZINHO != PROJETO COMPOSE. Só o Compose tem ações (ver docs/RUNTIME.md).
//!
//! Os serviços e as portas vêm de `docker compose config --format json` — a fonte autoritativa
//! (variáveis, `extends`, `include`, profiles já resolvidos), sem iniciar nenhum container. Essa
//! saída traz `environment` e `env_file` em texto puro, então o JSON é desserializado SÓ para os
//! campos abaixo (nome, portas, profiles) e descartado: nada dele é guardado, registrado ou
//! enviado ao contexto de IA. O estado dos serviços vem de `docker compose ps`, nunca do YAML.
use crate::tools::capture;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};

/// Ordem de precedência do próprio Compose quando não se passa `-f`.
pub const COMPOSE_FILES: [&str; 4] = [
    "compose.yaml",
    "compose.yml",
    "docker-compose.yaml",
    "docker-compose.yml",
];
pub const OVERRIDE_FILES: [&str; 4] = [
    "compose.override.yaml",
    "compose.override.yml",
    "docker-compose.override.yaml",
    "docker-compose.override.yml",
];
const MAX_SERVICES: usize = 100;
const CONFIG_TTL: Duration = Duration::from_secs(60);
const PS_TTL: Duration = Duration::from_secs(3);
const CONFIG_TIMEOUT: Duration = Duration::from_secs(10);
const PS_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_OUTPUT: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerInfo {
    pub dockerfile: bool,
    /// Arquivos Compose na ordem de precedência do Compose.
    pub compose_files: Vec<String>,
    pub override_files: Vec<String>,
    /// "compose" | "dockerfile"
    pub kind: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposePort {
    pub published: Option<u16>,
    pub target: u16,
    pub protocol: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ComposeService {
    pub name: String,
    pub ports: Vec<ComposePort>,
    pub profiles: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeConfig {
    pub project_name: Option<String>,
    pub services: Vec<ComposeService>,
}

/// Container como o `compose ps` o descreve (estado real, não o YAML).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    pub service: String,
    /// running | exited | created | paused | restarting | dead | removing
    pub state: String,
    pub health: Option<String>,
    pub exit_code: Option<i32>,
}

/// Nome de serviço do Compose: começa com alfanumérico; letras, números, `.`, `_` e `-`.
pub fn valid_service_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

pub fn detect_files(root: &Path) -> Option<DockerInfo> {
    let present = |names: &[&str]| -> Vec<String> {
        names
            .iter()
            .filter(|n| root.join(n).is_file())
            .map(|n| n.to_string())
            .collect()
    };
    let compose_files = present(&COMPOSE_FILES);
    let dockerfile = root.join("Dockerfile").is_file();
    if compose_files.is_empty() && !dockerfile {
        return None;
    }
    let kind = if compose_files.is_empty() {
        "dockerfile"
    } else {
        "compose"
    };
    Some(DockerInfo {
        dockerfile,
        compose_files,
        override_files: present(&OVERRIDE_FILES),
        kind,
    })
}

// ---------------------------------------------------------------- config

#[derive(Deserialize)]
struct RawConfig {
    name: Option<String>,
    #[serde(default)]
    services: BTreeMap<String, RawService>,
}
#[derive(Deserialize)]
struct RawService {
    #[serde(default)]
    ports: Vec<RawPort>,
    #[serde(default)]
    profiles: Vec<String>,
}
#[derive(Deserialize)]
struct RawPort {
    published: Option<serde_json::Value>,
    target: Option<u16>,
    protocol: Option<String>,
}

fn published(value: &Option<serde_json::Value>) -> Option<u16> {
    match value.as_ref()? {
        serde_json::Value::Number(n) => n.as_u64().and_then(|n| u16::try_from(n).ok()),
        serde_json::Value::String(s) => s.trim().parse::<u16>().ok(), // "8000-8010" (faixa) não vira porta
        _ => None,
    }
}

/// Lê a saída de `docker compose config --format json`. Só o que a interface precisa.
pub fn parse_config(json: &str) -> Result<ComposeConfig, String> {
    let raw: RawConfig = serde_json::from_str(json)
        .map_err(|_| "Saída do Compose em formato inesperado.".to_string())?;
    let mut services = Vec::new();
    for (name, service) in raw.services {
        if !valid_service_name(&name) || services.len() >= MAX_SERVICES {
            continue;
        }
        let mut ports: Vec<ComposePort> = service
            .ports
            .iter()
            .filter_map(|p| {
                Some(ComposePort {
                    published: published(&p.published),
                    target: p.target?,
                    protocol: p
                        .protocol
                        .clone()
                        .unwrap_or_else(|| "tcp".into())
                        .chars()
                        .take(8)
                        .collect(),
                })
            })
            .collect();
        ports.truncate(32);
        let profiles = service
            .profiles
            .into_iter()
            .filter(|p| valid_service_name(p))
            .take(16)
            .collect();
        services.push(ComposeService {
            name,
            ports,
            profiles,
        });
    }
    Ok(ComposeConfig {
        project_name: raw.name.filter(|n| valid_service_name(n)),
        services,
    })
}

/// Primeira linha útil do erro do Docker, curta e sem caminhos longos.
fn short_error(stderr: &str) -> String {
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("sem detalhes");
    line.chars().take(300).collect()
}

type ConfigCache = Mutex<
    HashMap<
        PathBuf,
        (
            Instant,
            Vec<Option<(u64, SystemTime)>>,
            Result<ComposeConfig, String>,
        ),
    >,
>;
fn fingerprint(root: &Path) -> Vec<Option<(u64, SystemTime)>> {
    COMPOSE_FILES
        .iter()
        .chain(OVERRIDE_FILES.iter())
        .chain([".env"].iter())
        .map(|name| {
            std::fs::metadata(root.join(name))
                .ok()
                .map(|m| (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
        })
        .collect()
}

/// Serviços do projeto (todos os profiles). Em cache por arquivo + TTL; erro também é cacheado
/// por pouco tempo para não repetir o comando a cada atualização da tela.
pub fn config(root: &Path, docker: &Path) -> Result<ComposeConfig, String> {
    static CACHE: OnceLock<ConfigCache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let print = fingerprint(root);
    if let Some((at, cached_print, result)) =
        cache.lock().unwrap_or_else(|e| e.into_inner()).get(root)
    {
        let ttl = if result.is_ok() {
            CONFIG_TTL
        } else {
            Duration::from_secs(10)
        };
        if *cached_print == print && at.elapsed() < ttl {
            return result.clone();
        }
    }
    let result = run_config(root, docker);
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(root.to_path_buf(), (Instant::now(), print, result.clone()));
    result
}

fn run_config(root: &Path, docker: &Path) -> Result<ComposeConfig, String> {
    let mut args = vec![
        "compose",
        "--profile",
        "*",
        "config",
        "--format",
        "json",
        "--no-env-resolution",
    ];
    let mut out = capture(docker, &args, root, CONFIG_TIMEOUT, MAX_OUTPUT)
        .map_err(|e| format!("Não foi possível ler o Compose: {e}."))?;
    if out.code != Some(0) && out.stderr.contains("--no-env-resolution") {
        args.pop(); // Compose antigo sem a opção: segue sem ela (a saída continua sendo filtrada).
        out = capture(docker, &args, root, CONFIG_TIMEOUT, MAX_OUTPUT)
            .map_err(|e| format!("Não foi possível ler o Compose: {e}."))?;
    }
    if out.code != Some(0) {
        return Err(format!(
            "Arquivo Compose inválido: {}",
            short_error(&out.stderr)
        ));
    }
    parse_config(&out.stdout)
}

// ---------------------------------------------------------------- ps

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawContainer {
    service: Option<String>,
    state: Option<String>,
    health: Option<String>,
    exit_code: Option<i32>,
}

/// `compose ps --format json`: uma linha JSON por container (v2.21+) ou um array.
pub fn parse_ps(text: &str) -> Result<Vec<Container>, String> {
    let text = text.trim();
    let raws: Vec<RawContainer> = if text.is_empty() {
        vec![]
    } else if text.starts_with('[') {
        serde_json::from_str(text)
            .map_err(|_| "Saída do compose ps em formato inesperado.".to_string())?
    } else {
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                serde_json::from_str(l)
                    .map_err(|_| "Saída do compose ps em formato inesperado.".to_string())
            })
            .collect::<Result<_, _>>()?
    };
    Ok(raws
        .into_iter()
        .filter_map(|c| {
            let service = c.service.filter(|s| valid_service_name(s))?;
            Some(Container {
                service,
                state: c
                    .state
                    .unwrap_or_default()
                    .to_lowercase()
                    .chars()
                    .take(16)
                    .collect(),
                health: c
                    .health
                    .filter(|h| !h.is_empty())
                    .map(|h| h.to_lowercase().chars().take(16).collect()),
                exit_code: c.exit_code,
            })
        })
        .take(500)
        .collect())
}

type PsCache = Mutex<HashMap<PathBuf, (Instant, Result<Vec<Container>, String>)>>;
fn ps_cache() -> &'static PsCache {
    static CACHE: OnceLock<PsCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Containers do projeto agora. Em cache por poucos segundos (várias telas pedem juntas).
pub fn ps(root: &Path, docker: &Path) -> Result<Vec<Container>, String> {
    if let Some((at, result)) = ps_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(root)
    {
        if at.elapsed() < PS_TTL {
            return result.clone();
        }
    }
    let result = match capture(
        docker,
        &["compose", "ps", "--all", "--format", "json"],
        root,
        PS_TIMEOUT,
        MAX_OUTPUT,
    ) {
        Ok(out) if out.code == Some(0) => parse_ps(&out.stdout),
        Ok(out) => Err(format!(
            "Não foi possível consultar os containers: {}",
            short_error(&out.stderr)
        )),
        Err(error) => Err(format!(
            "Não foi possível consultar os containers: {error}."
        )),
    };
    ps_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(root.to_path_buf(), (Instant::now(), result.clone()));
    result
}

/// Depois de subir/parar/reiniciar, a próxima leitura precisa ser fresca.
pub fn invalidate(root: &Path) {
    ps_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(root);
}
