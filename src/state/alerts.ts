import { useEffect, useSyncExternalStore } from "react";
import { api, desktop, errorText } from "../shared/api";
import type { AlertsSnapshot, DiagnosticsView } from "../shared/types";

/*
 * Cliente de Alerts & Diagnostics (Block 09). A avaliação é barata (regras puras sobre os snapshots
 * que os collectors já têm, cada um com o próprio TTL), então a tela consulta de 15 em 15 s com a
 * janela visível; o backend também avalia sozinho de 60 em 60 s. Enquanto um diagnóstico roda,
 * a tela acompanha o andamento de 1 em 1 s. Nada é gravado aqui: o estado é local da máquina.
 */
export interface AlertsState {
  snapshot: AlertsSnapshot | null;
  error: string | null;
  loading: boolean;
  /** Erro da última ação do usuário (reconhecer, iniciar ou cancelar diagnóstico). */
  actionError: string | null;
}

const POLL_MS = 15_000;
const RUN_POLL_MS = 1_000;

let state: AlertsState = { snapshot: null, error: null, loading: false, actionError: null };
const listeners = new Set<() => void>();
const publish = (next: AlertsState) => {
  state = next;
  listeners.forEach((listener) => listener());
};
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};

let inFlight = false;
export async function loadAlerts(): Promise<void> {
  if (!desktop || inFlight) return;
  inFlight = true;
  publish({ ...state, loading: true });
  try {
    const snapshot = await api<AlertsSnapshot>("alerts_snapshot");
    publish({ ...state, snapshot, error: null, loading: false });
  } catch (error) {
    // Mantém o último snapshot: a falha de uma leitura não apaga o que já se sabe.
    publish({ ...state, error: errorText(error), loading: false });
  } finally {
    inFlight = false;
  }
}

/** Atualiza só a parte de diagnósticos (andamento, histórico) sem reavaliar os alertas. */
async function loadDiagnostics(): Promise<void> {
  if (!desktop || !state.snapshot) return;
  try {
    const diagnostics = await api<DiagnosticsView>("diagnostics_status");
    if (state.snapshot) publish({ ...state, snapshot: { ...state.snapshot, diagnostics } });
  } catch (error) {
    publish({ ...state, error: errorText(error) });
  }
}

async function act(action: () => Promise<unknown>): Promise<void> {
  publish({ ...state, actionError: null });
  try {
    await action();
  } catch (error) {
    publish({ ...state, actionError: errorText(error) });
  }
  await loadDiagnostics();
}

/** "Reconhecer": só o ciclo de vida local do alerta; nada muda na máquina. */
export async function acknowledgeAlert(id: string): Promise<void> {
  await act(() => api("alert_acknowledge", { id }));
  await loadAlerts();
}
/** Só ids do catálogo (allowlist no backend). Exige administrador; o app nunca pede UAC. */
export const startDiagnostic = (id: string, target: string | null = null) =>
  act(() => api("diagnostic_start", { id, target }));
export const cancelDiagnostic = () => act(() => api("diagnostic_cancel"));

export interface AlertActions {
  acknowledge: (id: string) => void;
  start: (id: string, target: string | null) => void;
  cancel: () => void;
}
export const liveActions: AlertActions = {
  acknowledge: (id) => void acknowledgeAlert(id),
  start: (id, target) => void startDiagnostic(id, target),
  cancel: () => void cancelDiagnostic(),
};

/** Mantém o snapshot atualizado enquanto o componente está montado e a janela visível. */
export function useAlerts(): AlertsState {
  const current = useSyncExternalStore(subscribe, () => state, () => state);
  const running = current.snapshot?.diagnostics.current?.running === true;
  useEffect(() => {
    if (!desktop) return;
    void loadAlerts();
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") void loadAlerts();
    }, POLL_MS);
    return () => window.clearInterval(timer);
  }, []);
  useEffect(() => {
    if (!desktop || !running) return;
    const timer = window.setInterval(() => void loadDiagnostics(), RUN_POLL_MS);
    return () => window.clearInterval(timer);
  }, [running]);
  return current;
}
