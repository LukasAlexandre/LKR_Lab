//! Repository Locator: como reconhecer que uma pasta desta máquina corresponde a um Project
//! que o LKR LAB já conhece, SEM usar o caminho local como identidade.
//!
//!   PROJECT ID (UUID)  ≠  REPOSITORY LOCATOR  ≠  LOCAL BINDING (caminho desta máquina)
//!
//! O Project ID continua sendo a identidade canônica. O locator é uma identidade AUXILIAR,
//! opcional e portátil: o remote Git canônico + o caminho do projeto DENTRO do repositório
//! (vazio na raiz). Num monorepo, `apps/web` e `apps/api` têm o mesmo remote e caminhos
//! diferentes, então são Projects diferentes. Sem remote reconhecível (ou sem Git) não existe
//! locator e nunca há deduplicação automática: nome igual não é identidade, caminho igual
//! não é identidade portátil.
//!
//! Tudo aqui é leitura: nenhum `fetch`, `checkout` ou script. O locator nunca guarda caminho
//! absoluto, usuário ou credencial.
use crate::{commands, HubResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

const MAX_REMOTE: usize = 500;
/// Hosts cujo caminho não diferencia maiúsculas de minúsculas (e onde `www.` é o mesmo host).
const CASE_INSENSITIVE_HOSTS: [&str; 3] = ["github.com", "gitlab.com", "bitbucket.org"];

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryLocator {
    /// `host[:porta]/dono/repo`, sem esquema, credencial nem `.git`.
    pub remote: String,
    /// Caminho do projeto relativo à raiz do repositório, com `/`. Vazio = raiz.
    #[serde(default)]
    pub path: String,
}

impl RepositoryLocator {
    /// Texto curto para a interface: `github.com/org/repo` ou `github.com/org/repo › apps/web`.
    pub fn display(&self) -> String {
        if self.path.is_empty() {
            self.remote.clone()
        } else {
            format!("{} › {}", self.remote, self.path)
        }
    }
}

/// Remote reconhecido: identidade canônica + URL https para exibição/abrir no navegador.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedRemote {
    pub canonical: String,
    /// Só quando é seguro afirmar que existe uma página https (origem https, ou host conhecido).
    pub https: Option<String>,
}

fn host_chars_ok(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && !host.starts_with(['.', '-'])
}

fn path_chars_ok(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 200
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '~' | '+' | '%'))
}

/// Normaliza a URL de um remote Git. `None` quando não é um remote de rede reconhecível
/// (caminho local, `file://`, esquema desconhecido, texto estranho).
///
/// Equivalências (mesmo repositório):
/// `git@github.com:org/repo.git`, `ssh://git@github.com/org/repo`, `https://github.com/org/repo.git`
/// e `https://user:token@github.com/org/repo/` → `github.com/org/repo`.
///
/// Porta explícita (fora do padrão do esquema) FAZ PARTE da identidade: `ssh://host:2222/x/y` não
/// é `https://host/x/y`. Hosts desconhecidos não têm o caminho em minúsculas nem perdem `www.`.
pub fn normalize_remote(url: &str) -> Option<NormalizedRemote> {
    let url = url.trim();
    if url.is_empty()
        || url.len() > MAX_REMOTE
        || url.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return None;
    }
    let scp_like = !url.contains("://");
    let scheme = if scp_like {
        // scp-like: [usuario@]host:caminho (sem barra antes dos dois pontos).
        let (left, path) = url.split_once(':')?;
        if left.contains('/') || left.contains('\\') || path.starts_with('/') || path.contains('\\')
        {
            return None; // caminho local, drive do Windows ou caminho absoluto no host
        }
        if left.len() == 1 {
            return None; // "C:foo": letra de drive
        }
        "ssh".to_string()
    } else {
        url.split_once("://")?.0.to_ascii_lowercase()
    };
    let (default_port, https_origin) = match scheme.as_str() {
        "https" => (443, true),
        "http" => (80, false),
        "ssh" | "git+ssh" | "ssh+git" | "git" => (22, false),
        _ => return None,
    };
    let (authority, raw_path) = if scp_like {
        // O host vem antes do primeiro ':' e o caminho depois dele.
        url.split_once(':')?
    } else {
        let rest = url.split_once("://")?.1;
        rest.split_once('/').unwrap_or((rest, ""))
    };
    // Sem credencial: tudo até o último '@' é usuário[:senha].
    let host_port = authority
        .rsplit_once('@')
        .map(|(_, rest)| rest)
        .unwrap_or(authority);
    if host_port.contains(['[', ']']) {
        return None;
    }
    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port)) if !scp_like => {
            (host, Some(port.parse::<u16>().ok().filter(|p| *p != 0)?))
        }
        _ => (host_port, None),
    };
    let mut host = host.to_ascii_lowercase();
    while host.ends_with('.') {
        host.pop();
    }
    if !host_chars_ok(&host) {
        return None;
    }
    let known = CASE_INSENSITIVE_HOSTS
        .iter()
        .any(|h| host == *h || host == format!("www.{h}"));
    if known {
        host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    }
    if raw_path.contains(['?', '#']) {
        return None;
    }
    let mut segments: Vec<&str> = raw_path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() || segments.iter().any(|s| *s == "." || *s == "..") {
        return None;
    }
    let last = segments.len() - 1;
    let trimmed = segments[last]
        .strip_suffix(".git")
        .unwrap_or(segments[last]);
    segments[last] = trimmed;
    if segments.iter().any(|s| !path_chars_ok(s)) {
        return None;
    }
    let path = segments.join("/");
    let path = if known {
        path.to_ascii_lowercase()
    } else {
        path
    };
    let port = port.filter(|p| *p != default_port);
    let canonical = match port {
        Some(p) => format!("{host}:{p}/{path}"),
        None => format!("{host}/{path}"),
    };
    // Exibição https: origem https (sem porta de ssh), ou host conhecido (github/gitlab/bitbucket).
    let https = (https_origin || (known && port.is_none())).then(|| format!("https://{canonical}"));
    Some(NormalizedRemote { canonical, https })
}

