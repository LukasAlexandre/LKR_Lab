//! Rust do projeto, lido dos manifestos (Cargo.toml) SEM executar o cargo.
//!
//! `cargo metadata` seria mais completo, mas executa o `rustc` através de qualquer
//! `build.rustc-wrapper` do `.cargo/config.toml` do projeto — e a detecção roda sozinha ao
//! selecionar um projeto. Aqui só se lê arquivo pequeno, dentro da pasta, sem rede nem processo.
//!
//! Cobre o necessário para decidir "o que o `cargo run` rodaria": pacote raiz, `[workspace]`
//! (membros literais e `dir/*`), `[[bin]]`, `src/main.rs`, `src/bin/*` e `default-run`.
use serde::Serialize;
use std::path::{Path, PathBuf};

const MAX_MANIFEST: u64 = 1_000_000;
const MAX_PACKAGES: usize = 64;
const MAX_BINS: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RustBin {
    pub package: String,
    pub name: String,
    /// Pasta do pacote, relativa à raiz do Cargo ("." para o pacote raiz).
    pub package_dir: String,
    /// O pacote é o app do Tauri: quem roda é o `tauri dev`, não o `cargo run`.
    pub is_tauri: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RustInfo {
    /// Pasta do Cargo.toml raiz, relativa à raiz do projeto ("." ou "src-tauri").
    pub dir: String,
    pub workspace: bool,
    /// Manifesto raiz sem `[package]` (só `[workspace]`).
    pub virtual_manifest: bool,
    pub packages: Vec<String>,
    pub bins: Vec<RustBin>,
    /// `default-run` do pacote raiz (resolve a ambiguidade de vários binários).
    pub default_run: Option<String>,
    /// Observações para a interface (manifesto ilegível, padrão não suportado…).
    pub notes: Vec<String>,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

fn read_manifest(path: &Path) -> Result<toml::Table, String> {
    let meta = std::fs::metadata(path).map_err(|_| "Cargo.toml inacessível".to_string())?;
    if !meta.is_file() || meta.len() > MAX_MANIFEST {
        return Err("Cargo.toml grande demais ou inválido".into());
    }
    let text = std::fs::read_to_string(path).map_err(|_| "Cargo.toml ilegível".to_string())?;
    text.parse::<toml::Table>()
        .map_err(|_| "Cargo.toml com sintaxe inválida".to_string())
}

fn inside(base: &Path, path: &Path) -> bool {
    match (base.canonicalize(), path.canonicalize()) {
        (Ok(base), Ok(path)) => path.starts_with(base),
        _ => false,
    }
}

fn rel(root: &Path, dir: &Path) -> String {
    match dir.strip_prefix(root) {
        Ok(p) if p.as_os_str().is_empty() => ".".into(),
        Ok(p) => p.to_string_lossy().replace('\\', "/"),
        Err(_) => ".".into(),
    }
}

/// Membros do workspace: caminho literal ou `dir/*` (um nível). Nada fora da pasta do workspace.
fn expand_members(
    base: &Path,
    patterns: &[String],
    excluded: &[String],
    notes: &mut Vec<String>,
) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for pattern in patterns {
        let normalized = pattern.replace('\\', "/");
        let unsafe_path =
            normalized.starts_with('/') || normalized.contains("..") || normalized.contains(':');
        if unsafe_path {
            notes.push(format!(
                "Membro do workspace ignorado (caminho fora da pasta): {pattern}"
            ));
            continue;
        }
        if let Some(parent) = normalized.strip_suffix("/*") {
            if parent.contains(['*', '?', '[']) {
                notes.push(format!("Padrão de membros não suportado: {pattern}"));
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(base.join(parent)) {
                let mut dirs: Vec<PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.join("Cargo.toml").is_file())
                    .collect();
                dirs.sort();
                found.extend(dirs);
            }
        } else if normalized.contains(['*', '?', '[']) {
            notes.push(format!("Padrão de membros não suportado: {pattern}"));
        } else {
            found.push(base.join(&normalized));
        }
    }
    found.retain(|dir| {
        let name = rel(base, dir);
        !excluded
            .iter()
            .any(|e| e.replace('\\', "/").trim_end_matches('/') == name)
            && dir.join("Cargo.toml").is_file()
            && inside(base, dir)
    });
    found.dedup();
    found
}

fn strings(value: Option<&toml::Value>) -> Vec<String> {
    value
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

struct Member {
    name: String,
    dir: PathBuf,
    bins: Vec<String>,
    default_run: Option<String>,
}

fn member(dir: &Path, manifest: &toml::Table) -> Option<Member> {
    let package = manifest.get("package")?.as_table()?;
    let name = package.get("name")?.as_str()?.to_string();
    if !valid_name(&name) {
        return None;
    }
    let mut bins: Vec<String> = Vec::new();
    let mut add = |bin: String| {
        if valid_name(&bin) && !bins.contains(&bin) && bins.len() < MAX_BINS {
            bins.push(bin);
        }
    };
    if let Some(list) = manifest.get("bin").and_then(|b| b.as_array()) {
        for bin in list.iter().filter_map(|b| b.as_table()) {
            if let Some(n) = bin.get("name").and_then(|n| n.as_str()) {
                add(n.to_string());
            }
        }
    }
    let autobins = package
        .get("autobins")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    if autobins {
        if dir.join("src").join("main.rs").is_file() {
            add(name.clone());
        }
        if let Ok(entries) = std::fs::read_dir(dir.join("src").join("bin")) {
            let mut found: Vec<String> = entries
                .flatten()
                .filter_map(|e| {
                    let path = e.path();
                    if path.is_file() && path.extension().is_some_and(|x| x == "rs") {
                        path.file_stem().map(|s| s.to_string_lossy().to_string())
                    } else if path.join("main.rs").is_file() {
                        path.file_name().map(|s| s.to_string_lossy().to_string())
                    } else {
                        None
                    }
                })
                .collect();
            found.sort();
            for bin in found {
                add(bin);
            }
        }
    }
    let default_run = package
        .get("default-run")
        .and_then(|v| v.as_str())
        .filter(|v| valid_name(v))
        .map(String::from);
    Some(Member {
        name,
        dir: dir.to_path_buf(),
        bins,
        default_run,
    })
}

/// Detecta o projeto Rust na raiz ou em `src-tauri/`. `None` quando não há Cargo.toml lá.
pub fn detect(root: &Path) -> Option<RustInfo> {
    let (cargo_dir, rel_dir) = if root.join("Cargo.toml").is_file() {
        (root.to_path_buf(), ".")
    } else if root.join("src-tauri").join("Cargo.toml").is_file() {
        (root.join("src-tauri"), "src-tauri")
    } else {
        return None;
    };
    let mut info = RustInfo {
        dir: rel_dir.into(),
        ..RustInfo::default()
    };
    let manifest = match read_manifest(&cargo_dir.join("Cargo.toml")) {
        Ok(manifest) => manifest,
        Err(note) => {
            info.notes.push(note);
            return Some(info);
        }
    };
    let workspace = manifest.get("workspace").and_then(|w| w.as_table());
    info.workspace = workspace.is_some();
    info.virtual_manifest = !manifest.contains_key("package");
    let mut members: Vec<Member> = Vec::new();
    if let Some(root_member) = member(&cargo_dir, &manifest) {
        info.default_run = root_member.default_run.clone();
        members.push(root_member);
    }
    if let Some(workspace) = workspace {
        let patterns = strings(workspace.get("members"));
        let excluded = strings(workspace.get("exclude"));
        for dir in expand_members(&cargo_dir, &patterns, &excluded, &mut info.notes) {
            if members.len() >= MAX_PACKAGES {
                info.notes.push(format!(
                    "Mais de {MAX_PACKAGES} pacotes: o restante foi ignorado."
                ));
                break;
            }
            match read_manifest(&dir.join("Cargo.toml")) {
                Ok(manifest) => {
                    if let Some(found) = member(&dir, &manifest) {
                        if !members.iter().any(|m| m.name == found.name) {
                            members.push(found);
                        }
                    }
                }
                Err(note) => info
                    .notes
                    .push(format!("{}: {note}", rel(&cargo_dir, &dir))),
            }
        }
    }
    for found in &members {
        info.packages.push(found.name.clone());
        let is_tauri = found.dir.join("tauri.conf.json").is_file()
            || (found.dir == cargo_dir && rel_dir == "src-tauri");
        for bin in &found.bins {
            info.bins.push(RustBin {
                package: found.name.clone(),
                name: bin.clone(),
                package_dir: rel(&cargo_dir, &found.dir),
                is_tauri,
            });
        }
    }
    Some(info)
}
