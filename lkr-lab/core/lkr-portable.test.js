import { describe, expect, it } from "vitest";
import "./lkr-portable.js";
import "../lab-setup/catalog.js";
import "../lab-setup/store.js";

const { reconcile, normalizeMeta, createMetaStore, fileFromResponse, META_SCHEMA } = globalThis.LKR.portable;
const { catalog, createStore, persistableState, stateHash, STORAGE_KEY } = globalThis.LKR.labSetup;

const KEY = "lkr-lab-setup-v1:sync";
const H = (n) => "h" + String(n).repeat(16).slice(0, 16);

function memoryStorage(initial) {
  const data = new Map(Object.entries(initial || {}));
  return {
    data,
    get: (key) => (data.has(key) ? data.get(key) : null),
    set: (key, value) => {
      data.set(key, value);
      return true;
    },
    remove: (key) => data.delete(key),
  };
}

/** Envelope igual ao que o bridge grava em data/lab-setup.json. */
function envelopeOf(store) {
  const state = persistableState(store.state);
  return { schemaVersion: 1, source: "lkr-lab", module: "lab-setup", updatedAt: "2026-09-30T18:00:00-03:00", stateHash: stateHash(state), state };
}

describe("reconcile: cache local × arquivo portátil", () => {
  const ok = (hash) => ({ status: "ok", hash });

  it("arquivo ausente: vazio não tem o que sincronizar; com dados fica pendente", () => {
    expect(reconcile({ localHash: H(1), localEmpty: true, baseHash: null, file: { status: "missing" } })).toEqual({ status: "empty", action: "none" });
    expect(reconcile({ localHash: H(1), localEmpty: false, baseHash: null, file: { status: "missing" } })).toEqual({ status: "pending", action: "none" });
  });

  it("arquivo inválido nunca é sobrescrito nem adotado", () => {
    expect(reconcile({ localHash: H(1), localEmpty: false, baseHash: H(2), file: { status: "invalid" } })).toEqual({ status: "invalid", action: "none" });
    expect(reconcile({ localHash: H(1), localEmpty: false, baseHash: H(2), file: null }).action).toBe("none");
    expect(reconcile({ localHash: H(1), localEmpty: false, baseHash: H(2), file: { status: "ok", hash: "não-é-hash" } }).action).toBe("none");
  });

  it("mesmo conteúdo: só registra a base", () => {
    expect(reconcile({ localHash: H(3), localEmpty: false, baseHash: null, file: ok(H(3)) })).toEqual({ status: "in-sync", action: "mark-base" });
  });

  it("máquina nova (cache vazio, sem base) adota o arquivo", () => {
    expect(reconcile({ localHash: H(1), localEmpty: true, baseHash: null, file: ok(H(2)) })).toEqual({ status: "in-sync", action: "adopt" });
  });

  it("dados legados do navegador diferentes do arquivo: conflito, nada é trocado", () => {
    expect(reconcile({ localHash: H(1), localEmpty: false, baseHash: null, file: ok(H(2)) })).toEqual({ status: "conflict", action: "none" });
  });

  it("só o repositório mudou (git pull): adota", () => {
    expect(reconcile({ localHash: H(1), localEmpty: false, baseHash: H(1), file: ok(H(2)) })).toEqual({ status: "in-sync", action: "adopt" });
  });

  it("só o navegador mudou: pendente de sync, inclusive depois de um reset", () => {
    expect(reconcile({ localHash: H(4), localEmpty: false, baseHash: H(1), file: ok(H(1)) })).toEqual({ status: "pending", action: "none" });
    expect(reconcile({ localHash: H(5), localEmpty: true, baseHash: H(1), file: ok(H(1)) })).toEqual({ status: "pending", action: "none" });
  });

  it("os dois mudaram: conflito", () => {
    expect(reconcile({ localHash: H(4), localEmpty: false, baseHash: H(1), file: ok(H(2)) })).toEqual({ status: "conflict", action: "none" });
  });

  it("base corrompida é tratada como ausente", () => {
    expect(reconcile({ localHash: H(1), localEmpty: false, baseHash: "lixo", file: ok(H(2)) }).status).toBe("conflict");
  });
});