/// Um remote do repositório (nome + URL como está na configuração do Git).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

/// Resultado da escolha do remote principal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteChoice {
    pub name: Option<String>,
    pub remote: Option<NormalizedRemote>,
    /// Por que não há remote principal (ou como foi escolhido), para a interface.
    pub note: Option<String>,
}

/// Remote principal, de forma determinística:
/// 1. `origin`, se existir e for um remote de rede reconhecível;
/// 2. senão, o remote do upstream do branch atual (`branch.<nome>.remote`);
/// 3. senão, o único remote reconhecível;
/// 4. senão NÃO há remote principal (vários candidatos e nenhum desempata): sem locator.
///    Adivinhar entre `origin`/`upstream`/`company` criaria identidade errada.
pub fn choose_remote(remotes: &[Remote], upstream_remote: Option<&str>) -> RemoteChoice {
    let usable: Vec<(&Remote, NormalizedRemote)> = remotes
        .iter()
        .filter_map(|r| normalize_remote(&r.url).map(|n| (r, n)))
        .collect();
    let pick = |name: &str| {
        usable
            .iter()
            .find(|(r, _)| r.name == name)
            .map(|(r, n)| (r.name.clone(), n.clone()))
    };
    let chosen = pick("origin")
        .or_else(|| upstream_remote.and_then(pick))
        .or_else(|| {
            if usable.len() == 1 {
                Some((usable[0].0.name.clone(), usable[0].1.clone()))
            } else {
                None
            }
        });
    match chosen {
        Some((name, remote)) => RemoteChoice { name: Some(name), remote: Some(remote), note: None },
        None if remotes.is_empty() => RemoteChoice { name: None, remote: None, note: Some("O repositório não tem remote: sem identidade portátil (nenhuma deduplicação entre máquinas).".into()) },
        None if usable.is_empty() => RemoteChoice { name: None, remote: None, note: Some("Nenhum remote é um endereço de rede reconhecível: sem identidade portátil.".into()) },
        None => RemoteChoice {
            name: None,
            remote: None,
            note: Some(format!(
                "Vários remotes ({}) e nenhum é “origin”: não dá para escolher um com segurança; sem identidade portátil.",
                usable.iter().map(|(r, _)| r.name.as_str()).collect::<Vec<_>>().join(", ")
            )),
        },
    }
}

/// Caminho relativo à raiz do repositório vindo de `git rev-parse --show-prefix`.
/// Rejeita o que não for um caminho relativo simples (nunca guarda `..`, `\` nem absoluto).
pub fn clean_prefix(prefix: &str) -> Option<String> {
    let normalized = prefix.trim().replace('\\', "/");
    let trimmed = normalized.trim_matches('/');
    if trimmed.is_empty() {
        return Some(String::new());
    }
    let ok = trimmed.len() <= 500
        && !trimmed.contains(':')
        && trimmed
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != ".." && !s.chars().any(|c| c.is_control()));
    ok.then(|| trimmed.to_string())
}

