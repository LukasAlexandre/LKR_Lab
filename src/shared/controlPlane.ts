import type { Association, Confidence, ControlPlaneSnapshot, RuntimeObservation } from "./types";

/*
 * Leitura do contrato do Control Plane para a tela. Nada aqui inventa relação: sem evidência o
 * valor é "—" e o Project ativo NUNCA entra como palpite.
 */
export const NO_VALUE = "—";

export const CONFIDENCE_LABEL: Record<Confidence, string> = {
  exact: "Exata",
  high: "Alta",
  medium: "Média",
  unknown: "Desconhecida",
};

export interface RuntimeGroups {
  /** Iniciados pelo LKR LAB para este Project. */
  managed: RuntimeObservation[];
  /** Descobertos na máquina e atribuídos a este Project com evidência. */
  detected: RuntimeObservation[];
  /** Descobertos sem relação com este Project (outros Projects ou sem atribuição), sem os do sistema. */
  elsewhere: RuntimeObservation[];
  /** Serviços do Windows ocultos por padrão. */
  system: number;
}

export function groupRuntimes(snapshot: ControlPlaneSnapshot | null, projectId: string): RuntimeGroups {
  const groups: RuntimeGroups = { managed: [], detected: [], elsewhere: [], system: 0 };
  for (const runtime of snapshot?.runtimes ?? []) {
    const mine = runtime.association.projectId === projectId;
    if (runtime.origin === "managed") {
      if (mine && !runtime.execution?.observer) groups.managed.push(runtime);
    } else if (mine) {
      groups.detected.push(runtime);
    } else if (runtime.category === "system") {
      groups.system += 1;
    } else {
      groups.elsewhere.push(runtime);
    }
  }
  return groups;
}

export interface AssociationRow { label: "Project" | "Worktree" | "Session" | "Block"; value: string; known: boolean }

/** Project/Worktree/Session/Block para exibição: o que não tem evidência vira "—". */
export function associationRows(association: Association): AssociationRow[] {
  const row = (label: AssociationRow["label"], value: string | null): AssociationRow => ({ label, value: value ?? NO_VALUE, known: value !== null });
  return [
    row("Project", association.projectName),
    row("Worktree", association.worktreeName),
    row("Session", association.sessionLabel),
    row("Block", association.blockTitle),
  ];
}

/** "3 min", "2 h 05 min", "1 d 4 h". Ausente quando o início é desconhecido. */
export function uptimeLabel(startedAt: number | null, now: number): string | null {
  if (!startedAt || startedAt > now) return null;
  const seconds = Math.floor((now - startedAt) / 1000);
  if (seconds < 60) return `${seconds} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} h ${String(minutes % 60).padStart(2, "0")} min`;
  return `${Math.floor(hours / 24)} d ${hours % 24} h`;
}

export const cpuLabel = (cpu: number) => `${cpu.toFixed(cpu >= 10 ? 0 : 1)}%`;

/** Portas distintas (IPv4 e IPv6 do mesmo número viram uma). */
export const distinctPorts = (runtime: RuntimeObservation) => [...new Set(runtime.ports.map((p) => p.port))].sort((a, b) => a - b);
