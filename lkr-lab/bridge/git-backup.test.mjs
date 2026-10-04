// Testes com Git real: repositórios temporários e um remote "bare" local
// fazem o papel do GitHub. Nada aqui toca o repositório do projeto.
import { afterAll, describe, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import { promises as fs, realpathSync } from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { classifyGitError, createGitBackup, describeRemote, redact } from "./git-backup.mjs";
import { createLabServer } from "./server.mjs";
import "../core/lkr-portable.js";

const lab = globalThis.LKR.labSetup;
const portable = globalThis.LKR.portable;

function memoryStorage() {
  const data = new Map();
  return { get: (key) => (data.has(key) ? data.get(key) : null), set: (key, value) => (data.set(key, value), true), remove: (key) => data.delete(key) };
}

const T = 60_000;
const FILE = "data/lab-setup.json";
const NOTE = "Comprar multímetro depois do pagamento.";
const temps = [];

let ENV;

async function baseDir() {
  const dir = realpathSync.native(await fs.mkdtemp(path.join(os.tmpdir(), "lkr-git-")));
  temps.push(dir);
  const emptyConfig = path.join(dir, "empty.gitconfig");
  await fs.writeFile(emptyConfig, "");
  ENV = {
    GIT_AUTHOR_NAME: "LKR Test",
    GIT_AUTHOR_EMAIL: "lkr@example.invalid",
    GIT_COMMITTER_NAME: "LKR Test",
    GIT_COMMITTER_EMAIL: "lkr@example.invalid",
    // Isola dos helpers/hooks globais da máquina (credential manager incluso).
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_CONFIG_GLOBAL: emptyConfig,
  };
  return dir;
}

const git = (cwd, ...args) => execFileSync("git", args, { cwd, env: { ...process.env, ...ENV }, encoding: "utf8" }).trim();

async function setup() {
  const base = await baseDir();
  const remote = path.join(base, "remote.git");
  const work = path.join(base, "work");
  git(base, "init", "--quiet", "--bare", "-b", "main", remote);
  git(base, "init", "--quiet", "-b", "main", work);
  await fs.writeFile(path.join(work, "README.md"), "# projeto\n");
  git(work, "add", "README.md");
  git(work, "commit", "--quiet", "-m", "init");
  git(work, "remote", "add", "origin", remote);
  git(work, "push", "--quiet", "-u", "origin", "main");
  return { base, remote, work, backup: createGitBackup({ repoRoot: work, env: ENV }) };
}

async function cloneOf(ctx, name) {
  const dir = path.join(ctx.base, name);
  git(ctx.base, "clone", "--quiet", ctx.remote, dir);
  return { dir, backup: createGitBackup({ repoRoot: dir, env: ENV }) };
}

function payload(notes = NOTE, extra = {}) {
  return {
    schemaVersion: 1,
    state: {
      version: 1,
      createdAt: "2026-09-30T12:00:00.000Z",
      updatedAt: "2026-09-30T12:30:00.000Z",
      items: { multimetro: { completed: false, completedAt: null, notes, updatedAt: "2026-09-30T12:30:00.000Z" }, trena: { completed: true, completedAt: "2026-09-30T12:10:00.000Z", notes: "" } },
      custom: [],
      ui: { status: "done", category: "eletronica" },
      ...extra,
    },
  };
}

async function rejects(promise) {
  try {
    await promise;
  } catch (error) {
    return error;
  }
  throw new Error("era esperado um erro");
}

const exists = (p) => fs.access(p).then(() => true, () => false);
const commitCount = (dir) => Number(git(dir, "rev-list", "--count", "HEAD"));
const remoteHead = (ctx) => git(ctx.base, "--git-dir", ctx.remote, "rev-parse", "main");

afterAll(async () => {
  for (const dir of temps) await fs.rm(dir, { recursive: true, force: true, maxRetries: 3 });
});

describe.concurrent("git-backup: sync", () => {
  it("grava o JSON e commita/publica SOMENTE data/lab-setup.json", async () => {
    const ctx = await setup();
    // Trabalho em andamento no projeto: não pode entrar no commit.
    await fs.writeFile(path.join(ctx.work, "README.md"), "# projeto\nwip\n");
    await fs.writeFile(path.join(ctx.work, "staged.txt"), "já no index\n");
    git(ctx.work, "add", "staged.txt");
    await fs.writeFile(path.join(ctx.work, "scratch.txt"), "não rastreado\n");

    const result = await ctx.backup.sync("lab-setup", payload());
    expect(result).toMatchObject({ success: true, pushed: true, branch: "main", file: FILE });
    expect(result.commit).toMatch(/^[0-9a-f]{7,}$/);

    const [subject, ...files] = git(ctx.base, "--git-dir", ctx.remote, "log", "-1", "--name-only", "--format=%s", "main").split(/\n+/);
    expect(subject).toMatch(/^chore\(lab\): sync Lab Setup — \d{4}-\d{2}-\d{2} \d{2}:\d{2}$/);
    expect(subject).not.toContain("multímetro");
    expect(files).toEqual([FILE]);

    const published = JSON.parse(git(ctx.base, "--git-dir", ctx.remote, "show", "main:" + FILE));
    expect(published).toMatchObject({ schemaVersion: 1, source: "lkr-lab", module: "lab-setup", stateHash: result.stateHash });
    expect(published.updatedAt).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[+-]\d{2}:\d{2}$/);
    expect(published.state.items.multimetro.notes).toBe(NOTE);
    expect(published.state.ui).toBeUndefined(); // filtros não são persistidos

    expect(git(ctx.work, "diff", "--cached", "--name-only")).toBe("staged.txt");
    expect(git(ctx.work, "status", "--porcelain", "README.md")).toMatch(/^M/);
    expect(await exists(path.join(ctx.work, "scratch.txt"))).toBe(true);
  }, T);

  it("não cria commit vazio quando nada mudou (filtros e carimbos não contam)", async () => {
    const ctx = await setup();
    await ctx.backup.sync("lab-setup", payload());
    const count = commitCount(ctx.work);
    const again = await ctx.backup.sync("lab-setup", payload(NOTE, { updatedAt: "2026-10-01T09:00:00.000Z", ui: { status: "all", category: "all" } }));
    expect(again).toMatchObject({ success: true, alreadySynced: true, pushed: false, message: "Já está sincronizado." });
    expect(commitCount(ctx.work)).toBe(count);

    const changed = await ctx.backup.sync("lab-setup", payload("Comprado na loja."));
    expect(changed.pushed).toBe(true);
    expect(commitCount(ctx.work)).toBe(count + 1);
  }, T);

  it("remoto à frente: não escreve, não commita e nunca força", async () => {
    const ctx = await setup();
    await ctx.backup.sync("lab-setup", payload());
    const other = await cloneOf(ctx, "other");
    await other.backup.sync("lab-setup", payload("Editado em outro computador."));
    const head = remoteHead(ctx);
    const before = await fs.readFile(path.join(ctx.work, FILE), "utf8");
    const count = commitCount(ctx.work);

    const error = await rejects(ctx.backup.sync("lab-setup", payload("Mudança local")));
    expect(error.code).toBe("REMOTE_AHEAD");
    expect(error.message).toContain("remoto possui alterações mais recentes");
    expect(remoteHead(ctx)).toBe(head);
    expect(commitCount(ctx.work)).toBe(count);
    expect(await fs.readFile(path.join(ctx.work, FILE), "utf8")).toBe(before);
  }, T);

  it("commits locais não publicados com outros arquivos bloqueiam o push", async () => {
    const ctx = await setup();
    await fs.writeFile(path.join(ctx.work, "README.md"), "# projeto\nfeature local\n");
    git(ctx.work, "commit", "--quiet", "-am", "wip: feature");
    const head = remoteHead(ctx);
    const error = await rejects(ctx.backup.sync("lab-setup", payload()));
    expect(error.code).toBe("UNRELATED_UNPUSHED");
    expect(error.detail).toContain("README.md");
    expect(remoteHead(ctx)).toBe(head);
    expect(await exists(path.join(ctx.work, FILE))).toBe(false);
  }, T);

  it("rejeita payload inválido sem escrever nada", async () => {
    const ctx = await setup();
    for (const bad of [null, { schemaVersion: 2, state: payload().state }, { schemaVersion: 1, state: { version: 1 } }, { schemaVersion: 1, state: { version: 9, items: {} } }]) {
      expect((await rejects(ctx.backup.sync("lab-setup", bad))).code).toBe("INVALID_PAYLOAD");
    }
    expect((await rejects(ctx.backup.sync("run-command", payload()))).code).toBe("INVALID_PAYLOAD");
    expect(await exists(path.join(ctx.work, "data"))).toBe(false);
  }, T);
});

