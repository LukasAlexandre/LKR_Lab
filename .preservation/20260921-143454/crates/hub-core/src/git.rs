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
#[derive(Serialize)]
pub struct Worktree {
    pub path: String,
    pub head: String,
    pub branch: String,
    pub locked: bool,
}
pub fn worktrees(path: &Path) -> HubResult<Vec<Worktree>> {
    let output = run("git", &["worktree", "list", "--porcelain"], Some(path))?;
    Ok(output
        .split("\n\n")
        .filter_map(|block| {
            let mut w = Worktree {
                path: String::new(),
                head: String::new(),
                branch: String::new(),
                locked: false,
            };
            for l in block.lines() {
                if let Some(v) = l.strip_prefix("worktree ") {
                    w.path = v.into();
                }
                if let Some(v) = l.strip_prefix("HEAD ") {
                    w.head = v.into();
                }
                if let Some(v) = l.strip_prefix("branch ") {
                    w.branch = v.trim_start_matches("refs/heads/").into();
                }
                if l.starts_with("locked") {
                    w.locked = true;
                }
            }
            if w.path.is_empty() {
                None
            } else {
                Some(w)
            }
        })
        .collect())
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

pub fn create_worktree(repo: &Path, target: &str, branch: &str) -> HubResult<()> {
    if branch.trim() != branch || branch.is_empty() || branch.starts_with('-') {
        return Err("Informe um nome de branch válido.".into());
    }
    run("git", &["check-ref-format", "--branch", branch], Some(repo))?;
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
    run(
        "git",
        &[
            "worktree",
            "add",
            "-b",
            branch,
            "--",
            &cli_path(&destination),
        ],
        Some(&repo),
    )?;
    Ok(())
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
