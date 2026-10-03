import { useSyncExternalStore } from "react";
import { api, desktop, errorText } from "../shared/api";
import { PREFERENCE_SCOPE, preferences, type Preferences } from "../shared/preferences";
import { workspace } from "./workspace";

/*
 * Cliente do sync do workspace. É a única fronteira entre a interface e o
 * sync: a UI chama só `run`/`check*` daqui; quem fala com o bridge e com o Git
 * é o backend Rust. Nada acontece em segundo plano além de consultas leves:
 * estado local (sem rede) a cada edição, remoto na abertura, ao voltar à
 * janela (no máximo a cada 5 min) e depois de um sync. Nunca há push automático.
 */
export type SyncStateName = "clean" | "local_dirty" | "remote_changed" | "diverged" | "offline" | "error";

export interface SyncStatus {
  state: SyncStateName;
  message: string;
  code: string | null;
  localHash: string;
  remoteHash: string | null;
  baseHash: string | null;
  lastSyncedAt: string | null;
  checkedRemote: boolean;
  pushed: boolean;
  applied: boolean;
  preferences: PortablePreferences | null;
}
export interface PortablePreferences {
  sidebarCompact: boolean;
  density: Preferences["density"];
  promptFavorites: string[];
}
export interface SyncView {
  status: SyncStatus | null;
  busy: boolean;
  /** Mensagem curta do último sync iniciado pelo usuário. */
  notice: string | null;
}

const REMOTE_CHECK_MIN_MS = 5 * 60_000;
const LOCAL_DEBOUNCE_MS = 1500;

let view: SyncView = { status: null, busy: false, notice: null };
const listeners = new Set<() => void>();
const publish = (patch: Partial<SyncView>) => {
  view = { ...view, ...patch };
  listeners.forEach((listener) => listener());
};
export const syncStore = {
  get: () => view,
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => { listeners.delete(listener); };
  },
};
export const useSyncView = () => useSyncExternalStore(syncStore.subscribe, syncStore.get);

/** Só os campos que o próprio app marca como portáteis (PREFERENCE_SCOPE). */
export function portablePreferences(): PortablePreferences {
  const current = preferences.get();
  const pick = <K extends keyof Preferences>(key: K) => (PREFERENCE_SCOPE[key] === "portable" ? current[key] : undefined);
  return {
    sidebarCompact: pick("sidebarCompact") as boolean,
    density: pick("density") as Preferences["density"],
    promptFavorites: pick("promptFavorites") as string[],
  };
}

const failed = (): SyncStatus => ({
  state: "error",
  message: "Não foi possível consultar o sync.",
  code: "UI",
  localHash: "",
  remoteHash: null,
  baseHash: null,
  lastSyncedAt: null,
  checkedRemote: false,
  pushed: false,
  applied: false,
  preferences: null,
});

let remoteCheckedAt = 0;
let checking = false;

/** Instantâneo, sem rede: compara o workspace local com a última base sincronizada. */
export async function checkLocal() {
  if (!desktop || view.busy) return;
  try {
    const local = await api<SyncStatus>("sync_status", { preferences: portablePreferences(), checkRemote: false });
    // Não apaga um aviso de remoto/offline mais específico, só atualiza o que é local.
    const keep = view.status?.checkedRemote && view.status.localHash === local.localHash ? view.status : local;
    publish({ status: keep });
  } catch (error) {
    publish({ status: { ...failed(), message: errorText(error) } });
  }
}

/** Consulta o repositório via bridge. Não bloqueia nada: roda em segundo plano e tolera falhas. */
export async function checkRemote(force = false) {
  if (!desktop || view.busy || checking) return;
  if (!force && Date.now() - remoteCheckedAt < REMOTE_CHECK_MIN_MS) return;
  checking = true;
  remoteCheckedAt = Date.now();
  try {
    publish({ status: await api<SyncStatus>("sync_status", { preferences: portablePreferences(), checkRemote: true }) });
  } catch (error) {
    publish({ status: { ...failed(), message: errorText(error) } });
  } finally {
    checking = false;
  }
}

async function reloadAfterApply(status: SyncStatus) {
  if (status.preferences) {
    const next = status.preferences;
    preferences.set({ sidebarCompact: next.sidebarCompact, density: next.density, promptFavorites: next.promptFavorites });
  }
  await Promise.all([workspace.loadRegistry(), workspace.knowledge.refresh(), workspace.refreshDdae()]);
  void workspace.refreshRepositories(0);
}

/** Ação "Sincronizar". `resolution` só vem de uma escolha explícita do usuário numa divergência. */
export async function run(resolution?: "local" | "remote") {
  if (!desktop || view.busy) return;
  publish({ busy: true, notice: "Sincronizando…" });
  try {
    const status = await api<SyncStatus>("sync_run", { preferences: portablePreferences(), resolution: resolution ?? null });
    remoteCheckedAt = Date.now();
    if (status.applied) await reloadAfterApply(status);
    const notice =
      status.state === "clean" ? "Sincronizado agora"
      : status.state === "diverged" ? "Conflito detectado"
      : status.state === "offline" ? "Sem conexão"
      : "Erro ao sincronizar";
    publish({ status, busy: false, notice });
  } catch (error) {
    publish({ busy: false, notice: "Erro ao sincronizar", status: { ...failed(), message: errorText(error) } });
  }
}

let timer: ReturnType<typeof setTimeout> | undefined;
const scheduleLocal = () => {
  clearTimeout(timer);
  timer = setTimeout(() => void checkLocal(), LOCAL_DEBOUNCE_MS);
};

/** O DDAE faz parte do workspace portátil: depois de mudá-lo, reavalia o estado local (sem rede). */
export const notifyLocalChange = scheduleLocal;

let started = false;
/** Liga o cliente uma única vez. Retorna a função de limpeza. */
export function start() {
  if (!desktop || started) return () => {};
  started = true;
  const unsubscribe = [
    preferences.subscribe(scheduleLocal),
    workspace.projects.subscribe(scheduleLocal),
    workspace.prompts.subscribe(scheduleLocal),
    workspace.knowledge.subscribe(scheduleLocal),
  ];
  const onVisible = () => {
    if (document.visibilityState === "visible") void checkRemote();
  };
  document.addEventListener("visibilitychange", onVisible);
  // Abertura rápida: o estado local aparece já; o remoto chega depois, sem bloquear.
  void checkLocal().then(() => checkRemote(true));
  return () => {
    started = false;
    clearTimeout(timer);
    unsubscribe.forEach((off) => off());
    document.removeEventListener("visibilitychange", onVisible);
  };
}