describe.concurrent("git-backup: atualizar e restaurar", () => {
  it("atualiza por fast-forward e expõe o backup remoto para restauração", async () => {
    const ctx = await setup();
    await ctx.backup.sync("lab-setup", payload());
    const other = await cloneOf(ctx, "other");
    await other.backup.sync("lab-setup", payload("Nota do notebook."));

    const status = await ctx.backup.status({ fetch: true });
    expect(status).toMatchObject({ repository: true, branch: "main", remote: true, behind: 1, ahead: 0 });

    const update = await ctx.backup.update();
    expect(update).toMatchObject({ success: true, updated: true, commits: 1, labStateChanged: true });
    const state = await ctx.backup.readState();
    expect(state.synced).toBe(true);
    expect(state.backup.state.items.multimetro.notes).toBe("Nota do notebook.");
    expect(state.commit.hash).toMatch(/^[0-9a-f]{7,}$/);

    expect((await ctx.backup.update()).updated).toBe(false);
  }, T);

  it("divergência: update e sync param sem alterar nada", async () => {
    const ctx = await setup();
    await ctx.backup.sync("lab-setup", payload());
    const other = await cloneOf(ctx, "other");
    await other.backup.sync("lab-setup", payload("Remoto"));
    await fs.writeFile(path.join(ctx.work, "README.md"), "# local\n");
    git(ctx.work, "commit", "--quiet", "-am", "local");
    const localHead = git(ctx.work, "rev-parse", "HEAD");

    expect((await rejects(ctx.backup.update())).code).toBe("DIVERGED");
    expect((await rejects(ctx.backup.sync("lab-setup", payload("x")))).code).toBe("DIVERGED");
    expect(git(ctx.work, "rev-parse", "HEAD")).toBe(localHead);
  }, T);

  it("update para se o arquivo de dados tiver alteração local não commitada", async () => {
    const ctx = await setup();
    await ctx.backup.sync("lab-setup", payload());
    const other = await cloneOf(ctx, "other");
    await other.backup.sync("lab-setup", payload("Remoto"));
    await fs.appendFile(path.join(ctx.work, FILE), " ");
    const error = await rejects(ctx.backup.update());
    expect(error.code).toBe("LOCAL_CHANGES");
    expect(git(ctx.work, "status", "--porcelain", FILE)).toMatch(/^M\s+data\/lab-setup\.json$/);
  }, T);

  it("backup ausente ou inválido é informado", async () => {
    const ctx = await setup();
    expect((await rejects(ctx.backup.readState())).code).toBe("NO_BACKUP");
    await fs.mkdir(path.join(ctx.work, "data"));
    await fs.writeFile(path.join(ctx.work, FILE), "{quebrado");
    expect((await rejects(ctx.backup.readState())).code).toBe("INVALID_BACKUP");
    await fs.writeFile(path.join(ctx.work, FILE), JSON.stringify({ schemaVersion: 7, source: "lkr-lab", module: "lab-setup", state: { version: 7, items: {} } }));
    expect((await rejects(ctx.backup.readState())).code).toBe("INVALID_BACKUP");
  }, T);
});

