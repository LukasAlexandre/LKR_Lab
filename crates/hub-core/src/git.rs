use crate::{commands::run, HubResult};
use serde::Serialize;
use std::path::Path;
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitState {
    pub branch: String,
    pub head: String,
    pub upstream: Option<String>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    pub clean: bool,
    pub commits: Vec<Commit>,
    pub has_origin: bool,
    pub files: Vec<ChangedFile>,
    pub remote: Option<String>,
    pub stashes: usize,
    pub conflicts: u32,
}
#[derive(Debug, Serialize)]
pub struct ChangedFile {
    pub path: String,
    pub status: String,
    pub original: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct Commit {
    pub hash: String,
    pub subject: String,
}
pub fn parse_status(text: &str) -> GitState {
    let mut state = GitState::default();
    let null_delimited = text.contains('\0');
    let mut records = text.split(if null_delimited { '\0' } else { '\n' });
    while let Some(line) = records.next() {
        if let Some(s) = line.strip_prefix("# branch.head ") {
            state.branch = s.into();
        } else if let Some(s) = line.strip_prefix("# branch.oid ") {
            state.head = s.into();
        } else if let Some(s) = line.strip_prefix("# branch.upstream ") {
            state.upstream = Some(s.into());
        } else if let Some(s) = line.strip_prefix("# branch.ab ") {
            let mut p = s.split_whitespace();
            state.ahead = p
                .next()
                .and_then(|s| s.trim_start_matches('+').parse().ok());
            state.behind = p
                .next()
                .and_then(|s| s.trim_start_matches('-').parse().ok());
        } else if let Some(path) = line.strip_prefix("? ") {
            state.untracked += 1;
            state.files.push(ChangedFile {
                path: path.into(),
                status: "??".into(),
                original: None,
            });
        } else if line.starts_with("1 ") || line.starts_with("2 ") || line.starts_with("u ") {
            if line.starts_with("u ") {
                state.conflicts += 1;
            }
            if let Some(xy) = line.split_whitespace().nth(1) {
                let mut c = xy.chars();
                if c.next().is_some_and(|c| c != '.') {
                    state.staged += 1;
                }
                if c.next().is_some_and(|c| c != '.') {
                    state.unstaged += 1;
                }
                let fields = if line.starts_with("1 ") {
                    9
                } else if line.starts_with("2 ") {
                    10
                } else {
                    11
                };
                let path = line
                    .splitn(fields, ' ')
                    .last()
                    .unwrap_or_default()
                    .to_string();
                let original = if line.starts_with("2 ") && null_delimited {
                    records.next().map(str::to_string)
                } else {
                    None
                };
                state.files.push(ChangedFile {
                    path,
                    status: xy.into(),
                    original,
                });
            }
        }
    }
    state.clean = state.staged + state.unstaged + state.untracked == 0;
    state
}
/// Resumo leve do repositório DO PROJETO (uma única chamada `git status`).
/// Somente leitura; não tem relação com o repositório de sync do workspace (bridge).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSummary {
    pub is_repo: bool,
    pub branch: String,
    pub detached: bool,
    pub upstream: Option<String>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    pub conflicts: u32,
    pub changes: u32,
    pub clean: bool,
    pub error: Option<String>,
}
pub fn summary(path: &Path) -> GitSummary {
    match run(
        "git",
        &[
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=normal",
        ],
        Some(path),
    ) {
        Ok(text) => {
            let state = parse_status(&text);
            GitSummary {
                is_repo: true,
                detached: state.branch == "(detached)",
                branch: state.branch,
                upstream: state.upstream,
                ahead: state.ahead,
                behind: state.behind,
                staged: state.staged,
                unstaged: state.unstaged,
                untracked: state.untracked,
                conflicts: state.conflicts,
                changes: state.files.len() as u32,
                clean: state.clean,
                error: None,
            }
        }
        Err(error) => GitSummary {
            is_repo: false,
            error: Some(error),
            ..GitSummary::default()
        },
    }
}
pub fn inspect(path: &Path) -> HubResult<GitState> {
    let mut state = parse_status(&run(
        "git",
        &[
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=normal",
        ],
        Some(path),
    )?);
    state.has_origin = run("git", &["remote"], Some(path))?
        .lines()
        .any(|s| s == "origin");
    if state.has_origin {
        if let Ok(remote) = run("git", &["remote", "get-url", "origin"], Some(path)) {
            let remote = remote
                .strip_prefix("git@github.com:")
                .map(|value| format!("https://github.com/{value}"))
                .unwrap_or(remote);
            if crate::projects::valid_repository(&remote) {
                state.remote = Some(remote);
            }
        }
    }
    state.stashes = run("git", &["stash", "list", "--format=%gd"], Some(path))
        .unwrap_or_default()
        .lines()
        .count();
    if state.head != "(initial)" {
        state.commits = run("git", &["log", "-5", "--format=%h%x09%s"], Some(path))?
            .lines()
            .filter_map(|l| {
                l.split_once('\t').map(|(hash, subject)| Commit {
                    hash: hash.into(),
                    subject: subject.into(),
                })
            })
            .collect();
    }
    Ok(state)
}
/// Um Git worktree REAL, como o `git worktree list --porcelain` o descreve. É leitura do Git: não é
/// a metadata do LKR LAB (`worktrees.rs`) e o path é da máquina (classe C).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub path: String,
    pub head: String,
    /// Vazia quando detached ou bare (nunca inventada).
    pub branch: String,
    /// O checkout principal: o Git sempre o lista PRIMEIRO (documentado); o campo evita depender
    /// da posição no frontend.
    pub is_primary: bool,
    pub detached: bool,
    pub bare: bool,
    pub locked: bool,
    pub locked_reason: Option<String>,
    pub prunable: bool,
    pub prunable_reason: Option<String>,
}

