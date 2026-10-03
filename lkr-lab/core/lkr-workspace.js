/*
 * LKR LAB — workspace portátil (data/workspace.json)
 *
 * Formato canônico do que acompanha o usuário entre máquinas no app desktop:
 * projetos (identidade, sem caminho local), prompts, knowledge e preferências
 * marcadas como portáteis. Ver docs/STATE.md.
 *
 *   IDENTIDADE DO PROJETO != CAMINHO LOCAL
 *
 * O id do projeto (UUID gerado no cadastro) é a identidade. O caminho da pasta é
 * um vínculo desta máquina (tabela project_bindings do SQLite) e nunca entra aqui.
 *
 * Usado pelo bridge (valida antes de gravar/publicar) e pelo desktop (hash local
 * e pré-validação antes do sync). O Rust valida de novo antes de aplicar no SQLite.
 * Sem dependência de DOM. Depende de core/lkr-portable.js (hash de conteúdo).
 */
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  root.LKR = root.LKR || {};
  root.LKR.workspace = Object.assign(root.LKR.workspace || {}, api);
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  "use strict";

  /** v2 acrescenta `ddae`; v1 continua legível e vira v2 ao normalizar. */
  const SCHEMA_VERSION = 2;
  const SOURCE = "lkr-lab";
  const MODULE = "workspace";
  const ID_PATTERN = /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/;
  const KINDS = ["note", "decision", "architecture", "bug", "documentation"];
  const DENSITIES = ["comfortable", "compact"];
  /** Prompts que toda instalação cria na migration 001: não contam como conteúdo do usuário. */
  const SEED_PROMPT_IDS = ["audit", "bug", "pr", "continue", "security", "gate"];
  // Mesmos limites do hub-core (Rust), em bytes UTF-8.
  const LIMITS = {
    name: 100,
    description: 4000,
    listItems: 30,
    listText: 200,
    commandName: 100,
    program: 200,
    args: 50,
    arg: 1000,
    promptTitle: 240,
    promptCategory: 100,
    promptBody: 32000,
    knowledgeTitle: 240,
    knowledgeBody: 128000,
    knowledgeTags: 2000,
    timestamp: 40,
    favorites: 500,
    sessionTitle: 120,
    objective: 4000,
    blockTitle: 200,
    reason: 500,
    result: 2000,
    decisionTitle: 200,
    decisionBody: 8000,
    blocks: 500,
    decisions: 500,
    sessions: 10000,
  };

  // Padrões de credencial conhecidos: o sync é recusado em vez de publicar.
  const SECRET_PATTERNS = [
    /\bgh[pousr]_[A-Za-z0-9]{20,}/,
    /\bgithub_pat_[A-Za-z0-9_]{20,}/,
    /\bglpat-[A-Za-z0-9_-]{20,}/,
    /\bAKIA[0-9A-Z]{16}\b/,
    /\bAIza[0-9A-Za-z_-]{35}\b/,
    /\bxox[abprs]-[A-Za-z0-9-]{10,}/,
    /\bsk-(?:ant-|proj-)?[A-Za-z0-9_-]{20,}/,
    /-----BEGIN [A-Z ]*PRIVATE KEY-----/,
    /\b[a-z][a-z0-9+.-]*:\/\/[^\s/@:]+:[^\s/@]+@/i,
  ];

  const isPlainObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
  const bytes = (text) => new TextEncoder().encode(text).length;
  const portable = () => globalThis.LKR.portable;

  class InvalidWorkspace extends Error {}
  const fail = (where, message) => {
    throw new InvalidWorkspace(where + ": " + message);
  };

  function text(value, where, max, { required = false, trim = false } = {}) {
    if (value === undefined || value === null) value = "";
    if (typeof value !== "string") fail(where, "deve ser texto");
    if (trim) value = value.trim();
    if (required && !value) fail(where, "obrigatório");
    if (bytes(value) > max) fail(where, "excede " + max + " bytes");
    for (const pattern of SECRET_PATTERNS) if (pattern.test(value)) fail(where, "parece conter uma credencial; remova-a antes de sincronizar");
    return value;
  }

  function id(value, where) {
    if (typeof value !== "string" || !ID_PATTERN.test(value)) fail(where, "id inválido");
    return value;
  }

  function list(value, where, max) {
    if (value === undefined || value === null) return [];
    if (!Array.isArray(value)) fail(where, "deve ser uma lista");
    if (value.length > max) fail(where, "máximo de " + max + " itens");
    return value;
  }

  function stamp(value, where) {
    return text(value, where, LIMITS.timestamp);
  }

  /** Caminho absoluto de uma máquina (C:\…, \\servidor, /…, ~/…): nunca é portátil. */
  const isAbsolutePath = (value) => /^(?:[A-Za-z]:[\\/]|[\\/]{2}|\/|~[\\/])/.test(value);

  /** Mesma regra do hub-core: HTTPS, sem credenciais, query ou fragmento. */
  function validRepository(url) {
    if (!url.startsWith("https://")) return false;
    const rest = url.slice("https://".length);
    const slash = rest.indexOf("/");
    if (slash <= 0 || slash === rest.length - 1) return false;
    const host = rest.slice(0, slash);
    return !/[@?#\\\s\u0000-\u001f\u007f]/.test(url) && /^[A-Za-z0-9.-]+$/.test(host);
  }

  /**
   * Locator portátil: remote canônico ("host[:porta]/dono/repo") + caminho RELATIVO no repositório.
   * Nunca caminho absoluto, ".." , "\\", credencial ou esquema (mesmas regras do hub-core/portable.rs).
   */
  function locator(raw, where) {
    if (raw === undefined || raw === null) return undefined;
    if (!isPlainObject(raw)) fail(where, "locator inválido");
    const remote = text(raw.remote, where + ".remote", 400, { required: true, trim: true });
    const path = text(raw.path, where + ".path", 500, { trim: true }).replace(/^\/+|\/+$/g, "");
    const parts = remote.split("/");
    const hostOk = parts[0] !== "" && !/^[.-]/.test(parts[0]);
    const rest = parts.slice(1);
    const remoteOk =
      hostOk &&
      rest.length > 0 &&
      !remote.includes("://") &&
      !/[@\\?#\s]/.test(remote) &&
      // eslint-disable-next-line no-control-regex
      !/[\u0000-\u001f\u007f]/.test(remote) &&
      rest.every((s) => s !== "" && s !== "." && s !== ".." && !s.includes(":"));
    if (!remoteOk) fail(where + ".remote", "use o remote canônico (host/dono/repo), sem esquema, credencial ou caminho local");
    const pathOk =
      path === "" ||
      (!isAbsolutePath(path) &&
        !/[\\:]/.test(path) &&
        // eslint-disable-next-line no-control-regex
        !/[\u0000-\u001f\u007f]/.test(path) &&
        path.split("/").every((s) => s !== "" && s !== "." && s !== ".."));
    if (!pathOk) fail(where + ".path", "use um caminho relativo dentro do repositório (sem caminho local)");
    return { remote, path };
  }

  function project(raw, where) {
    if (!isPlainObject(raw)) fail(where, "projeto inválido");
    const repository = text(raw.repository, where + ".repository", 2048, { trim: true });
    if (repository && !validRepository(repository)) fail(where + ".repository", "use URL HTTPS sem credenciais, query ou fragmento");
    const strings = (value, field) =>
      list(value, where + "." + field, LIMITS.listItems)
        .map((item, i) => text(item, where + "." + field + "[" + i + "]", LIMITS.listText, { trim: true }))
        .filter(Boolean);
    const seenPorts = new Set();
    const ports = list(raw.ports, where + ".ports", LIMITS.listItems).map((port, i) => {
      const at = where + ".ports[" + i + "]";
      if (!isPlainObject(port)) fail(at, "porta inválida");
      if (!Number.isInteger(port.port) || port.port < 1 || port.port > 65535) fail(at + ".port", "entre 1 e 65535");
      if (seenPorts.has(port.port)) fail(at + ".port", "porta duplicada");
      seenPorts.add(port.port);
      return { name: text(port.name, at + ".name", LIMITS.listText, { required: true, trim: true }), port: port.port };
    });
    const commands = list(raw.commands, where + ".commands", LIMITS.listItems).map((command, i) => {
      const at = where + ".commands[" + i + "]";
      if (!isPlainObject(command)) fail(at, "comando inválido");
      const program = text(command.program, at + ".program", LIMITS.program, { required: true, trim: true });
      if (isAbsolutePath(program)) fail(at + ".program", "caminho absoluto é desta máquina; use o nome do programa no PATH");
      const args = list(command.args, at + ".args", LIMITS.args).map((arg, j) => {
        const value = text(arg, at + ".args[" + j + "]", LIMITS.arg);
        if (isAbsolutePath(value)) fail(at + ".args[" + j + "]", "caminho absoluto é desta máquina; use caminho relativo ao projeto");
        return value;
      });
      return { name: text(command.name, at + ".name", LIMITS.commandName, { trim: true }), program, args };
    });
    const found = locator(raw.locator, where + ".locator");
    return {
      id: id(raw.id, where + ".id"),
      name: text(raw.name, where + ".name", LIMITS.name, { required: true, trim: true }),
      description: text(raw.description, where + ".description", LIMITS.description),
      repository,
      // Só aparece quando existe: workspaces antigos (sem locator) mantêm a mesma forma canônica.
      ...(found ? { locator: found } : {}),
      stack: strings(raw.stack, "stack"),
      tags: strings(raw.tags, "tags"),
      ports,
      commands,
      createdAt: stamp(raw.createdAt, where + ".createdAt"),
      updatedAt: stamp(raw.updatedAt, where + ".updatedAt"),
    };
  }

  function projectRef(value, where, projectIds) {
    if (value === undefined || value === null || value === "") return null;
    id(value, where);
    if (!projectIds.has(value)) fail(where, "referencia um projeto que não está no workspace");
    return value;
  }

  function prompt(raw, where, projectIds) {
    if (!isPlainObject(raw)) fail(where, "prompt inválido");
    return {
      id: id(raw.id, where + ".id"),
      title: text(raw.title, where + ".title", LIMITS.promptTitle, { required: true, trim: true }),
      category: text(raw.category, where + ".category", LIMITS.promptCategory, { trim: true }),
      projectId: projectRef(raw.projectId, where + ".projectId", projectIds),
      body: text(raw.body, where + ".body", LIMITS.promptBody, { required: true }),
    };
  }

  function knowledge(raw, where, projectIds) {
    if (!isPlainObject(raw)) fail(where, "conhecimento inválido");
    if (!KINDS.includes(raw.kind)) fail(where + ".kind", "tipo desconhecido");
    return {
      id: id(raw.id, where + ".id"),
      projectId: projectRef(raw.projectId, where + ".projectId", projectIds),
      title: text(raw.title, where + ".title", LIMITS.knowledgeTitle, { required: true, trim: true }),
      kind: raw.kind,
      body: text(raw.body, where + ".body", LIMITS.knowledgeBody, { required: true }),
      tags: text(raw.tags, where + ".tags", LIMITS.knowledgeTags),
      updatedAt: stamp(raw.updatedAt, where + ".updatedAt"),
    };
  }

  // ------------------------------------------------------------------ DDAE (v2)

  const SESSION_STATUSES = ["active", "frozen", "stopped", "completed"];
  const BLOCK_STATUSES = ["pending", "in_progress", "completed"];
  // eslint-disable-next-line no-control-regex
  const CONTROL_CHARS = /[\u0000-\u0008\u000b-\u001f\u007f-\u009f]/;

  /** Mesma heurística do hub-core (ddae.rs): caminho absoluto de máquina dentro de texto livre. */
  const hasMachinePath = (value) =>
    /(?:^|[^A-Za-z0-9])[A-Za-z]:(?:\\|\/(?!\/))/.test(value) ||
    value.includes("\\\\") ||
    value.includes("/Users/") ||
    value.includes("/home/") ||
    value.includes("~/") ||
    value.includes("~\\");

  /** Texto do DDAE: aparado, sem controle, sem credencial e SEM caminho local (estado portátil). */
  function ddaeText(value, where, max, { required = false } = {}) {
    const result = text(value, where, max, { required, trim: true });
    if (CONTROL_CHARS.test(result)) fail(where, "contém caracteres de controle");
    if (hasMachinePath(result)) fail(where, "contém caminho local; o DDAE é portátil e não guarda caminhos");
    return result;
  }

  const optionalDdaeText = (value, where, max) => ddaeText(value, where, max) || undefined;

  function claimId(seen, value, where) {
    if (seen.has(value)) fail(where, "id duplicado (" + value + ")");
    seen.add(value);
  }

  function session(raw, where, projectIds, seen) {
    if (!isPlainObject(raw)) fail(where, "sessão inválida");
    const sessionId = id(raw.id, where + ".id");
    claimId(seen, sessionId, where + ".id");
    const projectId = id(raw.projectId, where + ".projectId");
    if (!projectIds.has(projectId)) fail(where + ".projectId", "referencia um projeto que não está no workspace");
    if (!Number.isInteger(raw.number) || raw.number < 1) fail(where + ".number", "deve ser um inteiro a partir de 1");
    if (!SESSION_STATUSES.includes(raw.status)) fail(where + ".status", "estado desconhecido");
    const blocks = list(raw.blocks, where + ".blocks", LIMITS.blocks).map((block, i) => {
      const at = where + ".blocks[" + i + "]";
      if (!isPlainObject(block)) fail(at, "bloco inválido");
      if (!BLOCK_STATUSES.includes(block.status)) fail(at + ".status", "estado desconhecido");
      const blockId = id(block.id, at + ".id");
      claimId(seen, blockId, at + ".id");
      return { id: blockId, title: ddaeText(block.title, at + ".title", LIMITS.blockTitle, { required: true }), status: block.status };
    });
    if (blocks.filter((b) => b.status === "in_progress").length > 1) fail(where + ".blocks", "mais de um bloco em andamento");
    if (raw.status === "completed" && (!blocks.length || blocks.some((b) => b.status !== "completed"))) {
      fail(where + ".status", "finalizada exige todos os blocos concluídos e nenhum em andamento");
    }
    const decisions = list(raw.decisions, where + ".decisions", LIMITS.decisions).map((decision, i) => {
      const at = where + ".decisions[" + i + "]";
      if (!isPlainObject(decision)) fail(at, "decisão inválida");
      const decisionId = id(decision.id, at + ".id");
      claimId(seen, decisionId, at + ".id");
      return {
        id: decisionId,
        title: ddaeText(decision.title, at + ".title", LIMITS.decisionTitle, { required: true }),
        body: ddaeText(decision.body, at + ".body", LIMITS.decisionBody),
        createdAt: stamp(decision.createdAt, at + ".createdAt"),
      };
    });
    const pauseReason = optionalDdaeText(raw.pauseReason, where + ".pauseReason", LIMITS.reason);
    const result = optionalDdaeText(raw.result, where + ".result", LIMITS.result);
    const completedAt = stamp(raw.completedAt, where + ".completedAt") || undefined;
    return {
      id: sessionId,
      projectId,
      number: raw.number,
      title: ddaeText(raw.title, where + ".title", LIMITS.sessionTitle, { required: true }),
      objective: ddaeText(raw.objective, where + ".objective", LIMITS.objective),
      status: raw.status,
      ...(pauseReason ? { pauseReason } : {}),
      ...(result ? { result } : {}),
      blocks,
      decisions,
      createdAt: stamp(raw.createdAt, where + ".createdAt"),
      updatedAt: stamp(raw.updatedAt, where + ".updatedAt"),
      ...(completedAt ? { completedAt } : {}),
    };
  }

  /** Invariantes entre sessões: numeração única por projeto e no máximo uma ativa por projeto. */
  function ddaeInvariants(sessions) {
    const numbers = new Set();
    const active = new Set();
    sessions.forEach((s, i) => {
      const key = s.projectId + "#" + s.number;
      if (numbers.has(key)) fail("ddae[" + i + "].number", "número repetido no projeto");
      numbers.add(key);
      if (s.status === "active") {
        if (active.has(s.projectId)) fail("ddae[" + i + "].status", "o projeto já tem outra sessão ativa (no máximo uma)");
        active.add(s.projectId);
      }
    });
  }

  const bySession = (a, b) => (a.projectId < b.projectId ? -1 : a.projectId > b.projectId ? 1 : a.number - b.number);

  function preferences(raw) {
    const value = isPlainObject(raw) ? raw : {};
    const favorites = Array.isArray(value.promptFavorites) ? value.promptFavorites.filter((f) => typeof f === "string" && ID_PATTERN.test(f)) : [];
    return {
      sidebarCompact: value.sidebarCompact === true,
      density: DENSITIES.includes(value.density) ? value.density : "comfortable",
      promptFavorites: [...new Set(favorites)].sort().slice(0, LIMITS.favorites),
    };
  }

  const byId = (a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);

  function uniqueIds(items, where) {
    const seen = new Set();
    items.forEach((item, i) => {
      if (seen.has(item.id)) fail(where + "[" + i + "].id", "id duplicado (" + item.id + ")");
      seen.add(item.id);
    });
  }

  /**
   * Estado canônico: campos conhecidos em ordem fixa, listas ordenadas por id
   * (ordem estável, diffs pequenos no Git). Lança InvalidWorkspace.
   */
  function canonical(raw) {
    if (!isPlainObject(raw)) fail("workspace", "não é um objeto");
    if (!Number.isInteger(raw.version) || raw.version < 1) fail("workspace.version", "ausente ou inválida");
    if (raw.version > SCHEMA_VERSION) fail("workspace.version", "criado por uma versão mais nova do LKR LAB (v" + raw.version + ")");
    const projects = list(raw.projects, "projects", 10000).map((p, i) => project(p, "projects[" + i + "]"));
    uniqueIds(projects, "projects");
    const projectIds = new Set(projects.map((p) => p.id));
    const prompts = list(raw.prompts, "prompts", 10000).map((p, i) => prompt(p, "prompts[" + i + "]", projectIds));
    uniqueIds(prompts, "prompts");
    const notes = list(raw.knowledge, "knowledge", 10000).map((k, i) => knowledge(k, "knowledge[" + i + "]", projectIds));
    uniqueIds(notes, "knowledge");
    const seenDdaeIds = new Set();
    const sessions = list(raw.ddae, "ddae", LIMITS.sessions).map((x, i) => session(x, "ddae[" + i + "]", projectIds, seenDdaeIds));
    if (sessions.length && raw.version < 2) fail("workspace.ddae", "DDAE exige a versão 2 do workspace");
    ddaeInvariants(sessions);
    return {
      version: SCHEMA_VERSION,
      projects: projects.sort(byId),
      prompts: prompts.sort(byId),
      knowledge: notes.sort(byId),
      preferences: preferences(raw.preferences),
      // Só aparece quando há sessões: workspaces sem DDAE mantêm a mesma forma canônica.
      ...(sessions.length ? { ddae: sessions.sort(bySession) } : {}),
    };
  }

  /**
   * Valida o arquivo versionado ({ source, module, schemaVersion, state }) ou o
   * estado puro. Nunca lança: devolve { ok, state, hash, summary } ou { ok: false, error }.
   */
  function validate(input) {
    try {
      if (!isPlainObject(input)) fail("workspace", "não é um objeto JSON");
      let raw = input;
      if ("source" in input || "module" in input) {
        if (input.source !== SOURCE || input.module !== MODULE) fail("workspace", "não é um workspace do LKR LAB");
        if (!Number.isInteger(input.schemaVersion) || input.schemaVersion < 1) fail("schemaVersion", "ausente ou inválido");
        if (input.schemaVersion > SCHEMA_VERSION) fail("schemaVersion", "criado por uma versão mais nova do LKR LAB (v" + input.schemaVersion + ")");
        raw = input.state;
      }
      const state = canonical(raw);
      return {
        ok: true,
        state,
        hash: hash(state),
        summary: { projects: state.projects.length, prompts: state.prompts.length, knowledge: state.knowledge.length, sessions: state.ddae ? state.ddae.length : 0 },
      };
    } catch (error) {
      if (error instanceof InvalidWorkspace) return { ok: false, error: error.message };
      throw error;
    }
  }

  const hash = (state) => portable().contentHash(state);

  /** Máquina nova: sem projetos, sem conhecimento e só com os prompts de fábrica. */
  function isEmpty(state) {
    return !state.projects.length && !state.knowledge.length && !(state.ddae && state.ddae.length) && state.prompts.every((p) => SEED_PROMPT_IDS.includes(p.id));
  }

  return { SCHEMA_VERSION, SOURCE, MODULE, SEED_PROMPT_IDS, LIMITS, validate, hash, isEmpty, isAbsolutePath, validRepository };
});
