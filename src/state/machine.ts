import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import { api, desktop, errorText } from "../shared/api";
import { MACHINE_CHECK_INTERVAL_MS } from "../shared/machine";
import type { MachineInput, MachineStatus } from "../shared/types";

/**
 * Estados da tela de cadastro:
 *  loading     primeira leitura do registro (instantânea, sem detecção)
 *  detecting   coleta passiva em andamento
 *  ready       snapshot disponível para revisão
 *  saving      "Cadastrar computador" em andamento
 *  error       falha de leitura/detecção/cadastro (mensagem em `error`)
 *  registered  ambiente liberado
 *  preview     prévia web: sem acesso nativo, nada é detectado
 */
export type MachinePhase = "loading" | "detecting" | "ready" | "saving" | "error" | "registered" | "preview";

export interface MachineRegistry {
  phase: MachinePhase;
  status: MachineStatus | null;
  error: string | null;
  /** Coleta em andamento (também depois do cadastro, em "Atualizar agora"). */
  refreshing: boolean;
  refresh(force: boolean): Promise<void>;
  register(input: MachineInput): Promise<boolean>;
}

/** Gate global + política de validade (6h no backend). Montado acima do App. */
export function useMachineRegistry(): MachineRegistry {
  const [status, setStatus] = useState<MachineStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [saving, setSaving] = useState(false);
  const inflight = useRef<Promise<void> | null>(null);

  const refresh = useCallback((force: boolean) => {
    if (!desktop) return Promise.resolve();
    // Uma coleta por vez; "Atualizar detecção" espera a que já está rodando.
    if (inflight.current) return inflight.current;
    setRefreshing(true);
    const run = api<MachineStatus>("machine_refresh", { force })
      .then((next) => {
        setStatus(next);
        setError(null);
      })
      .catch((e: unknown) => setError(errorText(e)))
      .finally(() => {
        inflight.current = null;
        setRefreshing(false);
      });
    inflight.current = run;
    return run;
  }, []);

  const register = useCallback(async (input: MachineInput) => {
    setSaving(true);
    setError(null);
    try {
      setStatus(await api<MachineStatus>("machine_register", { input }));
      return true;
    } catch (e) {
      setError(errorText(e));
      return false;
    } finally {
      setSaving(false);
    }
  }, []);

  useEffect(() => {
    if (!desktop) return;
    let alive = true;
    // Leitura instantânea decide o gate; a detecção (se expirada) vem em seguida.
    void api<MachineStatus>("machine_status")
      .then((first) => {
        if (!alive) return;
        setStatus(first);
        void refresh(false);
      })
      .catch((e: unknown) => alive && setError(errorText(e)));
    // Volta de suspensão/foco e consulta leve periódica: o backend só detecta se expirou.
    const check = () => {
      if (document.visibilityState === "visible") void refresh(false);
    };
    const timer = window.setInterval(check, MACHINE_CHECK_INTERVAL_MS);
    window.addEventListener("focus", check);
    document.addEventListener("visibilitychange", check);
    return () => {
      alive = false;
      window.clearInterval(timer);
      window.removeEventListener("focus", check);
      document.removeEventListener("visibilitychange", check);
    };
  }, [refresh]);

  const phase: MachinePhase = !desktop
    ? "preview"
    : status?.registered
      ? "registered"
      : saving
        ? "saving"
        : error
          ? "error"
          : !status
            ? "loading"
            : refreshing || !status.snapshot
              ? "detecting"
              : "ready";
  return { phase, status, error, refreshing, refresh, register };
}

export const MachineContext = createContext<MachineRegistry | null>(null);

export function useMachine(): MachineRegistry {
  const value = useContext(MachineContext);
  if (!value) throw new Error("useMachine fora do MachineGate");
  return value;
}