/// Interpreta a saída de `git worktree list --porcelain`: registros separados por linha em branco;
/// só o que o Git informa (`worktree`, `HEAD`, `branch`, `detached`, `bare`, `locked [motivo]`,
/// `prunable [motivo]`).
pub fn parse_worktrees(text: &str) -> Vec<Worktree> {
    let text = text.replace("\r\n", "\n");
    let mut out: Vec<Worktree> = Vec::new();
    for block in text.split("\n\n") {
        let mut w = Worktree {
            path: String::new(),
            head: String::new(),
            branch: String::new(),
            is_primary: false,
            detached: false,
            bare: false,
            locked: false,
            locked_reason: None,
            prunable: false,
            prunable_reason: None,
        };
        let reason = |rest: &str| {
            let r = rest.trim();
            if r.is_empty() {
                None
            } else {
                Some(r.to_string())
            }
        };
        for l in block.lines() {
            if let Some(v) = l.strip_prefix("worktree ") {
                w.path = v.into();
            } else if let Some(v) = l.strip_prefix("HEAD ") {
                w.head = v.into();
            } else if let Some(v) = l.strip_prefix("branch ") {
                w.branch = v.trim_start_matches("refs/heads/").into();
            } else if l == "detached" {
                w.detached = true;
            } else if l == "bare" {
                w.bare = true;
            } else if l == "locked" || l.starts_with("locked ") {
                w.locked = true;
                w.locked_reason = reason(l.trim_start_matches("locked"));
            } else if l == "prunable" || l.starts_with("prunable ") {
                w.prunable = true;
                w.prunable_reason = reason(l.trim_start_matches("prunable"));
            }
        }
        if !w.path.is_empty() {
            out.push(w);
        }
    }
    if let Some(first) = out.first_mut() {
        first.is_primary = true;
    }
    out
}

pub fn worktrees(path: &Path) -> HubResult<Vec<Worktree>> {
    // Somente leitura: nunca prune, repair, fetch ou checkout.
    let output = run("git", &["worktree", "list", "--porcelain"], Some(path))?;
    Ok(parse_worktrees(&output))
}

/// Resolve only a worktree registered by Git for this project.
pub fn worktree_path(repo: &Path, target: &str) -> HubResult<std::path::PathBuf> {
    let canonical = crate::projects::canonical(target)?;
    let known = worktrees(repo)?.into_iter().any(|tree| {
        Path::new(&tree.path)
            .canonicalize()
            .is_ok_and(|path| path == canonical)
    });
    if !known {
        return Err("Worktree não pertence ao repositório selecionado.".into());
    }
    Ok(canonical)
}

/// Como o Git deve criar o worktree (mutação explícita; nunca fetch, pull, push ou merge).
#[derive(Debug, Clone, Copy)]
pub enum NewWorktree<'a> {
    /// Branch nova a partir de uma base EXPLÍCITA (qualquer commit-ish válido).
    NewBranch { branch: &'a str, base: &'a str },
    /// Branch que já existe localmente (o Git recusa se já estiver em outro worktree).
    ExistingBranch { branch: &'a str },
}