describe.concurrent("git-backup: ambiente com problema", () => {
  it("repositório inexistente", async () => {
    const dir = path.join(await baseDir(), "sem-git");
    await fs.mkdir(dir);
    const backup = createGitBackup({ repoRoot: dir, env: ENV });
    expect(await backup.status()).toMatchObject({ repository: false });
    expect((await rejects(backup.sync("lab-setup", payload()))).code).toBe("NO_REPO");
  }, T);

  it("remote origin ausente", async () => {
    const base = await baseDir();
    const work = path.join(base, "solo");
    git(base, "init", "--quiet", "-b", "main", work);
    git(work, "commit", "--quiet", "--allow-empty", "-m", "init");
    const backup = createGitBackup({ repoRoot: work, env: ENV });
    expect(await backup.status()).toMatchObject({ repository: true, remote: false });
    expect((await rejects(backup.sync("lab-setup", payload()))).code).toBe("NO_REMOTE");
  }, T);

  it("erro de rede não cria commit e não vaza credenciais", async () => {
    const ctx = await setup();
    const count = commitCount(ctx.work);
    git(ctx.work, "remote", "set-url", "origin", "https://lukas:ghp_SuperSecretToken123456@127.0.0.1:9/LKR_Lab.git");
    const error = await rejects(ctx.backup.sync("lab-setup", payload()));
    expect(error.code).toBe("NETWORK");
    expect(commitCount(ctx.work)).toBe(count);
    const status = await ctx.backup.status({ fetch: true });
    const exposed = JSON.stringify({ message: error.message, detail: error.detail, status });
    expect(exposed).not.toContain("ghp_");
    expect(exposed).not.toContain("SuperSecret");
    expect(status.fetchError.code).toBe("NETWORK");
  }, T);

  it("erro de autenticação é identificado", async () => {
    const ctx = await setup();
    const server = http.createServer((req, res) => res.writeHead(401, { "WWW-Authenticate": 'Basic realm="git"' }).end());
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    try {
      git(ctx.work, "remote", "set-url", "origin", "http://127.0.0.1:" + server.address().port + "/LKR_Lab.git");
      const error = await rejects(ctx.backup.sync("lab-setup", payload()));
      expect(error.code).toBe("AUTH");
      expect(await exists(path.join(ctx.work, FILE))).toBe(false);
    } finally {
      server.close();
    }
  }, T);
});

