import { useCallback, useSyncExternalStore } from "react";
import type { KnowledgeEntry } from "./types";

/*
 * Estado de interface do desktop guardado no WebView. Única porta para esse
 * armazenamento: componentes leem e gravam por aqui, nunca por localStorage direto.
 *
 * Cada campo declara seu escopo (docs/STATE.md):
 *  - portable: pode acompanhar o usuário entre máquinas quando houver exportação portátil;
 *  - machine:  vale só nesta máquina (ids do SQLite local, filtros de portas, rascunhos).
 */
export type Density = "comfortable" | "compact";
export const PORT_FILTERS = ["projects", "expected", "unexpected", "unknown", "conflicts", "system", "all"] as const;
export type PortFilter = (typeof PORT_FILTERS)[number];
export const PORT_PROTOCOLS = ["all", "TCP", "UDP"] as const;
export type PortProtocol = (typeof PORT_PROTOCOLS)[number];
const KNOWLEDGE_KINDS: KnowledgeEntry["kind"][] = ["note", "decision", "architecture", "bug", "documentation"];

export interface Preferences {
  sidebarCompact: boolean;
  density: Density;
  promptFavorites: string[];
  activeProjectId: string;
  portsFilter: PortFilter;
  portsProtocol: PortProtocol;
  knowledgeDraft: KnowledgeEntry | null;
}

export const PREFERENCE_SCOPE: Record<keyof Preferences, "portable" | "machine"> = {
  sidebarCompact: "portable",
  density: "portable",
  promptFavorites: "portable",
  activeProjectId: "machine",
  portsFilter: "machine",
  portsProtocol: "machine",
  knowledgeDraft: "machine",
};

export const PREFERENCES_KEY = "lk.preferences";
export const PREFERENCES_SCHEMA = 1;
/** Formato anterior: uma chave por preferência, com o valor em JSON. */
export const LEGACY_KEYS: Record<keyof Preferences, string> = {
  sidebarCompact: "lk.sidebarCompact",
  density: "lk.density",
  promptFavorites: "lk.prompt.favorites",
  activeProjectId: "lk.activeProject",
  portsFilter: "lk.ports.filter",
  portsProtocol: "lk.ports.protocol",
  knowledgeDraft: "lk.knowledge.draft",
};
export const DEFAULT_PREFERENCES: Preferences = {
  sidebarCompact: false,
  density: "comfortable",
  promptFavorites: [],
  activeProjectId: "",
  portsFilter: "projects",
  portsProtocol: "all",
  knowledgeDraft: null,
};

export interface KeyValueStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);
const shortId = (value: unknown): value is string => typeof value === "string" && value.length > 0 && value.length <= 64;
const oneOf = <T extends string>(list: readonly T[], value: unknown): value is T => typeof value === "string" && (list as readonly string[]).includes(value);

function parseDraft(value: unknown): KnowledgeEntry | null {
  if (!isRecord(value)) return null;
  const text = (field: unknown, max: number) => (typeof field === "string" ? field.slice(0, max) : "");
  return {
    id: text(value.id, 64),
    projectId: shortId(value.projectId) ? value.projectId : null,
    title: text(value.title, 300),
    kind: oneOf(KNOWLEDGE_KINDS, value.kind) ? value.kind : "note",
    body: text(value.body, 200_000),
    tags: text(value.tags, 2000),
    updatedAt: text(value.updatedAt, 40),
  };
}

/** Valida campo a campo; desconhecidos e valores inválidos viram padrão. */
export function parsePreferences(raw: unknown): Preferences {
  const value = isRecord(raw) ? raw : {};
  return {
    sidebarCompact: typeof value.sidebarCompact === "boolean" ? value.sidebarCompact : DEFAULT_PREFERENCES.sidebarCompact,
    density: oneOf(["comfortable", "compact"] as const, value.density) ? value.density : DEFAULT_PREFERENCES.density,
    promptFavorites: Array.isArray(value.promptFavorites) ? [...new Set(value.promptFavorites.filter(shortId))].slice(0, 500) : [],
    activeProjectId: shortId(value.activeProjectId) ? value.activeProjectId : DEFAULT_PREFERENCES.activeProjectId,
    portsFilter: oneOf(PORT_FILTERS, value.portsFilter) ? value.portsFilter : DEFAULT_PREFERENCES.portsFilter,
    portsProtocol: oneOf(PORT_PROTOCOLS, value.portsProtocol) ? value.portsProtocol : DEFAULT_PREFERENCES.portsProtocol,
    knowledgeDraft: parseDraft(value.knowledgeDraft),
  };
}