fn valid_branch(repo: &Path, branch: &str) -> HubResult<()> {
    if branch.trim() != branch || branch.is_empty() || branch.starts_with('-') {
        return Err("Informe um nome de branch válido.".into());
    }
    run("git", &["check-ref-format", "--branch", branch], Some(repo))?;
    Ok(())
}

/// Destino novo, absoluto, fora do repositório e sem `..`. Devolve (repo canônico, destino).
fn validated_destination(
    repo: &Path,
    target: &str,
) -> HubResult<(std::path::PathBuf, std::path::PathBuf)> {
    let target = Path::new(target);
    if !target.is_absolute()
        || target.exists()
        || target
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("Escolha um caminho absoluto novo, sem componentes '..'.".into());
    }
    let parent = target
        .parent()
        .ok_or("Pasta de destino inválida")?
        .canonicalize()
        .map_err(|_| "A pasta pai precisa existir.")?;
    let name = target.file_name().ok_or("Nome de pasta inválido")?;
    let destination = parent.join(name);
    let repo = repo
        .canonicalize()
        .map_err(|_| "Repositório indisponível")?;
    if destination.starts_with(&repo) {
        return Err("Crie a worktree fora da pasta do repositório atual.".into());
    }
    Ok((repo, destination))
}

pub fn add_worktree(repo: &Path, target: &str, spec: NewWorktree<'_>) -> HubResult<()> {
    match spec {
        NewWorktree::NewBranch { branch, base } => {
            valid_branch(repo, branch)?;
            let base = base.trim();
            if base.is_empty() || base.starts_with('-') {
                return Err("Informe a base (branch, tag ou commit) da nova branch.".into());
            }
            run(
                "git",
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("{base}^{{commit}}"),
                ],
                Some(repo),
            )
            .map_err(|_| format!("A base “{base}” não existe neste repositório."))?;
            let (repo, destination) = validated_destination(repo, target)?;
            run(
                "git",
                &[
                    "worktree",
                    "add",
                    "-b",
                    branch,
                    "--",
                    &cli_path(&destination),
                    base,
                ],
                Some(&repo),
            )?;
        }
        NewWorktree::ExistingBranch { branch } => {
            valid_branch(repo, branch)?;
            run(
                "git",
                &[
                    "show-ref",
                    "--verify",
                    "--quiet",
                    &format!("refs/heads/{branch}"),
                ],
                Some(repo),
            )
            .map_err(|_| format!("A branch “{branch}” não existe neste repositório."))?;
            let (repo, destination) = validated_destination(repo, target)?;
            run(
                "git",
                &["worktree", "add", "--", &cli_path(&destination), branch],
                Some(&repo),
            )?;
        }
    }
    Ok(())
}

/// Compatível com o fluxo anterior: branch nova a partir do HEAD atual.
pub fn create_worktree(repo: &Path, target: &str, branch: &str) -> HubResult<()> {
    add_worktree(
        repo,
        target,
        NewWorktree::NewBranch {
            branch,
            base: "HEAD",
        },
    )
}

pub fn remove_worktree(repo: &Path, target: &str, confirmed: bool) -> HubResult<()> {
    if !confirmed {
        return Err("Confirme explicitamente a remoção da worktree.".into());
    }
    let path = worktree_path(repo, target)?;
    let trees = worktrees(repo)?;
    let is_main = trees.first().is_some_and(|tree| {
        Path::new(&tree.path)
            .canonicalize()
            .is_ok_and(|first| first == path)
    });
    let is_locked = trees.iter().any(|tree| {
        tree.locked
            && Path::new(&tree.path)
                .canonicalize()
                .is_ok_and(|item| item == path)
    });
    if is_main || is_locked || repo.canonicalize().is_ok_and(|current| current == path) {
        return Err("Worktree principal, ativa ou bloqueada não pode ser removida.".into());
    }
    let status = run(
        "git",
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--ignored",
        ],
        Some(&path),
    )?;
    if !status.is_empty() {
        return Err("Worktree contém alterações, arquivos não rastreados ou ignorados. Revise-os antes de remover.".into());
    }
    // Git performs its own second cleanliness check; never use --force or remove_dir_all.
    run(
        "git",
        &["worktree", "remove", "--", &cli_path(&path)],
        Some(repo),
    )?;
    Ok(())
}

fn cli_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        format!("//{}", unc.replace('\\', "/"))
    } else {
        text.strip_prefix(r"\\?\")
            .unwrap_or(&text)
            .replace('\\', "/")
    }
}