describe("git-backup: utilitários", () => {
  it("classifica erros do Git", () => {
    expect(classifyGitError("fatal: Authentication failed for 'https://github.com/x/y.git/'")).toBe("AUTH");
    expect(classifyGitError("fatal: could not read Username for 'https://github.com': terminal prompts disabled")).toBe("AUTH");
    expect(classifyGitError("remote: Permission to x/y.git denied to z.\nfatal: unable to access '...': The requested URL returned error: 403")).toBe("AUTH");
    expect(classifyGitError("fatal: unable to access 'https://github.com/x/y.git/': Could not resolve host: github.com")).toBe("NETWORK");
    expect(classifyGitError(" ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs")).toBe("REMOTE_AHEAD");
    expect(classifyGitError("fatal: 'origin' does not appear to be a git repository")).toBe("NO_REMOTE");
    expect(classifyGitError("", true)).toBe("NETWORK");
  });

  it("descreve o remote sem credenciais", () => {
    expect(describeRemote("https://github.com/LukasAlexandre/LKR_Lab.git")).toBe("LukasAlexandre/LKR_Lab");
    expect(describeRemote("https://user:ghp_abc123456789@github.com/LukasAlexandre/LKR_Lab.git")).toBe("LukasAlexandre/LKR_Lab");
    expect(describeRemote("git@github.com:LukasAlexandre/LKR_Lab.git")).toBe("LukasAlexandre/LKR_Lab");
    expect(redact("unable to access 'https://u:ghp_abcdefghijk@github.com/x.git'")).not.toContain("ghp_");
  });
});