describe("metadados desta máquina", () => {
  it("padrões seguros sem dados", () => {
    expect(normalizeMeta(undefined)).toEqual({ schemaVersion: META_SCHEMA, baseHash: null, migratedAt: null, lastSuccess: null, lastAttempt: null });
  });

  it("migra o formato v1 preservando o original e é idempotente", () => {
    const legacy = { lastSyncedHash: H(7), lastSuccess: { at: "2026-09-29T10:00:00.000Z", commit: "abc1234", branch: "main", repo: "LukasAlexandre/LKR_Lab" }, lastAttempt: { at: "2026-09-29T10:00:00.000Z", ok: true, code: "PUSHED", message: "ok" } };
    const raw = JSON.stringify(legacy);
    const storage = memoryStorage({ [KEY]: raw });

    const first = createMetaStore(storage, KEY).get();
    expect(first.baseHash).toBe(H(7));
    expect(first.lastSuccess.commit).toBe("abc1234");
    expect(storage.get(KEY + ":v1")).toBe(raw);
    const migrated = storage.get(KEY);

    const second = createMetaStore(storage, KEY).get();
    expect(second).toEqual(first);
    expect(storage.get(KEY)).toBe(migrated);
    expect(storage.get(KEY + ":v1")).toBe(raw);
    expect(normalizeMeta(normalizeMeta(legacy))).toEqual(normalizeMeta(legacy));
  });

  it("descarta campos desconhecidos e valores inválidos", () => {
    const meta = normalizeMeta({ schemaVersion: 2, baseHash: "../../etc", token: "ghp_x", lastSuccess: { at: "ontem", commit: "x" }, lastAttempt: { at: "2026-09-29T10:00:00Z", ok: "sim", message: 42 } });
    expect(meta).toEqual({ schemaVersion: 2, baseHash: null, migratedAt: null, lastSuccess: null, lastAttempt: { at: "2026-09-29T10:00:00Z", ok: false, code: null, message: null } });
    expect(meta).not.toHaveProperty("token");
  });

  it("JSON ilegível recomeça sem quebrar", () => {
    const storage = memoryStorage({ [KEY]: "{oops" });
    const store = createMetaStore(storage, KEY);
    expect(store.get().baseHash).toBeNull();
    expect(store.update({ baseHash: H(1) })).toBe(true);
    expect(JSON.parse(storage.get(KEY)).baseHash).toBe(H(1));
  });

  it("versão mais nova é preservada: lê padrões e não sobrescreve", () => {
    const future = JSON.stringify({ schemaVersion: 99, baseHash: H(9), novoCampo: true });
    const storage = memoryStorage({ [KEY]: future });
    const store = createMetaStore(storage, KEY);
    expect(store.readOnly).toBe(true);
    expect(store.get().baseHash).toBeNull();
    expect(store.update({ baseHash: H(1) })).toBe(false);
    expect(storage.get(KEY)).toBe(future);
  });
});

describe("resposta do bridge", () => {
  it("traduz arquivo presente, ausente, inválido e indisponível", () => {
    expect(fileFromResponse({ ok: true, data: { stateHash: H(1) } })).toEqual({ status: "ok", hash: H(1) });
    expect(fileFromResponse({ ok: false, error: { code: "NO_BACKUP" } })).toEqual({ status: "missing" });
    expect(fileFromResponse({ ok: false, error: { code: "INVALID_BACKUP" } })).toEqual({ status: "invalid" });
    expect(fileFromResponse({ ok: false, offline: true, error: { code: "OFFLINE" } })).toBeNull();
    expect(fileFromResponse({ ok: false, error: { code: "DETACHED" } })).toBeNull();
  });
});

describe("fluxo de duas máquinas (sem Git)", () => {
  it("PC casa publica, PC trabalho restaura; filtros de cada máquina ficam onde estão", () => {
    const casa = createStore({ catalog, storage: memoryStorage() });
    casa.setCompleted("trena", true);
    casa.setNotes("multimetro", "Comprar depois do pagamento.");
    casa.setUi({ status: "done", category: "eletronica" });
    const file = envelopeOf(casa);
    expect(file.state).not.toHaveProperty("ui");

    const trabalhoStorage = memoryStorage();
    const trabalho = createStore({ catalog, storage: trabalhoStorage });
    trabalho.setUi({ status: "pending", category: "bancada" });
    const meta = createMetaStore(trabalhoStorage, KEY);
    const decision = reconcile({ localHash: trabalho.contentHash(), localEmpty: trabalho.isEmpty(), baseHash: meta.get().baseHash, file: { status: "ok", hash: file.stateHash } });
    expect(decision.action).toBe("adopt");

    expect(trabalho.importData(file, { reason: "repo" }).ok).toBe(true);
    meta.update({ baseHash: file.stateHash });
    expect(trabalho.contentHash()).toBe(file.stateHash);
    expect(trabalho.getItem("trena").completed).toBe(true);
    expect(trabalho.getItem("multimetro").notes).toBe("Comprar depois do pagamento.");
    // Estado desta máquina não é sobrescrito pelo portátil.
    expect(trabalho.state.ui).toEqual({ status: "pending", category: "bancada" });
    expect(JSON.parse(trabalhoStorage.get(STORAGE_KEY)).ui).toEqual({ status: "pending", category: "bancada" });

    // Reabrir: mesma decisão vira apenas "em dia" (idempotente).
    const again = reconcile({ localHash: trabalho.contentHash(), localEmpty: trabalho.isEmpty(), baseHash: meta.get().baseHash, file: { status: "ok", hash: file.stateHash } });
    expect(again).toEqual({ status: "in-sync", action: "mark-base" });

    // Edição no trabalho: pendente, não adota nada por cima.
    trabalho.setNotes("trena", "Emprestei.");
    expect(reconcile({ localHash: trabalho.contentHash(), localEmpty: false, baseHash: meta.get().baseHash, file: { status: "ok", hash: file.stateHash } }).status).toBe("pending");
  });

  it("dados desconhecidos e schema futuro no arquivo portátil não entram no estado", () => {
    const store = createStore({ catalog, storage: memoryStorage() });
    const strange = { schemaVersion: 1, source: "lkr-lab", module: "lab-setup", extra: { path: "C:\\Users\\x" }, state: { version: 1, items: { trena: { completed: true, pid: 42 }, "../x": { completed: true } }, custom: [], machine: { path: "C:\\x" } } };
    expect(store.importData(strange, { reason: "repo" }).ok).toBe(true);
    expect(store.state.items).toEqual({ trena: { completed: true, completedAt: null, notes: "", updatedAt: null } });
    expect(store.state).not.toHaveProperty("machine");
    expect(store.previewImport({ ...strange, schemaVersion: 2 }).ok).toBe(false);
    expect(store.previewImport({ ...strange, state: { ...strange.state, version: 9 } }).ok).toBe(false);
  });
});
