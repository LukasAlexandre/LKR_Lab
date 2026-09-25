use crate::{
    agents::{AgentProvider, ClaudeProvider},
    git,
    models::Project,
    ports,
};
use std::path::Path;
pub fn generate(p: &Project) -> String {
    let path = Path::new(&p.local_path);
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
    format!("# Project Development Context\n\nProject: {}\nRepository: {}\nLocal path: {}\nStack: {}\n\n## Git\n{}\n\n## Portas declaradas\n{}\n\n## Portas observadas\n{}\nOcupação de porta não confirma saúde nem propriedade do serviço.\n\n## IA\nProvider: {}\nInstruções encontradas: {}\nSkills: {}\nMCP: {}\n\n## Environment health\n.env: {}\n.env.local: {}\nValores não lidos. Validação de chaves: NOT VERIFIED.\n\n## GitHub / issues\nNOT VERIFIED neste snapshot; consulte Git / PRs para consulta explícita.\n\n## Documentação e tarefas\nREADME.md: {}\nTASKS.md: {}\nConteúdo não incorporado automaticamente.\n\n## Development rules\nLeia instruções do repositório antes de agir. Preserve alterações existentes. Não faça merge, push, publicação ou operações destrutivas sem autorização.\n",p.name,p.repository,p.local_path,p.stack.join(", "),git,p.ports.iter().map(|p|format!("- {} :{}",p.name,p.port)).collect::<Vec<_>>().join("\n"),observed,agent.provider,agent.instructions.join(", "),agent.skills.join(", "),agent.mcp_status,path.join(".env").is_file(),path.join(".env.local").is_file(),path.join("README.md").is_file(),path.join("TASKS.md").is_file())
}
