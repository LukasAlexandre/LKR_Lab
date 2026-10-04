import { useSyncExternalStore } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, desktop } from "../shared/api";
import type { LogChunk, LogLine, RunInfo, RuntimeEvent } from "../shared/types";
import { workspace } from "./workspace";

/*
 * Cliente do Project Runtime Manager. Só o app desktop controla processos; o
 * estado das execuções gerenciadas chega por EVENTOS do backend (sem polling) e
 * os logs são buscados sob demanda, apenas para a execução que está na tela.
 */
export const MAX_LINES = 5000;

interface RunLogs {
  lines: LogLine[];
  nextSeq: number;
  truncated: boolean;
}
const logs = new Map<string, RunLogs>();
const pulling = new Set<string>();
const listeners = new Set<() => void>();
let version = 0;
const bump = () => {
  version += 1;
  listeners.forEach((listener) => listener());
};
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
};

/** Busca as linhas novas de uma execução (idempotente e sem concorrência por execução). */
export async function pullLogs(runId: string) {
  if (!desktop || pulling.has(runId)) return;
  pulling.add(runId);
  try {
    const current = logs.get(runId) ?? { lines: [], nextSeq: 0, truncated: false };
    const chunk = await api<LogChunk>("runtime_logs", { runId, since: current.nextSeq });
    if (chunk.lines.length || chunk.truncated !== current.truncated) {
      const lines = [...current.lines, ...chunk.lines].slice(-MAX_LINES);
      logs.set(runId, { lines, nextSeq: chunk.nextSeq, truncated: current.truncated || chunk.truncated });
      bump();
    }
  } catch {
    /* execução descartada pelo backend: o painel mostra o que já tinha */
  } finally {
    pulling.delete(runId);
  }
}

/** Cada tela com um log aberto é um "dono"; só as execuções que algum dono vê são puxadas. */
const watchers = new Map<string, string>();
const isWatched = (runId: string) => [...watchers.values()].includes(runId);
const EMPTY_LOGS: RunLogs = { lines: [], nextSeq: 0, truncated: false };
export function useRunLogs(runId: string | null): RunLogs {
  useSyncExternalStore(subscribe, () => version, () => version);
  return runId ? logs.get(runId) ?? EMPTY_LOGS : EMPTY_LOGS;
}
/** Marca a execução cujo log está aberto: só ela é atualizada pelos eventos de saída. */
export function watchRun(runId: string | null, owner = "default") {
  if (runId) {
    watchers.set(owner, runId);
    void pullLogs(runId);
  } else {
    watchers.delete(owner);
  }
}

let started = false;
/** Liga uma única vez o ouvinte de eventos do supervisor. */
export function startRuntimeEvents() {
  if (!desktop || started) return;
  started = true;
  void listen<RuntimeEvent>("runtime://event", ({ payload }) => {
      if (payload.kind === "output") {
        if (isWatched(payload.runId)) void pullLogs(payload.runId);
        return;
      }
      // Mudança de estado: o snapshot do projeto (runs, processos, portas) é refeito.
      void workspace.forProject(payload.projectId).runtime.refresh();
      void workspace.controlPlane.refresh();
      if (isWatched(payload.runId)) void pullLogs(payload.runId);
      if (payload.state === "running" || payload.state === "stopped" || payload.state === "failed" || payload.state === "completed") {
        void workspace.ports.refresh();
        void workspace.processes.refresh();
      }
  });
}

/** Executa uma ação do projeto. O backend resolve o comando; aqui só vão o id e a opção escolhida. */
export const runCommand = (id: string, commandId: string, selection?: string) =>
  api<RunInfo>("runtime_start", { id, commandId, selection: selection ?? null });
export const stopRun = (runId: string) => api<void>("runtime_stop", { runId });
export const restartRun = (id: string, runId: string) => api<RunInfo>("runtime_restart", { id, runId });
