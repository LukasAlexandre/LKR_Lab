/*
 * LKR LAB — backup versionado no Git
 *
 * Grava o estado de um módulo em um arquivo JSON do repositório e publica
 * SOMENTE esse arquivo: `git add -- <arquivo>` + `git commit --only -- <arquivo>`
 * + `git push origin <branch>`. Nunca usa force, reset, clean, stash ou restore.
 *
 * Todo comando Git é executado com execFile (sem shell) e argumentos fixos
 * definidos aqui. Repositório, arquivos e branch vêm da aplicação/do próprio
 * Git, nunca da requisição. A autenticação é a que o Git já usa na máquina.
 */
import { execFile } from "node:child_process";
import { promises as fs } from "node:fs";
import path from "node:path";

await import("../core/lkr-portable.js");
await import("../core/lkr-workspace.js");
await import("../lab-setup/catalog.js");
await import("../lab-setup/store.js");
const labSetup = globalThis.LKR.labSetup;
const workspace = globalThis.LKR.workspace;

/**
 * Módulos com backup no Git. Cada novo módulo do LKR LAB (inventário,
 * projetos…) entra aqui com seu arquivo e sua validação.
 */
export const MODULES = {
  "lab-setup": {
    label: "Lab Setup",
    file: "data/lab-setup.json",
    schemaVersion: labSetup.SCHEMA_VERSION,
    source: labSetup.REPO_SOURCE,
    // Valida e normaliza; devolve só o estado persistente (sem filtros de UI).
    prepare(state) {
      const result = labSetup.validateImport(state, labSetup.catalog, new Date().toISOString());
      if (!result.ok) return result;
      const persistable = labSetup.persistableState(result.state);
      return { ok: true, state: persistable, hash: labSetup.stateHash(persistable), summary: result.summary };
    },
  },
  // Workspace do app desktop: projetos (sem caminho local), prompts, knowledge e preferências portáveis.
  workspace: {
    label: "Workspace",
    file: "data/workspace.json",
    schemaVersion: workspace.SCHEMA_VERSION,
    source: workspace.SOURCE,
    prepare: (state) => workspace.validate(state),
  },
};

const MODULE_FILES = new Set(Object.values(MODULES).map((m) => m.file));

const MAX_OUTPUT = 4 * 1024 * 1024;
const TIMEOUT_LOCAL_MS = 20_000;
const TIMEOUT_REMOTE_MS = 60_000;

// ------------------------------------------------------------------ erros

export const MESSAGES = {
  NO_REPO: "O LKR LAB não está dentro de um repositório Git.",
  NO_REMOTE: "O repositório não tem o remote “origin” configurado.",
  NO_REMOTE_BRANCH: "A branch ainda não existe no GitHub. Faça o primeiro push manualmente no terminal.",
  DETACHED: "O repositório está sem branch (detached HEAD). Faça checkout de uma branch.",
  IN_PROGRESS: "Há um merge, rebase ou cherry-pick em andamento. Conclua-o no terminal antes de sincronizar.",
  REMOTE_AHEAD: "O repositório remoto possui alterações mais recentes. Use “Verificar atualização” antes de sincronizar.",
  DIVERGED: "A branch local e a do GitHub divergiram. Resolva no terminal (sem force) antes de continuar.",
  UNRELATED_UNPUSHED:
    "Há commits locais ainda não enviados com outras alterações do projeto. O LKR LAB só publica o arquivo de dados: envie esses commits manualmente.",
  LOCAL_CHANGES: "Alterações locais impedem a atualização sem risco. Nada foi alterado.",
  NETWORK: "Não foi possível conectar ao GitHub. Verifique a internet e tente de novo.",
  AUTH: "O Git não conseguiu autenticar no GitHub. Rode “git push” no terminal para renovar a credencial.",
  IDENTITY: "O Git não sabe quem é o autor do commit. Configure user.name e user.email.",
  INVALID_PAYLOAD: "Os dados enviados não são um estado válido deste módulo.",
  INVALID_BACKUP: "O arquivo de backup do repositório está inválido.",
  NO_BACKUP: "Ainda não existe backup deste módulo no repositório.",
  BUSY: "Outra operação Git do LKR LAB está em andamento.",
  GIT_FAILED: "O Git retornou um erro inesperado.",
};