/// Fatos do Git de uma pasta, lidos só com comandos de leitura.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitFacts {
    pub is_repo: bool,
    /// Caminho da pasta dentro do repositório ("" na raiz).
    pub prefix: String,
    pub remotes: Vec<Remote>,
    pub upstream_remote: Option<String>,
    /// Avisos de leitura (Git ausente, pasta de outro dono…).
    pub warning: Option<String>,
}

/// Lê remotes e posição da pasta no repositório: `rev-parse --show-toplevel/--show-prefix`,
/// `config --get-regexp` e `config --get`. Nada disso busca na rede, altera árvore ou executa
/// código do projeto (`core.fsmonitor` é desligado por `commands::run`).
pub fn read_git_facts(dir: &Path) -> GitFacts {
    if commands::executable("git").is_none() {
        return GitFacts {
            warning: Some("Git não encontrado no PATH: a inspeção não lê o repositório.".into()),
            ..GitFacts::default()
        };
    }
    if commands::run("git", &["rev-parse", "--show-toplevel"], Some(dir)).is_err() {
        return GitFacts::default(); // não é um repositório (ou o Git recusou a pasta)
    }
    let prefix = commands::run("git", &["rev-parse", "--show-prefix"], Some(dir))
        .ok()
        .and_then(|p| clean_prefix(&p));
    let remotes = commands::run(
        "git",
        &["config", "--get-regexp", r"^remote\..*\.url$"],
        Some(dir),
    )
    .map(|text| parse_remote_config(&text))
    .unwrap_or_default();
    let upstream_remote =
        commands::run("git", &["symbolic-ref", "--short", "-q", "HEAD"], Some(dir))
            .ok()
            .filter(|b| !b.is_empty() && !b.starts_with('-'))
            .and_then(|branch| {
                commands::run(
                    "git",
                    &["config", "--get", &format!("branch.{branch}.remote")],
                    Some(dir),
                )
                .ok()
            })
            .filter(|r| !r.is_empty() && r != ".");
    GitFacts {
        is_repo: true,
        warning: prefix
            .is_none()
            .then(|| "Não foi possível determinar a posição da pasta no repositório.".to_string()),
        prefix: prefix.unwrap_or_default(),
        remotes,
        upstream_remote,
    }
}

/// `remote.origin.url git@host:org/repo.git` → ("origin", url). Aceita nomes com ponto.
pub fn parse_remote_config(text: &str) -> Vec<Remote> {
    let mut remotes: Vec<Remote> = Vec::new();
    for line in text.lines() {
        let Some((key, url)) = line.split_once(' ') else {
            continue;
        };
        let Some(name) = key
            .strip_prefix("remote.")
            .and_then(|k| k.strip_suffix(".url"))
        else {
            continue;
        };
        if name.is_empty() || name.len() > 100 || remotes.iter().any(|r| r.name == name) {
            continue;
        }
        remotes.push(Remote {
            name: name.to_string(),
            url: url.trim().to_string(),
        });
        if remotes.len() >= 20 {
            break;
        }
    }
    remotes
}

/// O que sai dos fatos do Git: locator (se houver) + remote escolhido + motivo quando não há.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatorResult {
    pub locator: Option<RepositoryLocator>,
    pub remote_name: Option<String>,
    /// URL https para exibir/abrir; vazia quando não há como afirmar uma.
    pub https: Option<String>,
    pub note: Option<String>,
}

pub fn locator_from(facts: &GitFacts) -> LocatorResult {
    if !facts.is_repo {
        return LocatorResult {
            locator: None,
            remote_name: None,
            https: None,
            note: facts.warning.clone(),
        };
    }
    let choice = choose_remote(&facts.remotes, facts.upstream_remote.as_deref());
    match choice.remote {
        Some(remote) => LocatorResult {
            locator: Some(RepositoryLocator {
                remote: remote.canonical,
                path: facts.prefix.clone(),
            }),
            remote_name: choice.name,
            https: remote.https,
            note: facts.warning.clone(),
        },
        None => LocatorResult {
            locator: None,
            remote_name: None,
            https: None,
            note: choice.note,
        },
    }
}

/// Atalho: locator de uma pasta (usado no cadastro e no vínculo). `Ok(None)` = sem identidade.
pub fn locator_of(dir: &Path) -> HubResult<LocatorResult> {
    Ok(locator_from(&read_git_facts(dir)))
}
