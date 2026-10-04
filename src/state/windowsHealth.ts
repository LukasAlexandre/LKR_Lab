import { useEffect, useSyncExternalStore } from "react";
import { api, desktop, errorText } from "../shared/api";
import type { WindowsHealthSnapshot } from "../shared/types";

/*
 * Cliente do Windows Health (Block 07). O backend já guarda um cache por domínio com TTL próprio
 * (reinício/serviços 45 s, eventos 3 min, atualização/dispositivos 10 min), então consultar de 30 em
 * 30 s com a janela visível é barato: só o que expirou é relido. Nada é gravado aqui nem no disco.
 */
export interface WindowsHealthState {
  snapshot: WindowsHealthSnapshot | null;
  error: string | null;
  loading: boolean;
}

const POLL_MS = 30_000;

let state: WindowsHealthState = { snapshot: null, error: null, loading: false };
const listeners = new Set<() => void>();
const publish = (next: WindowsHealthState) => {
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
/** `force` ("Atualizar agora") relê todos os domínios; sem ele, só os que expiraram. */
export async function loadWindowsHealth(force = false): Promise<void> {
  if (!desktop || inFlight) return;
  inFlight = true;
  publish({ ...state, loading: true });
  try {
    const snapshot = await api<WindowsHealthSnapshot>("windows_health_snapshot", { force });
    publish({ snapshot, error: null, loading: false });
  } catch (error) {
    // Mantém o último snapshot: a falha de uma leitura não apaga o que já se sabe.
    publish({ ...state, error: errorText(error), loading: false });
  } finally {
    inFlight = false;
  }
}

export const refreshWindowsHealth = () => loadWindowsHealth(true);

/** Mantém o snapshot atualizado enquanto o componente está montado e a janela visível. */
export function useWindowsHealth(): WindowsHealthState {
  const current = useSyncExternalStore(subscribe, () => state, () => state);
  useEffect(() => {
    if (!desktop) return;
    void loadWindowsHealth(false);
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") void loadWindowsHealth(false);
    }, POLL_MS);
    return () => window.clearInterval(timer);
  }, []);
  return current;
}