describe("estado portátil entre máquinas (Git real)", () => {
  // Mesma sequência do navegador: GET /api/lab-state → reconcile → importData.
  async function open(machine, storage) {
    const store = lab.createStore({ catalog: lab.catalog, storage });
    const meta = portable.createMetaStore(storage, lab.STORAGE_KEY + ":sync");
    let res;
    try {
      res = { ok: true, data: await machine.backup.readState() };
    } catch (error) {
      res = { ok: false, error: { code: error.code } };
    }
    const file = portable.fileFromResponse(res);
    const decision = portable.reconcile({ localHash: store.contentHash(), localEmpty: store.isEmpty(), baseHash: meta.get().baseHash, file });
    if (decision.action === "adopt") {
      expect(store.importData(res.data.backup, { reason: "repo" }).ok).toBe(true);
      meta.update({ baseHash: file.hash });
    } else if (decision.action === "mark-base") meta.update({ baseHash: file.hash });
    return { store, meta, decision };
  }

  it("casa → GitHub → trabalho: clonar e abrir restaura; edição dos dois lados vira conflito", async () => {
    const ctx = await setup();
    const casaStorage = memoryStorage();
    const casa = await open({ backup: ctx.backup }, casaStorage);
    expect(casa.decision.status).toBe("empty");
    casa.store.setCompleted("trena", true);
    casa.store.setNotes("multimetro", NOTE);
    casa.store.setUi({ status: "done", category: "all" });
    const sent = await ctx.backup.sync("lab-setup", { schemaVersion: 1, state: casa.store.persistable() });
    casa.meta.update({ baseHash: sent.stateHash });

    const trabalho = await cloneOf(ctx, "trabalho");
    const trabalhoStorage = memoryStorage();
    const first = await open(trabalho, trabalhoStorage);
    expect(first.decision).toEqual({ status: "in-sync", action: "adopt" });
    expect(first.store.getItem("multimetro").notes).toBe(NOTE);
    expect(first.store.state.ui).toEqual({ status: "all", category: "all" });
    expect((await open(trabalho, trabalhoStorage)).decision.action).toBe("mark-base");

    // Casa publica de novo; trabalho sem edições recebe por fast-forward e adota.
    casa.store.setNotes("trena", "Emprestei ao vizinho.");
    await ctx.backup.sync("lab-setup", { schemaVersion: 1, state: casa.store.persistable() });
    expect((await trabalho.backup.update()).labStateChanged).toBe(true);
    const second = await open(trabalho, trabalhoStorage);
    expect(second.decision.action).toBe("adopt");
    expect(second.store.getItem("trena").notes).toBe("Emprestei ao vizinho.");

    // Os dois editam: trabalho não perde nada e não adota.
    second.store.setNotes("esquadro", "Só no trabalho.");
    casa.store.setNotes("esquadro", "Só em casa.");
    await ctx.backup.sync("lab-setup", { schemaVersion: 1, state: casa.store.persistable() });
    await trabalho.backup.update();
    const third = await open(trabalho, trabalhoStorage);
    expect(third.decision.status).toBe("conflict");
    expect(third.store.getItem("esquadro").notes).toBe("Só no trabalho.");
  }, T);

  it("arquivo portátil corrompido não é adotado nem sobrescrito na leitura", async () => {
    const ctx = await setup();
    await fs.mkdir(path.join(ctx.work, "data"), { recursive: true });
    await fs.writeFile(path.join(ctx.work, FILE), "{corrompido");
    const storage = memoryStorage();
    const before = lab.createStore({ catalog: lab.catalog, storage });
    before.setNotes("trena", "local");
    const opened = await open({ backup: ctx.backup }, storage);
    expect(opened.decision).toEqual({ status: "invalid", action: "none" });
    expect(opened.store.getItem("trena").notes).toBe("local");
    expect(await fs.readFile(path.join(ctx.work, FILE), "utf8")).toBe("{corrompido");
  }, T);
});

