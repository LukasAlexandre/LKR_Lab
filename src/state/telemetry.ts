import { useEffect, useSyncExternalStore } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, desktop } from "../shared/api";
import type { Telemetry, TelemetryPoint, TelemetryState } from "../shared/types";

/*
 * Cliente da Machine Telemetry. O backend tem UM sampler; aqui há UM listener do evento
 * `machine://telemetry` para o app inteiro (criado na primeira inscrição, nunca por
 * render). O histórico é o buffer curto do backend, mantido do mesmo tamanho aqui.
 */
export const TELEMETRY_EVENT = "machine://telemetry";
const HISTORY_LEN = 120;
const WATCH_RENEW_MS = 5_000;

let snapshot: TelemetryState = { latest: null, history: [] };
const listeners = new Set<() => void>();
let started = false;

const publish = (next: TelemetryState) => {
  snapshot = next;
  listeners.forEach((listener) => listener());
};

/** Junta uma amostra ao histórico: só amostras com carga nova entram (o backend faz o mesmo). */
export function appendPoint(history: TelemetryPoint[], telemetry: Telemetry): TelemetryPoint[] {
  const last = history[history.length - 1];
  if (last && last.at >= telemetry.timestamp) return history;
  const gpus = telemetry.gpus.map((g) => g.usage).filter((u): u is number => u != null);
  const point: TelemetryPoint = {
    at: telemetry.timestamp,
    cpu: telemetry.cpu.usage,
    memory: telemetry.memory.percent,
    disk: telemetry.diskIo.activity,
    gpu: gpus.length ? Math.max(...gpus) : null,
    downloadBps: telemetry.network.downloadBps,
    uploadBps: telemetry.network.uploadBps,
  };
  return [...history, point].slice(-HISTORY_LEN);
}

function start() {
  if (started || !desktop) return;
  started = true;
  void listen<Telemetry>(TELEMETRY_EVENT, (event) => {
    publish({ latest: event.payload, history: appendPoint(snapshot.history, event.payload) });
  });
  void api<TelemetryState>("machine_telemetry")
    .then((initial) => {
      // Um evento pode ter chegado antes: fica o mais novo.
      if (!snapshot.latest || (initial.latest && initial.latest.timestamp >= snapshot.latest.timestamp))
        publish({ latest: initial.latest, history: initial.history.slice(-HISTORY_LEN) });
    })
    .catch(() => undefined);
}

const subscribe = (listener: () => void) => {
  start();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};

export function useTelemetry(): TelemetryState {
  return useSyncExternalStore(subscribe, () => snapshot);
}

/**
 * Enquanto o componente estiver montado e a janela visível, mantém o sampler no modo
 * ativo (lease renovado a cada 5 s; o backend expira sozinho em 15 s). Sem limpeza
 * manual: recarregar a página não deixa o sampler preso no modo ativo.
 */
export function useTelemetryWatch() {
  useEffect(() => {
    if (!desktop) return;
    const renew = () => {
      if (document.visibilityState === "visible") void api("machine_telemetry_watch").catch(() => undefined);
    };
    renew();
    const timer = window.setInterval(renew, WATCH_RENEW_MS);
    document.addEventListener("visibilitychange", renew);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", renew);
    };
  }, []);
}

export const refreshTelemetry = () => api("machine_telemetry_refresh");