export class BackupError extends Error {
  constructor(code, { status = 409, detail = "", message } = {}) {
    super(message || MESSAGES[code] || MESSAGES.GIT_FAILED);
    this.code = code;
    this.status = status;
    this.detail = redact(detail).slice(0, 600);
  }
}

/** Remove credenciais embutidas em URLs (https://usuario:token@host). */
export function redact(text) {
  return String(text || "")
    .replace(/(\b[a-z][a-z0-9+.-]*:\/\/)[^\s/@'"]+@/gi, "$1***@")
    .replace(/\b(gh[pousr]_[A-Za-z0-9]{8,}|github_pat_[A-Za-z0-9_]{8,})\b/g, "***")
    .trim();
}

/** Traduz a saída de erro do Git em um código conhecido. */
export function classifyGitError(stderr, timedOut) {
  const s = String(stderr || "").toLowerCase();
  if (timedOut) return "NETWORK";
  if (/not a git repository/.test(s)) return "NO_REPO";
  if (/no such remote|does not appear to be a git repository|'origin' does not appear/.test(s)) return "NO_REMOTE";
  if (
    /authentication failed|could not read (username|password)|terminal prompts disabled|invalid username or password|permission denied|returned error: 40[13]|access denied|repository not found|logon failed|denied to /.test(
      s,
    )
  )
    return "AUTH";
  if (/\[rejected\]|non-fast-forward|fetch first|updates were rejected|failed to push some refs/.test(s)) return "REMOTE_AHEAD";
  if (/please tell me who you are|unable to auto-detect email|empty ident/.test(s)) return "IDENTITY";
  if (/would be overwritten|commit your changes or stash|untracked working tree files|not possible to fast-forward/.test(s)) return "LOCAL_CHANGES";
  if (
    /could not resolve|unable to access|failed to connect|couldn't connect|connection (refused|reset|timed out)|network is unreachable|timed out|could not connect|ssl|proxy|unable to connect|no route to host|host is down/.test(
      s,
    )
  )
    return "NETWORK";
  return "GIT_FAILED";
}

/** "https://x:tok@github.com/Owner/Repo.git" ou "git@github.com:Owner/Repo.git" → "Owner/Repo". */
export function describeRemote(url) {
  const value = String(url || "").trim();
  // Caminho no disco (C:\…, /…, ./…): não expõe o caminho, só indica que é local.
  if (/^([a-z]:[\\/]|[\\/.]|file:)/i.test(value)) return "remote local";
  const scp = /^[\w.-]+@([\w.-]+):(.+?)(?:\.git)?\/?$/.exec(value);
  if (scp) return scp[1].toLowerCase() === "github.com" ? scp[2] : scp[1] + "/" + scp[2];
  try {
    const parsed = new URL(value);
    const repoPath = parsed.pathname.replace(/^\/+/, "").replace(/\.git\/?$/, "");
    if (parsed.protocol === "file:") return "repositório local";
    return parsed.hostname.toLowerCase() === "github.com" ? repoPath : parsed.host + "/" + repoPath;
  } catch {
    return "remote local";
  }
}

// ------------------------------------------------------------- utilidades

function isoWithOffset(date) {
  const pad = (n) => String(Math.abs(n)).padStart(2, "0");
  const offset = -date.getTimezoneOffset();
  const sign = offset >= 0 ? "+" : "-";
  return (
    date.getFullYear() + "-" + pad(date.getMonth() + 1) + "-" + pad(date.getDate()) +
    "T" + pad(date.getHours()) + ":" + pad(date.getMinutes()) + ":" + pad(date.getSeconds()) +
    sign + pad(Math.trunc(offset / 60)) + ":" + pad(offset % 60)
  );
}

function commitStamp(date) {
  const pad = (n) => String(n).padStart(2, "0");
  return date.getFullYear() + "-" + pad(date.getMonth() + 1) + "-" + pad(date.getDate()) + " " + pad(date.getHours()) + ":" + pad(date.getMinutes());
}

const samePath = (a, b) => {
  const norm = (p) => path.resolve(p).replace(/[\\/]+$/, "");
  return process.platform === "win32" ? norm(a).toLowerCase() === norm(b).toLowerCase() : norm(a) === norm(b);
};

const lines = (text) => String(text || "").split(/\r?\n/).map((l) => l.trim()).filter(Boolean);

// ----------------------------------------------------------------- backup

/**
 * @param {object} options
 * @param {string} options.repoRoot  raiz do repositório (definida pela aplicação)
 * @param {object} [options.env]     variáveis extras para o Git (usado nos testes)
 * @param {() => Date} [options.now]
 */
export function createGitBackup({ repoRoot, env = {}, now = () => new Date() }) {
  let busy = false;

  function git(args, timeout = TIMEOUT_LOCAL_MS) {
    return new Promise((resolve) => {
      execFile(
        "git",
        args,
        {
          cwd: repoRoot,
          timeout,
          windowsHide: true,
          maxBuffer: MAX_OUTPUT,
          env: {
            ...process.env,
            // Sem prompts interativos: se a credencial não estiver disponível, falha e avisa.
            GIT_TERMINAL_PROMPT: "0",
            GCM_INTERACTIVE: "never",
            LC_ALL: "C",
            ...env,
          },
        },
        (error, stdout, stderr) =>
          resolve({ ok: !error, stdout: String(stdout || ""), stderr: String(stderr || ""), timedOut: Boolean(error && error.killed) }),
      );
    });
  }

  async function gitOrThrow(args, timeout) {
    const result = await git(args, timeout);
    if (!result.ok) {
      const code = classifyGitError(result.stderr, result.timedOut);
      throw new BackupError(code, { detail: result.stderr });
    }
    return result.stdout;
  }

  /** Garante que só uma operação que escreve no repositório rode por vez. */
  async function exclusive(task) {
    if (busy) throw new BackupError("BUSY");
    busy = true;
    try {
      return await task();
    } finally {
      busy = false;
    }
  }

  function moduleOf(id) {
    // hasOwn: "constructor", "__proto__" etc. não são módulos.
    if (typeof id !== "string" || !Object.hasOwn(MODULES, id)) throw new BackupError("INVALID_PAYLOAD", { status: 400, message: "Módulo desconhecido." });
    return MODULES[id];
  }

  /** Confirma repositório, branch e remote. Lança erro quando falta algo obrigatório. */
  async function inspect({ requireRemote = true } = {}) {
    const top = await git(["rev-parse", "--show-toplevel"]);
    if (!top.ok || !samePath(top.stdout.trim(), repoRoot)) throw new BackupError("NO_REPO", { status: 503 });

    const branchResult = await git(["symbolic-ref", "--quiet", "--short", "HEAD"]);
    const branch = branchResult.ok ? branchResult.stdout.trim() : null;
    // Branch vem do próprio Git, mas ainda assim é validada antes de virar argumento.
    if (!branch || branch.startsWith("-") || !/^[\w./-]+$/.test(branch)) throw new BackupError("DETACHED");

    const remote = await git(["remote", "get-url", "origin"]);
    if (requireRemote && !remote.ok) throw new BackupError("NO_REMOTE");

    const markers = ["MERGE_HEAD", "rebase-merge", "rebase-apply", "CHERRY_PICK_HEAD", "REVERT_HEAD"].flatMap((m) => ["--git-path", m]);
    for (const markerPath of lines((await git(["rev-parse", ...markers])).stdout)) {
      if (await exists(path.resolve(repoRoot, markerPath))) throw new BackupError("IN_PROGRESS");
    }

    const head = await git(["rev-parse", "--short", "HEAD"]);
    return { branch, remote: remote.ok, remoteLabel: remote.ok ? describeRemote(remote.stdout.trim()) : null, head: head.ok ? head.stdout.trim() : null };
  }

  async function exists(target) {
    try {
      await fs.access(target);
      return true;
    } catch {
      return false;
    }
  }

  async function fetchRemote(branch) {
    // Atualiza apenas o ref de acompanhamento origin/<branch>; não toca na branch local nem nos arquivos.
    await gitOrThrow(["fetch", "--quiet", "--no-tags", "origin", "+refs/heads/" + branch + ":refs/remotes/origin/" + branch], TIMEOUT_REMOTE_MS).catch(
      (error) => {
        // Branch inexistente no remote não é falha de rede.
        if (/couldn't find remote ref/i.test(error.detail)) return;
        throw error;
      },
    );
  }

  /** ahead/behind em relação a origin/<branch>; null se a branch não existe no remote. */
  async function divergence(branch) {
    const ref = "refs/remotes/origin/" + branch;
    if (!(await git(["rev-parse", "--verify", "--quiet", ref])).ok) return null;
    const out = await gitOrThrow(["rev-list", "--left-right", "--count", "HEAD..." + ref]);
    const [ahead, behind] = out.trim().split(/\s+/).map(Number);
    return { ahead, behind };
  }

  async function readFileState(mod) {
    const file = path.join(repoRoot, mod.file);
    let text;
    try {
      text = await fs.readFile(file, "utf8");
    } catch {
      return null;
    }
    try {
      const envelope = JSON.parse(text);
      const prepared = mod.prepare(envelope);
      return prepared.ok ? { envelope, hash: prepared.hash } : { envelope, invalid: prepared.error };
    } catch {
      return { invalid: "JSON ilegível." };
    }
  }

  async function lastModuleCommit(mod) {
    const out = (await git(["log", "-1", "--format=%h%x09%cI", "--", mod.file])).stdout.trim();
    if (!out) return null;
    const [hash, date] = out.split("\t");
    return { hash, date };
  }

  async function fileStatus(mod) {
    return (await gitOrThrow(["status", "--porcelain=v1", "--untracked-files=all", "--", mod.file])).trim();
  }

  /** Arquivos alterados por commits locais que ainda não estão em nenhum ref do origin. */
  async function unpushedFiles() {
    const out = await gitOrThrow(["log", "--format=", "--name-only", "HEAD", "--not", "--remotes=origin"]);
    return new Set(lines(out).map((l) => l.replace(/\\/g, "/")));
  }

  async function writeAtomic(mod, content) {
    const target = path.join(repoRoot, mod.file);
    await fs.mkdir(path.dirname(target), { recursive: true });
    const temp = target + ".tmp-" + process.pid + "-" + Date.now();
    await fs.writeFile(temp, content, "utf8");
    try {
      await fs.rename(temp, target);
    } catch (error) {
      await fs.rm(temp, { force: true });
      throw error;
    }
  }

  // ------------------------------------------------------------ operações

  /** GET /api/git/status */
  async function status({ fetch = false, module: moduleId = "lab-setup" } = {}) {
    const mod = moduleOf(moduleId);
    let info;
    try {
      info = await inspect({ requireRemote: false });
    } catch (error) {
      if (error instanceof BackupError) return { repository: error.code !== "NO_REPO", error: { code: error.code, message: error.message } };
      throw error;
    }
    let fetchError = null;
    if (fetch && info.remote) {
      try {
        await fetchRemote(info.branch);
      } catch (error) {
        fetchError = { code: error.code, message: error.message };
      }
    }
    const porcelain = lines((await git(["status", "--porcelain=v1", "--untracked-files=all"])).stdout);
    const labLines = porcelain.filter((l) => l.slice(3).replace(/\\/g, "/").replace(/^"|"$/g, "") === mod.file);
    const counts = info.remote ? await divergence(info.branch) : null;
    const file = await readFileState(mod);
    return {
      repository: true,
      repo: info.remoteLabel,
      branch: info.branch,
      remote: info.remote,
      remoteBranch: Boolean(counts),
      dirty: porcelain.length > labLines.length,
      labStateModified: labLines.length > 0,
      lastCommit: info.head,
      ahead: counts ? counts.ahead : null,
      behind: counts ? counts.behind : null,
      fetched: fetch && info.remote && !fetchError,
      fetchError,
      backup: file
        ? { file: mod.file, exists: true, valid: !file.invalid, updatedAt: file.envelope && file.envelope.updatedAt, stateHash: file.hash || null }
        : { file: mod.file, exists: false },
      lastBackupCommit: await lastModuleCommit(mod),
    };
  }

  /** GET /api/lab-state — backup atual do arquivo versionado. */
  async function readState(moduleId = "lab-setup") {
    const mod = moduleOf(moduleId);
    const info = await inspect({ requireRemote: false });
    const file = await readFileState(mod);
    if (!file) throw new BackupError("NO_BACKUP", { status: 404 });
    if (file.invalid) throw new BackupError("INVALID_BACKUP", { status: 422, detail: file.invalid });
    const clean = !(await fileStatus(mod));
    const counts = info.remote ? await divergence(info.branch) : null;
    return {
      backup: file.envelope,
      stateHash: file.hash,
      // "synced": o arquivo lido é exatamente o que está publicado no GitHub.
      synced: clean && Boolean(counts) && counts.ahead === 0 && !(await unpushedFiles()).has(mod.file),
      commit: await lastModuleCommit(mod),
      branch: info.branch,
    };
  }

  /**
   * GET /api/remote/:module — o que o GitHub tem, SEM tocar na árvore de trabalho.
   * Atualiza só origin/<branch> (fetch) e lê o arquivo do módulo direto desse ref.
   * Falha de rede não é erro: devolve fetched=false e o que já se sabia do remote.
   */
  async function remoteState(moduleId = "lab-setup", { fetch = true } = {}) {
    const mod = moduleOf(moduleId);
    const info = await inspect({ requireRemote: false });
    let fetchError = null;
    if (fetch && info.remote) {
      try {
        await fetchRemote(info.branch);
      } catch (error) {
        if (!(error instanceof BackupError)) throw error;
        fetchError = { code: error.code, message: error.message };
      }
    }
    const counts = info.remote ? await divergence(info.branch) : null;
    const result = {
      branch: info.branch,
      repo: info.remoteLabel,
      remote: info.remote,
      remoteBranch: Boolean(counts),
      fetched: fetch && info.remote && !fetchError,
      fetchError,
      ahead: counts ? counts.ahead : null,
      behind: counts ? counts.behind : null,
      file: { status: "missing" },
    };
    if (!counts) return result;
    // mod.file é constante deste arquivo; o ref é montado a partir de um nome validado em inspect().
    const shown = await git(["show", "refs/remotes/origin/" + info.branch + ":" + mod.file]);
    if (!shown.ok) return result; // arquivo ainda não existe no remote
    let envelope;
    try {
      envelope = JSON.parse(shown.stdout);
    } catch {
      return { ...result, file: { status: "invalid", message: "JSON ilegível no repositório remoto." } };
    }
    if (envelope && Number.isInteger(envelope.schemaVersion) && envelope.schemaVersion > mod.schemaVersion) {
      return { ...result, file: { status: "newer", message: "O arquivo foi criado por uma versão mais nova do LKR LAB (v" + envelope.schemaVersion + ")." } };
    }
    const prepared = mod.prepare(envelope);
    if (!prepared.ok) return { ...result, file: { status: "invalid", message: prepared.error } };
    return { ...result, file: { status: "ok", state: prepared.state, stateHash: prepared.hash, updatedAt: envelope.updatedAt || null } };
  }

  /** POST /api/lab-sync */
  async function sync(moduleId, payload) {
    const mod = moduleOf(moduleId);
    return exclusive(async () => {
      if (!payload || typeof payload !== "object" || payload.schemaVersion !== mod.schemaVersion) {
        throw new BackupError("INVALID_PAYLOAD", { status: 400, message: "schemaVersion ausente ou incompatível." });
      }
      const prepared = mod.prepare(payload.state);
      if (!prepared.ok) throw new BackupError("INVALID_PAYLOAD", { status: 400, detail: prepared.error });

      const info = await inspect();
      await fetchRemote(info.branch);
      const before = await divergence(info.branch);
      if (!before) throw new BackupError("NO_REMOTE_BRANCH");
      if (before.behind > 0 && before.ahead > 0) throw new BackupError("DIVERGED");
      if (before.behind > 0) throw new BackupError("REMOTE_AHEAD");

      // Commits locais pendentes com outros arquivos: não publicar nada.
      const pendingBefore = await unpushedFiles();
      // Commits só com arquivos de dados do LKR LAB (este ou outro módulo) podem seguir juntos.
      for (const file of MODULE_FILES) pendingBefore.delete(file);
      if (pendingBefore.size) throw new BackupError("UNRELATED_UNPUSHED", { detail: [...pendingBefore].slice(0, 10).join(", ") });

      const date = now();
      const current = await readFileState(mod);
      if (!current || current.hash !== prepared.hash) {
        const envelope = {
          schemaVersion: mod.schemaVersion,
          source: mod.source,
          module: moduleId,
          updatedAt: isoWithOffset(date),
          stateHash: prepared.hash,
          state: prepared.state,
        };
        await writeAtomic(mod, JSON.stringify(envelope, null, 2) + "\n");
      }

      let committed = false;
      if (await fileStatus(mod)) {
        await gitOrThrow(["add", "--", mod.file]);
        // --only: o commit leva só este arquivo, mesmo que haja outras mudanças já no index.
        await gitOrThrow(["commit", "--only", "--quiet", "-m", "chore(lab): sync " + mod.label + " — " + commitStamp(date), "--", mod.file]);
        committed = true;
        const touched = lines(await gitOrThrow(["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]));
        if (touched.length !== 1 || touched[0] !== mod.file) {
          throw new BackupError("GIT_FAILED", { message: "O commit criado contém arquivos inesperados; o push foi cancelado.", detail: touched.join(", ") });
        }
      }

      const pending = await unpushedFiles();
      const commit = (await gitOrThrow(["rev-parse", "--short", "HEAD"])).trim();
      const result = { success: true, branch: info.branch, repo: info.remoteLabel, commit, stateHash: prepared.hash, file: mod.file, timestamp: date.toISOString() };

      if (!pending.size) {
        return Object.assign(result, { pushed: false, alreadySynced: true, message: "Já está sincronizado.", commit: (await lastModuleCommit(mod) || {}).hash || commit });
      }

      const push = await git(["push", "--quiet", "origin", "refs/heads/" + info.branch + ":refs/heads/" + info.branch], TIMEOUT_REMOTE_MS);
      if (!push.ok) {
        const code = classifyGitError(push.stderr, push.timedOut);
        const error = new BackupError(code, { detail: push.stderr });
        if (committed || pending.size) error.message += " O commit local " + commit + " foi mantido e será enviado na próxima sincronização.";
        error.commit = commit;
        throw error;
      }
      return Object.assign(result, { pushed: true, alreadySynced: false, committed, message: "Sincronizado com GitHub." });
    });
  }

  /**
   * POST /api/lab-update — traz commits do GitHub somente por fast-forward.
   * Se houver divergência ou alterações locais em risco, para sem mexer em nada.
   */
  async function update(moduleId = "lab-setup", { strict = false } = {}) {
    const mod = moduleOf(moduleId);
    return exclusive(async () => {
      const info = await inspect();
      await fetchRemote(info.branch);
      const counts = await divergence(info.branch);
      if (!counts) throw new BackupError("NO_REMOTE_BRANCH");
      if (counts.behind === 0) return { success: true, updated: false, commits: 0, stateChanged: false, labStateChanged: false, branch: info.branch, message: "O repositório local já está atualizado." };
      if (counts.ahead > 0) throw new BackupError("DIVERGED");
      if (await fileStatus(mod)) throw new BackupError("LOCAL_CHANGES", { message: "O arquivo " + mod.file + " tem alterações locais não commitadas. Nada foi alterado." });
      if (strict) {
        // Pedido pelo sync do desktop: nunca mover a árvore de quem está desenvolvendo.
        const dirty = lines((await git(["status", "--porcelain=v1", "--untracked-files=no"])).stdout);
        if (dirty.length) {
          throw new BackupError("LOCAL_CHANGES", { message: "Há alterações não commitadas no repositório. Atualize-o manualmente (git pull) e sincronize de novo. Nada foi alterado." });
        }
      }

      const oldHead = (await gitOrThrow(["rev-parse", "HEAD"])).trim();
      await gitOrThrow(["merge", "--ff-only", "--quiet", "refs/remotes/origin/" + info.branch]);
      const newHead = (await gitOrThrow(["rev-parse", "HEAD"])).trim();
      const changed = lines(await gitOrThrow(["diff", "--name-only", oldHead, newHead, "--", mod.file])).length > 0;
      return {
        success: true,
        updated: true,
        commits: counts.behind,
        stateChanged: changed,
        labStateChanged: changed, // nome antigo, mantido para o Lab Setup
        branch: info.branch,
        commit: newHead.slice(0, 7),
        message: counts.behind + (counts.behind === 1 ? " commit trazido do GitHub." : " commits trazidos do GitHub."),
      };
    });
  }

  return { status, readState, remoteState, sync, update, repoRoot };
}
