use crate::commands;
use serde::Serialize;
use std::path::Path;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentContext {
    pub provider: String,
    pub available: bool,
    pub account_status: String,
    pub instructions: Vec<String>,
    pub skills: Vec<String>,
    pub mcp_status: String,
    pub sessions_status: String,
}
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentProviderStatus {
    pub provider: String,
    pub availability: String,
    pub account_status: String,
    pub usage_status: String,
    pub sessions_status: String,
    pub detail: String,
}

// Detection only: npm CLIs can expose a .cmd shim. Never execute it here.
fn cli_present(name: &str) -> bool {
    commands::executable(name).is_some()
        || (cfg!(windows)
            && std::env::var_os("PATH").is_some_and(|path| {
                std::env::split_paths(&path)
                    .filter(|dir| dir.is_absolute())
                    .any(|dir| dir.join(format!("{name}.cmd")).is_file())
            }))
}

pub fn providers() -> Vec<AgentProviderStatus> {
    [
        (
            "Codex",
            "codex",
            "CLI local detectado; conta e métricas não são lidas",
        ),
        (
            "Claude",
            "claude",
            "CLI local detectado; conta e métricas não são lidas",
        ),
    ]
    .into_iter()
    .map(|(provider, executable, detail)| {
        let available = cli_present(executable);
        AgentProviderStatus {
            provider: provider.into(),
            availability: if available {
                "available"
            } else {
                "unavailable"
            }
            .into(),
            account_status: "unsupported".into(),
            usage_status: "unsupported".into(),
            sessions_status: "unsupported".into(),
            detail: if available {
                detail
            } else {
                "CLI não encontrado neste ambiente"
            }
            .into(),
        }
    })
    .chain(std::iter::once(AgentProviderStatus {
        provider: "ChatGPT".into(),
        availability: "unsupported".into(),
        account_status: "unsupported".into(),
        usage_status: "unsupported".into(),
        sessions_status: "unsupported".into(),
        detail: "Nenhum adapter local seguro disponível; métricas não são inventadas".into(),
    }))
    .collect()
}
pub trait AgentProvider {
    fn context(&self, path: &Path) -> AgentContext;
}
pub struct ClaudeProvider;
impl AgentProvider for ClaudeProvider {
    fn context(&self, path: &Path) -> AgentContext {
        let instructions = ["CLAUDE.md", "AGENTS.md"]
            .iter()
            .filter(|f| path.join(f).is_file())
            .map(|f| (*f).to_string())
            .collect();
        let skills = std::fs::read_dir(path.join(".claude/skills"))
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .take(200)
            .filter(|e| e.path().join("SKILL.md").is_file())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        AgentContext {
            provider: "Claude".into(),
            available: commands::executable("claude").is_some(),
            account_status: "Não verificado; credenciais não são lidas".into(),
            instructions,
            skills,
            mcp_status: if path.join(".mcp.json").is_file() {
                "Configuração encontrada; conteúdo e credenciais não lidos"
            } else {
                "Não configurado"
            }
            .into(),
            sessions_status: "Provider preparado; descoberta de sessões não implementada".into(),
        }
    }
}
