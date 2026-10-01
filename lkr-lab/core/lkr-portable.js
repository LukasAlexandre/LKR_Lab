/*
 * LKR LAB — estado portátil × estado desta máquina
 *
 * Cada módulo tem três camadas de estado (ver docs/STATE.md):
 *
 *   cache local   localStorage do navegador: autosave instantâneo, também offline (file://)
 *   portátil      data/<módulo>.json no repositório: o que acompanha o usuário entre máquinas
 *   máquina       metadados deste navegador/computador: nunca vão para o Git
 *
 * Este arquivo decide como cache local e arquivo portátil se reconciliam, usando
 * uma "base": o hash do conteúdo portátil com o qual o cache foi reconciliado pela
 * última vez. Com a base é possível distinguir "o repositório mudou" (git pull,
 * outra máquina) de "eu mudei aqui" sem comparar datas de relógios diferentes.
 *
 * Nunca escreve no repositório: publicar continua sendo uma ação explícita (sync).
 * Sem dependência de DOM; roda no navegador e nos testes (Node).
 */
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  root.LKR = root.LKR || {};
  root.LKR.portable = Object.assign(root.LKR.portable || {}, api);
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  "use strict";

  const META_SCHEMA = 2;
  const HASH_PATTERN = /^h[0-9a-f]{16}$/;
  const SHORT_TEXT = 200;

  const isPlainObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
  const hashOrNull = (value) => (typeof value === "string" && HASH_PATTERN.test(value) ? value : null);
  const isoOrNull = (value) => (typeof value === "string" && value.length <= 40 && !Number.isNaN(Date.parse(value)) ? value : null);
  const shortText = (value) => (typeof value === "string" ? value.slice(0, SHORT_TEXT) : null);

  /**
   * Situação do cache local em relação ao arquivo portátil.
   *
   * @param {object} input
   * @param {string} input.localHash     hash do conteúdo do cache local
   * @param {boolean} input.localEmpty   o cache não tem nada produzido pelo usuário
   * @param {string|null} input.baseHash hash portátil da última reconciliação (null = nunca reconciliado)
   * @param {{status: "ok"|"missing"|"invalid", hash?: string}} input.file
   * @returns {{status: string, action: string}}
   *   status  in-sync | pending | conflict | empty | invalid
   *   action  none | mark-base | adopt
   */
  function reconcile({ localHash, localEmpty, baseHash, file }) {
    const base = hashOrNull(baseHash);
    if (!file || file.status === "invalid") return { status: "invalid", action: "none" };
    if (file.status !== "ok" || !hashOrNull(file.hash)) {
      // Sem arquivo portátil: o que existe só vive aqui até o primeiro sync.
      return localEmpty ? { status: "empty", action: "none" } : { status: "pending", action: "none" };
    }
    if (localHash === file.hash) return { status: "in-sync", action: "mark-base" };
    if (base === null) {
      // Nunca reconciliado (máquina nova ou dados legados só do navegador).
      return localEmpty ? { status: "in-sync", action: "adopt" } : { status: "conflict", action: "none" };
    }
    if (localHash === base) return { status: "in-sync", action: "adopt" }; // só o repositório mudou
    if (file.hash === base) return { status: "pending", action: "none" }; // só o cache local mudou
    return { status: "conflict", action: "none" }; // os dois mudaram
  }

  // --------------------------------------------- metadados desta máquina

  function emptyMeta() {
    return { schemaVersion: META_SCHEMA, baseHash: null, migratedAt: null, lastSuccess: null, lastAttempt: null };
  }

  function normalizeSuccess(raw) {
    if (!isPlainObject(raw)) return null;
    const at = isoOrNull(raw.at);
    if (!at) return null;
    return { at, commit: shortText(raw.commit), branch: shortText(raw.branch), repo: shortText(raw.repo) };
  }

  function normalizeAttempt(raw) {
    if (!isPlainObject(raw)) return null;
    const at = isoOrNull(raw.at);
    if (!at) return null;
    return { at, ok: raw.ok === true, code: shortText(raw.code), message: typeof raw.message === "string" ? raw.message.slice(0, 800) : null };
  }

  /**
   * Converte qualquer valor salvo em metadados v2 válidos, descartando campos
   * desconhecidos. Aceita o formato v1 ({ lastSyncedHash, lastSuccess, lastAttempt }):
   * o último hash publicado vira a base. Idempotente.
   */
  function normalizeMeta(raw) {
    const meta = emptyMeta();
    if (!isPlainObject(raw)) return meta;
    if (Number.isInteger(raw.schemaVersion) && raw.schemaVersion > META_SCHEMA) return null; // versão mais nova: não tocar
    const legacy = raw.schemaVersion === undefined;
    meta.baseHash = hashOrNull(legacy ? raw.lastSyncedHash : raw.baseHash);
    meta.migratedAt = isoOrNull(raw.migratedAt);
    meta.lastSuccess = normalizeSuccess(raw.lastSuccess);
    meta.lastAttempt = normalizeAttempt(raw.lastAttempt);
    return meta;
  }

  /**
   * Metadados de sync desta máquina sobre um adaptador de armazenamento
   * ({ get, set, remove }). Migra o formato v1 guardando antes uma cópia
   * do original em `<key>:v1`; a cópia só é escrita uma vez.
   */
  function createMetaStore(storage, key) {
    let readOnly = false;
    let meta = load();

    function load() {
      let raw = null;
      let text = null;
      try {
        text = storage.get(key);
        raw = text === null || text === undefined ? null : JSON.parse(text);
      } catch {
        raw = null; // ilegível: recomeça; o cache do checklist não depende disto
      }
      const normalized = normalizeMeta(raw);
      if (normalized === null) {
        readOnly = true; // criado por versão mais nova: usa padrão sem sobrescrever
        return emptyMeta();
      }
      if (isPlainObject(raw) && raw.schemaVersion === undefined) {
        if (storage.get(key + ":v1") === null) storage.set(key + ":v1", text);
        storage.set(key, JSON.stringify(normalized));
      }
      return normalized;
    }

    return {
      get: () => meta,
      get readOnly() {
        return readOnly;
      },
      update(patch) {
        meta = normalizeMeta(Object.assign({}, meta, patch)) || meta;
        return readOnly ? false : storage.set(key, JSON.stringify(meta));
      },
    };
  }

  /** Converte a resposta de GET /api/lab-state no formato que `reconcile` espera. */
  function fileFromResponse(res) {
    if (res && res.ok && res.data && hashOrNull(res.data.stateHash)) return { status: "ok", hash: res.data.stateHash };
    const code = res && res.error && res.error.code;
    if (code === "NO_BACKUP") return { status: "missing" };
    if (code === "INVALID_BACKUP") return { status: "invalid" };
    return null; // bridge fora do ar ou erro do Git: não há o que reconciliar agora
  }

  return { META_SCHEMA, reconcile, normalizeMeta, createMetaStore, fileFromResponse };
});