function readJson(storage: KeyValueStorage, key: string): unknown {
  try {
    const text = storage.getItem(key);
    return text === null ? undefined : JSON.parse(text);
  } catch {
    return undefined;
  }
}

function writePreferences(storage: KeyValueStorage, preferences: Preferences) {
  try {
    storage.setItem(PREFERENCES_KEY, JSON.stringify({ schemaVersion: PREFERENCES_SCHEMA, ...preferences }));
    return true;
  } catch {
    return false;
  }
}

/**
 * Carrega as preferências. Sem a chave atual, converte as chaves legadas e só
 * remove as antigas depois de confirmar que a nova foi gravada e relida igual.
 * Rodar de novo não muda nada. Chave criada por versão mais nova: usa os
 * valores reconhecidos, mas nunca a sobrescreve.
 */
export function loadPreferences(storage: KeyValueStorage): { preferences: Preferences; writable: boolean; migrated: boolean } {
  const current = readJson(storage, PREFERENCES_KEY);
  if (isRecord(current)) {
    const version = current.schemaVersion;
    return { preferences: parsePreferences(current), writable: !(typeof version === "number" && version > PREFERENCES_SCHEMA), migrated: false };
  }
  const legacy: Record<string, unknown> = {};
  for (const [field, key] of Object.entries(LEGACY_KEYS)) {
    const value = readJson(storage, key);
    if (value !== undefined) legacy[field] = value;
  }
  const preferences = parsePreferences(legacy);
  if (!Object.keys(legacy).length || !writePreferences(storage, preferences)) return { preferences, writable: true, migrated: false };
  if (JSON.stringify(parsePreferences(readJson(storage, PREFERENCES_KEY))) !== JSON.stringify(preferences)) {
    return { preferences, writable: true, migrated: false };
  }
  for (const key of Object.values(LEGACY_KEYS)) {
    try { storage.removeItem(key); } catch { /* a chave antiga fica; a nova já vale */ }
  }
  return { preferences, writable: true, migrated: true };
}

export function createPreferenceStore(storage: KeyValueStorage) {
  const loaded = loadPreferences(storage);
  let preferences = loaded.preferences;
  const listeners = new Set<() => void>();
  return {
    get: () => preferences,
    set(patch: Partial<Preferences>) {
      const next = parsePreferences({ ...preferences, ...patch });
      if (JSON.stringify(next) === JSON.stringify(preferences)) return;
      preferences = next;
      // Falha de gravação não pode quebrar a navegação: o valor vale na sessão.
      if (loaded.writable) writePreferences(storage, preferences);
      listeners.forEach((listener) => listener());
    },
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
  };
}

export function memoryStorage(): KeyValueStorage {
  const data = new Map<string, string>();
  return { getItem: (key) => data.get(key) ?? null, setItem: (key, value) => { data.set(key, value); }, removeItem: (key) => { data.delete(key); } };
}

function browserStorage(): KeyValueStorage {
  try {
    const storage = window.localStorage;
    storage.getItem(PREFERENCES_KEY);
    return storage;
  } catch {
    return memoryStorage();
  }
}

export const preferences = createPreferenceStore(typeof window === "undefined" ? memoryStorage() : browserStorage());

export function usePreference<K extends keyof Preferences>(key: K) {
  const value = useSyncExternalStore(preferences.subscribe, () => preferences.get()[key]);
  const setValue = useCallback((next: Preferences[K] | ((current: Preferences[K]) => Preferences[K])) => {
    const resolved = typeof next === "function" ? (next as (current: Preferences[K]) => Preferences[K])(preferences.get()[key]) : next;
    preferences.set({ [key]: resolved } as Partial<Preferences>);
  }, [key]);
  return [value, setValue] as const;
}
