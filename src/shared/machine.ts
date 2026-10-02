import type { MachineSnapshot, MachineUsage } from "./types";

/** Limites espelhados de hub-core::machine (o backend valida de novo). */
export const MACHINE_NAME_MAX = 60;
export const MACHINE_DESCRIPTION_MAX = 120;
/** Consulta leve com o app aberto: o backend só detecta se o snapshot tiver 6h ou mais. */
export const MACHINE_CHECK_INTERVAL_MS = 15 * 60 * 1000;

export const USAGE_OPTIONS: { value: MachineUsage; label: string }[] = [
  { value: "home", label: "Casa" },
  { value: "work", label: "Trabalho" },
  { value: "other", label: "Outro" },
];

export const usageLabel = (usage: string) =>
  USAGE_OPTIONS.find((option) => option.value === usage)?.label ?? "Outro";

const GB = 1024 ** 3;
export function formatBytes(bytes: number | null | undefined): string | null {
  if (!bytes || bytes <= 0) return null;
  if (bytes >= GB) {
    const value = bytes / GB;
    return `${value.toLocaleString("pt-BR", { maximumFractionDigits: value >= 10 ? 0 : 1 })} GB`;
  }
  return `${Math.round(bytes / 1024 ** 2).toLocaleString("pt-BR")} MB`;
}

/** RAM instalada: o sistema informa um pouco menos que o módulo (reserva de hardware). */
export function formatMemory(bytes: number | null | undefined): string | null {
  if (!bytes || bytes <= 0) return null;
  return bytes >= 4 * GB ? `${Math.round(bytes / GB)} GB` : formatBytes(bytes);
}

export function osDetail(snapshot: MachineSnapshot): string | null {
  const { osVersion, osBuild } = snapshot;
  if (osVersion && osBuild) return `Versão ${osVersion} (Build ${osBuild})`;
  if (osVersion) return `Versão ${osVersion}`;
  return osBuild ? `Build ${osBuild}` : null;
}

export function cpuDetail(snapshot: MachineSnapshot): string | null {
  const { cpuCores, cpuThreads } = snapshot;
  const parts = [
    cpuCores ? `${cpuCores} ${cpuCores === 1 ? "núcleo" : "núcleos"}` : null,
    cpuThreads ? `${cpuThreads} threads` : null,
  ].filter(Boolean);
  return parts.length ? parts.join(" / ") : null;
}

export function detectionAge(detectedAt: number, now: number): string {
  const minutes = Math.floor((now - detectedAt) / 60_000);
  if (minutes < 1) return "Agora";
  if (minutes < 60) return `Há ${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `Há ${hours} h`;
  const days = Math.floor(hours / 24);
  return `Há ${days} ${days === 1 ? "dia" : "dias"}`;
}

export const formatDateTime = (ms: number) =>
  new Date(ms).toLocaleString("pt-BR", { dateStyle: "medium", timeStyle: "short" });

/** Nome sugerido: o hostname, se houver. O nome amigável é sempre confirmado pelo usuário. */
export const suggestedName = (snapshot: MachineSnapshot | null) =>
  (snapshot?.hostname ?? "").slice(0, MACHINE_NAME_MAX);

export function validateMachineName(name: string): string | null {
  const value = name.trim();
  if (!value) return "Informe um nome para este computador.";
  if (value.length > MACHINE_NAME_MAX) return `Use até ${MACHINE_NAME_MAX} caracteres.`;
  return null;
}
