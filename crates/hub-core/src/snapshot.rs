use crate::HubResult;
use crate::{
    agents::{AgentProvider, ClaudeProvider},
    git,
    models::Project,
    ports,
    runtime::{ProjectRuntime, ScriptKind},
};
pub fn generate(p: &Project, runtime: &ProjectRuntime) -> HubResult<String> {
    let dir = crate::projects::local_dir(p)?;
    let path = dir.as_path();
    let state = git::inspect(path);
    let git=match state {Ok(g)=>format!("Branch: {}\nHEAD: {}\nWorking tree: {}\nStaged: {} / Unstaged: {} / Untracked: {}\nAhead: {:?} / Behind: {:?}\n\nCommits recentes:\n{}",g.branch,g.head,if g.clean{"CLEAN"}else{"CHANGES"},g.staged,g.unstaged,g.untracked,g.ahead,g.behind,g.commits.iter().map(|c|format!("- {} {}",c.hash,c.subject)).collect::<Vec<_>>().join("\n")),Err(e)=>format!("NOT VERIFIED: {e}")};
    let agent = ClaudeProvider.context(path);
    let observed = match ports::inspect(std::slice::from_ref(p)) {
        Ok(ports) => ports
            .iter()
            .filter(|port| p.ports.iter().any(|expected| expected.port == port.port))
            .map(|port| {
                format!(
                    "- {} {} PID {:?}: ocupada; vínculo {}",
                    port.protocol, port.port, port.pid, port.confidence
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Err(e) => format!("NOT VERIFIED: {e}"),
    };
    let mut text = format!("# Project Development Context\n\nProject: {}\nRepository: {}\nLocal path: {}\nStack: {}\n\n## Git\n{}\n\n## Portas declaradas\n{}\n\n## Portas observadas\n{}\nOcupação de porta não confirma saúde nem propriedade do serviço.\n\n## IA\nProvider: {}\nInstruções encontradas: {}\nSkills: {}\nMCP: {}\n\n## Environment health\n.env: {}\n.env.local: {}\nValores não lidos. Validação de chaves: NOT VERIFIED.\n\n## GitHub / issues\nNOT VERIFIED neste snapshot; consulte Git / PRs para consulta explícita.\n\n## Documentação e tarefas\nREADME.md: {}\nTASKS.md: {}\nConteúdo não incorporado automaticamente.\n\n## Development rules\nLeia instruções do repositório antes de agir. Preserve alterações existentes. Não faça merge, push, publicação ou operações destrutivas sem autorização.\n",p.name,p.repository,p.local_path,p.stack.join(", "),git,p.ports.iter().map(|p|format!("- {} :{}",p.name,p.port)).collect::<Vec<_>>().join("\n"),observed,agent.provider,agent.instructions.join(", "),agent.skills.join(", "),agent.mcp_status,path.join(".env").is_file(),path.join(".env.local").is_file(),path.join("README.md").is_file(),path.join("TASKS.md").is_file());
    text.push_str(&runtime_section(runtime));
    Ok(text)
}

/// Resumo do runtime para agentes: só nomes e estados; nunca o texto dos scripts (podem conter segredos).
fn runtime_section(rt: &ProjectRuntime) -> String {
    let list = |items: Vec<String>| {
        if items.is_empty() {
            "nenhum".to_string()
        } else {
            items.join(", ")
        }
    };
    let stack = list(
        rt.detection
            .stack
            .iter()
            .map(|s| s.label.to_string())
            .collect(),
    );
    let manager = rt
        .detection
        .package_manager
        .as_ref()
        .map(|m| m.name.to_string())
        .unwrap_or_else(|| "não identificado".into());
    let git = match &rt.git {
        Some(g) if g.is_repo => format!(
            "{}{} · {} · staged {} / modificados {} / não rastreados {} / conflitos {} · ahead {:?} / behind {:?}",
            g.branch,
            if g.detached { " (HEAD destacado)" } else { "" },
            if g.clean { "limpo" } else { "com alterações" },
            g.staged, g.unstaged, g.untracked, g.conflicts, g.ahead, g.behind
        ),
        Some(_) => "não é um repositório Git".into(),
        None => "NOT VERIFIED".into(),
    };
    let scripts = list(
        rt.detection
            .scripts
            .iter()
            .map(|s| {
                format!(
                    "{} ({})",
                    s.name,
                    match s.kind {
                        ScriptKind::Service => "serviço",
                        ScriptKind::Task => "tarefa",
                        ScriptKind::Other => "outro",
                    }
                )
            })
            .collect(),
    );
    let runs = list(
        rt.runs
            .iter()
            .map(|r| {
                format!(
                    "{} [{:?}{}]",
                    r.command,
                    r.state,
                    r.pid.map(|p| format!(" PID {p}")).unwrap_or_default()
                )
            })
            .collect(),
    );
    let ports = list(
        rt.ports
            .iter()
            .map(|p| {
                format!(
                    "{}/{} {}{}",
                    p.port,
                    p.protocol,
                    p.process,
                    if p.managed {
                        " (gerenciado)"
                    } else {
                        " (externo)"
                    }
                )
            })
            .collect(),
    );
    let composition = if rt.detection.composition.parts.is_empty() {
        "nenhuma".to_string()
    } else {
        rt.detection
            .composition
            .parts
            .iter()
            .map(|p| format!("{} {}", p.role, p.label))
            .collect::<Vec<_>>()
            .join("; ")
    };
    // Versões vêm de `--version`; motivos são mensagens do produto (nunca saída crua de comandos).
    let tools = list(
        rt.tools
            .iter()
            .map(|t| match (&t.version, t.available) {
                (Some(v), true) => format!("{} (disponível, {v})", t.label),
                (None, true) => format!("{} (disponível)", t.label),
                (_, false) => format!(
                    "{} (ausente: {})",
                    t.label,
                    t.reason.clone().unwrap_or_default()
                ),
            })
            .collect(),
    );
    // Só rótulo e id: os argumentos e o texto dos scripts não vão para o contexto.
    let commands = list(
        rt.commands
            .iter()
            .filter(|c| c.available)
            .map(|c| format!("{} [{}]", c.label, c.id))
            .collect(),
    );
    let blocked = list(
        rt.commands
            .iter()
            .filter(|c| !c.available)
            .map(|c| {
                format!(
                    "{} [{}]: {}",
                    c.label,
                    c.id,
                    c.unavailable_reason.clone().unwrap_or_default()
                )
            })
            .collect(),
    );
    let compose = match &rt.compose {
        None => "não aplicável".to_string(),
        Some(c) => format!(
            "{} · containers: {} · serviços: {}{}{}",
            c.file,
            c.containers,
            list(
                c.services
                    .iter()
                    .map(|s| format!("{} ({})", s.name, s.state))
                    .collect()
            ),
            if c.started_here {
                " · subido por esta sessão"
            } else {
                ""
            },
            // O texto do erro do Compose pode citar variáveis do projeto: fica só na tela.
            if c.error.is_some() {
                " · leitura com erro (ver tela de Runtime)"
            } else {
                ""
            },
        ),
    };
    let last_task = match &rt.last_task {
        None => "nenhuma".to_string(),
        Some(t) => format!(
            "{} — {:?}{}",
            t.command,
            t.state,
            t.exit_code
                .map(|c| format!(" (exit {c})"))
                .unwrap_or_default()
        ),
    };
    format!(
        "\n## Runtime (estado desta máquina, observado agora)\nProject ID: {}\nStatus: {:?}{}\nStack: {}\nComposição: {}\nGerenciador de pacotes: {}\nFerramentas: {}\nGit: {}\nScripts: {}\nAções disponíveis: {}\nAções indisponíveis: {}\nCompose: {}\nÚltima tarefa: {}\nExecuções gerenciadas: {}\nPortas com dono verificado: {}\nProcesso externo em execução: {}\n",
        rt.project_id,
        rt.status,
        rt.status_detail.as_ref().map(|d| format!(" — {d}")).unwrap_or_default(),
        stack, composition, manager, tools, git, scripts, commands, blocked, compose, last_task, runs, ports,
        if rt.external_running { "sim" } else { "não" }
    )
}