describe("git-backup: módulo workspace", () => {
  const WS_FILE = "data/workspace.json";
  const workspaceState = (extra = {}) => ({
    version: 1,
    projects: [{ id: "p1", name: "LKR_Lab", description: "", repository: "https://github.com/org/lkr", stack: [], tags: [], ports: [], commands: [], createdAt: "", updatedAt: "" }],
    prompts: [],
    knowledge: [],
    preferences: {},
    ...extra,
  });

  it("publica só data/workspace.json, sem caminho local, e lê de volta", async () => {
    const ctx = await setup();
    const input = workspaceState();
    input.projects[0].localPath = "C:\\Users\\segredo\\Dev\\lkr";
    const result = await ctx.backup.sync("workspace", { schemaVersion: 1, state: input });
    expect(result).toMatchObject({ success: true, pushed: true, file: WS_FILE });
    const files = git(ctx.base, "--git-dir", ctx.remote, "log", "-1", "--name-only", "--format=", "main").split(/\n+/).filter(Boolean);
    expect(files).toEqual([WS_FILE]);
    const published = git(ctx.base, "--git-dir", ctx.remote, "show", "main:" + WS_FILE);
    expect(published).not.toMatch(/localPath|segredo/);
    expect(JSON.parse(published)).toMatchObject({ module: "workspace", source: "lkr-lab", schemaVersion: 5 });
    const read = await ctx.backup.readState("workspace");
    expect(read.synced).toBe(true);
    expect(read.backup.state.projects[0].id).toBe("p1");
    expect(await exists(path.join(ctx.work, FILE))).toBe(false);
  }, T);

  it("rejeita workspace inválido ou com credencial sem escrever nada", async () => {
    const ctx = await setup();
    const withToken = workspaceState({ prompts: [{ id: "q", title: "T", category: "", projectId: null, body: "ghp_" + "a".repeat(36) }] });
    expect((await rejects(ctx.backup.sync("workspace", { schemaVersion: 1, state: withToken }))).code).toBe("INVALID_PAYLOAD");
    expect((await rejects(ctx.backup.sync("workspace", { schemaVersion: 1, state: { version: 9 } }))).code).toBe("INVALID_PAYLOAD");
    expect((await rejects(ctx.backup.sync("workspace", { schemaVersion: 6, state: workspaceState() }))).code).toBe("INVALID_PAYLOAD");
    expect(await exists(path.join(ctx.work, "data"))).toBe(false);
  }, T);

  it("workspace inexistente ou corrompido não é adotado", async () => {
    const ctx = await setup();
    expect((await rejects(ctx.backup.readState("workspace"))).code).toBe("NO_BACKUP");
    await fs.mkdir(path.join(ctx.work, "data"));
    await fs.writeFile(path.join(ctx.work, WS_FILE), "{quebrado");
    expect((await rejects(ctx.backup.readState("workspace"))).code).toBe("INVALID_BACKUP");
  }, T);

  it("remoteState lê o arquivo do ref remoto sem tocar na árvore de trabalho", async () => {
    const ctx = await setup();
    expect((await ctx.backup.remoteState("workspace")).file).toEqual({ status: "missing" });

    const other = await cloneOf(ctx, "other");
    await other.backup.sync("workspace", { schemaVersion: 1, state: workspaceState() });
    const headBefore = git(ctx.work, "rev-parse", "HEAD");
    const remote = await ctx.backup.remoteState("workspace");
    expect(remote).toMatchObject({ fetched: true, remote: true, remoteBranch: true, behind: 1, ahead: 0, branch: "main" });
    expect(remote.file.status).toBe("ok");
    expect(remote.file.state.projects[0].id).toBe("p1");
    expect(remote.file.stateHash).toMatch(/^h[0-9a-f]{16}$/);
    // Nada mudou no repositório de quem está desenvolvendo.
    expect(git(ctx.work, "rev-parse", "HEAD")).toBe(headBefore);
    expect(await exists(path.join(ctx.work, WS_FILE))).toBe(false);
    expect(git(ctx.work, "status", "--porcelain")).toBe("");
  }, T);

  it("remoteState distingue arquivo inválido e de versão futura; falha de rede não é exceção", async () => {
    const ctx = await setup();
    const other = await cloneOf(ctx, "other");
    await fs.mkdir(path.join(other.dir, "data"));
    const publish = async (text) => {
      await fs.writeFile(path.join(other.dir, WS_FILE), text);
      git(other.dir, "add", WS_FILE);
      git(other.dir, "commit", "--quiet", "-m", "x");
      git(other.dir, "push", "--quiet", "origin", "main");
    };
    await publish("{quebrado");
    expect((await ctx.backup.remoteState("workspace")).file.status).toBe("invalid");
    await publish(JSON.stringify({ schemaVersion: 9, source: "lkr-lab", module: "workspace", state: { version: 9 } }));
    expect((await ctx.backup.remoteState("workspace")).file).toMatchObject({ status: "newer" });
    await publish(JSON.stringify({ schemaVersion: 1, source: "lkr-lab", module: "workspace", state: { version: 1, projects: [{ id: "../x", name: "n" }] } }));
    expect((await ctx.backup.remoteState("workspace")).file.status).toBe("invalid");

    git(ctx.work, "remote", "set-url", "origin", path.join(ctx.base, "nao-existe.git"));
    const offline = await ctx.backup.remoteState("workspace");
    expect(offline.fetched).toBe(false);
    expect(offline.fetchError).toMatchObject({ code: expect.any(String) });
  }, T);

  it("update strict recusa mexer na árvore com alterações de desenvolvimento", async () => {
    const ctx = await setup();
    const other = await cloneOf(ctx, "other");
    await fs.writeFile(path.join(other.dir, "code.txt"), "novo\n");
    git(other.dir, "add", "code.txt");
    git(other.dir, "commit", "--quiet", "-m", "code");
    git(other.dir, "push", "--quiet", "origin", "main");

    await fs.writeFile(path.join(ctx.work, "README.md"), "# projeto\nwip\n");
    const head = git(ctx.work, "rev-parse", "HEAD");
    expect((await rejects(ctx.backup.update("workspace", { strict: true }))).code).toBe("LOCAL_CHANGES");
    expect(git(ctx.work, "rev-parse", "HEAD")).toBe(head);
    expect(await fs.readFile(path.join(ctx.work, "README.md"), "utf8")).toBe("# projeto\nwip\n");

    git(ctx.work, "checkout", "--quiet", "--", "README.md");
    expect(await ctx.backup.update("workspace", { strict: true })).toMatchObject({ success: true, updated: true });
    expect(await exists(path.join(ctx.work, "code.txt"))).toBe(true);
  }, T);

  it("módulo desconhecido (inclusive nomes do prototype) é recusado", async () => {
    const ctx = await setup();
    for (const id of ["constructor", "__proto__", "../x", "nope"]) {
      expect((await rejects(ctx.backup.readState(id))).code).toBe("INVALID_PAYLOAD");
    }
  }, T);
});

