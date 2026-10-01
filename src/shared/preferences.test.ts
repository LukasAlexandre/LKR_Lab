import { describe, expect, it } from "vitest";
import {
  createPreferenceStore,
  DEFAULT_PREFERENCES,
  LEGACY_KEYS,
  loadPreferences,
  memoryStorage,
  parsePreferences,
  PREFERENCE_SCOPE,
  PREFERENCES_KEY,
  type KeyValueStorage,
} from "./preferences";

function storageWith(entries: Record<string, string>) {
  const storage = memoryStorage();
  for (const [key, value] of Object.entries(entries)) storage.setItem(key, value);
  return storage;
}

const keys = (storage: KeyValueStorage) => Object.values(LEGACY_KEYS).concat(PREFERENCES_KEY).filter((key) => storage.getItem(key) !== null);

describe("preferências do desktop", () => {
  it("sem dados usa os padrões e não grava nada", () => {
    const storage = memoryStorage();
    expect(loadPreferences(storage)).toEqual({ preferences: DEFAULT_PREFERENCES, writable: true, migrated: false });
    expect(keys(storage)).toEqual([]);
  });

  it("grava em uma chave versionada e relê", () => {
    const storage = memoryStorage();
    const store = createPreferenceStore(storage);
    store.set({ density: "compact", promptFavorites: ["audit", "audit", "bug"] });
    expect(JSON.parse(storage.getItem(PREFERENCES_KEY) ?? "")).toMatchObject({ schemaVersion: 1, density: "compact", promptFavorites: ["audit", "bug"] });
    expect(createPreferenceStore(storage).get().density).toBe("compact");
  });

  it("valores inválidos e campos desconhecidos viram padrão", () => {
    const parsed = parsePreferences({ density: "huge", sidebarCompact: "yes", portsFilter: "../x", promptFavorites: [1, "", "ok"], token: "ghp_x", knowledgeDraft: { kind: "hack", title: 5 } });
    expect(parsed).toMatchObject({ density: "comfortable", sidebarCompact: false, portsFilter: "projects", promptFavorites: ["ok"] });
    expect(parsed.knowledgeDraft).toMatchObject({ kind: "note", title: "" });
    expect(parsed).not.toHaveProperty("token");
    expect(parsePreferences("lixo")).toEqual(DEFAULT_PREFERENCES);
  });

  it("migra as chaves legadas, remove-as só após confirmar e é idempotente", () => {
    const storage = storageWith({
      "lk.sidebarCompact": "true",
      "lk.density": '"compact"',
      "lk.activeProject": '"7f1c"',
      "lk.ports.protocol": '"UDP"',
      "lk.prompt.favorites": '["audit"]',
      "lk.unrelated": "fica",
    });
    const first = loadPreferences(storage);
    expect(first.migrated).toBe(true);
    expect(first.preferences).toMatchObject({ sidebarCompact: true, density: "compact", activeProjectId: "7f1c", portsProtocol: "UDP", promptFavorites: ["audit"] });
    expect(keys(storage)).toEqual([PREFERENCES_KEY]);
    expect(storage.getItem("lk.unrelated")).toBe("fica");

    const stored = storage.getItem(PREFERENCES_KEY);
    const second = loadPreferences(storage);
    expect(second.migrated).toBe(false);
    expect(second.preferences).toEqual(first.preferences);
    expect(storage.getItem(PREFERENCES_KEY)).toBe(stored);
  });

  it("falha ao gravar a nova chave preserva as legadas", () => {
    const base = storageWith({ "lk.density": '"compact"' });
    const failing: KeyValueStorage = { getItem: base.getItem, removeItem: base.removeItem, setItem: () => { throw new Error("cota"); } };
    const result = loadPreferences(failing);
    expect(result).toMatchObject({ migrated: false, preferences: { density: "compact" } });
    expect(base.getItem("lk.density")).toBe('"compact"');
  });

  it("legado corrompido é ignorado campo a campo", () => {
    const storage = storageWith({ "lk.density": "{oops", "lk.sidebarCompact": "true" });
    expect(loadPreferences(storage).preferences).toMatchObject({ density: "comfortable", sidebarCompact: true });
  });

  it("chave criada por versão mais nova não é sobrescrita", () => {
    const future = JSON.stringify({ schemaVersion: 9, density: "compact", novo: 1 });
    const storage = storageWith({ [PREFERENCES_KEY]: future });
    const store = createPreferenceStore(storage);
    expect(store.get().density).toBe("compact");
    store.set({ density: "comfortable" });
    expect(store.get().density).toBe("comfortable");
    expect(storage.getItem(PREFERENCES_KEY)).toBe(future);
  });

  it("todo campo tem escopo declarado; ids locais e rascunhos não são portáteis", () => {
    expect(Object.keys(PREFERENCE_SCOPE).sort()).toEqual(Object.keys(DEFAULT_PREFERENCES).sort());
    expect(PREFERENCE_SCOPE.activeProjectId).toBe("machine");
    expect(PREFERENCE_SCOPE.knowledgeDraft).toBe("machine");
  });

  it("rascunho válido volta idêntico, na mesma ordem de chaves (comparação de 'não salvo')", () => {
    const entry = { id: "k1", projectId: null, title: "ADR", kind: "decision" as const, body: "texto", tags: "a, b", updatedAt: "2026-09-30T12:00:00Z" };
    const store = createPreferenceStore(memoryStorage());
    store.set({ knowledgeDraft: entry });
    expect(JSON.stringify(store.get().knowledgeDraft)).toBe(JSON.stringify(entry));
  });

  it("avisa assinantes só quando o valor muda", () => {
    const store = createPreferenceStore(memoryStorage());
    let calls = 0;
    const off = store.subscribe(() => { calls += 1; });
    store.set({ sidebarCompact: true });
    store.set({ sidebarCompact: true });
    off();
    store.set({ sidebarCompact: false });
    expect(calls).toBe(1);
  });
});
