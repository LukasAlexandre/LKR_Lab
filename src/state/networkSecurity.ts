import { useEffect, useSyncExternalStore } from "react";
import { api, desktop, errorText } from "../shared/api";
import type { NetworkSecuritySnapshot } from "../shared/types";

/*
 * Cliente do Network & Security (Block 08). O backend guarda um cache por domínio (rede/portas 15 s,
 * conexões 10 s, firewall/antivírus 60 s, BitLocker 5 min, Secure Boot/TPM 10 min); consultar de 15 em
 * 15 s com a janela visível só relê o que expirou. Nada é gravado aqui nem no disco: endpoints remotos
 * e conexões são estado da MÁQUINA e nunca saem dela.
 */
export interface NetworkSecurityState {
  snapshot: NetworkSecuritySnapshot | null;
  error: string | null;
  loading: boolean;
}

const POLL_MS = 15_000;

let state: NetworkSecurityState = { snapshot: null, error: null, loading: false };
const listeners = new Set<() => void>();
const publish = (next: NetworkSecurityState) => {
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
export async function loadNetworkSecurity(force = false): Promise<void> {
  if (!desktop || inFlight) return;
  inFlight = true;
  publish({ ...state, loading: true });
  try {
    const snapshot = await api<NetworkSecuritySnapshot>("network_security_snapshot", { force });
    publish({ snapshot, error: null, loading: false });
  } catch (error) {
    // Mantém o último snapshot: a falha de uma leitura não apaga o que já se sabe.
    publish({ ...state, error: errorText(error), loading: false });
  } finally {
    inFlight = false;
  }
}

export const refreshNetworkSecurity = () => loadNetworkSecurity(true);

/** Mantém o snapshot atualizado enquanto o componente está montado e a janela visível. */
export function useNetworkSecurity(): NetworkSecurityState {
  const current = useSyncExternalStore(subscribe, () => state, () => state);
  useEffect(() => {
    if (!desktop) return;
    void loadNetworkSecurity(false);
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") void loadNetworkSecurity(false);
    }, POLL_MS);
    return () => window.clearInterval(timer);
  }, []);
  return current;
}