describe("bridge HTTP", () => {
  function request(port, { method = "GET", path: pathname, headers = {}, body }) {
    return new Promise((resolve, reject) => {
      const req = http.request({ host: "127.0.0.1", port, method, path: pathname, headers: { Host: "127.0.0.1:" + port, ...headers } }, (res) => {
        let data = "";
        res.on("data", (c) => (data += c));
        res.on("end", () => resolve({ status: res.statusCode, headers: res.headers, body: data }));
      });
      req.on("error", reject);
      if (body) req.write(body);
      req.end();
    });
  }

  it("aceita apenas requisições locais e da própria origem", async () => {
    const ctx = await setup();
    const server = createLabServer({ backup: ctx.backup });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const port = server.address().port;
    const origin = "http://127.0.0.1:" + port;
    const api = { "X-LKR-Lab": "1" };
    try {
      expect(server.address().address).toBe("127.0.0.1");
      expect((await request(port, { path: "/lab-setup/" })).status).toBe(200);
      expect((await request(port, { path: "/core/lkr-portable.js" })).status).toBe(200);
      expect((await request(port, { path: "/core/lkr-portable.test.js" })).status).toBe(404);
      // Uma origem só para o cache local: localhost redireciona para 127.0.0.1.
      const moved = await request(port, { path: "/lab-setup/?x=1", headers: { Host: "localhost:" + port } });
      expect(moved.status).toBe(308);
      expect(moved.headers.location).toBe(origin + "/lab-setup/?x=1");
      for (const blocked of ["/bridge/server.mjs", "/../package.json", "/%2e%2e/package.json", "/lab-setup/store.test.js", "/lab-setup/..%5c..%5cpackage.json"]) {
        expect((await request(port, { path: blocked })).status).toBe(404);
      }
      expect((await request(port, { path: "/api/git/status", headers: { Host: "evil.example:" + port } })).status).toBe(421);
      expect((await request(port, { path: "/api/git/status" })).status).toBe(403);
      const ok = await request(port, { path: "/api/git/status", headers: api });
      expect(ok.status).toBe(200);
      expect(JSON.parse(ok.body)).toMatchObject({ bridge: "lkr-lab", repository: true, branch: "main" });

      const body = JSON.stringify(payload());
      const json = { ...api, "Content-Type": "application/json" };
      expect((await request(port, { method: "POST", path: "/api/lab-sync", headers: { ...json, Origin: "https://evil.example" }, body })).status).toBe(403);
      expect((await request(port, { method: "POST", path: "/api/lab-sync", headers: { ...api, Origin: origin, "Content-Type": "text/plain" }, body })).status).toBe(415);
      expect((await request(port, { method: "POST", path: "/api/run-command", headers: { ...json, Origin: origin }, body: "{}" })).status).toBe(404);
      expect((await request(port, { method: "POST", path: "/api/lab-sync", headers: { ...json, Origin: origin }, body: "{x" })).status).toBe(400);

      const synced = await request(port, { method: "POST", path: "/api/lab-sync", headers: { ...json, Origin: origin }, body });
      expect(synced.status).toBe(200);
      expect(JSON.parse(synced.body)).toMatchObject({ success: true, pushed: true });
      const state = JSON.parse((await request(port, { path: "/api/lab-state", headers: api })).body);
      expect(state).toMatchObject({ success: true, synced: true });
      expect(state.backup.state.items.multimetro.notes).toBe(NOTE);
    } finally {
      server.close();
    }
  }, T);
});
