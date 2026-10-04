import type { LogLine } from "./types";

/*
 * Lógica pura do console de runtime (Block 05). Estado de VISUALIZAÇÃO apenas: filtrar, buscar,
 * pausar e limpar nunca tocam o processo, o ring buffer do backend ou qualquer log persistente.
 */
export type StreamFilter = "all" | "out" | "err";

export interface ConsoleView {
  filter: StreamFilter;
  query: string;
  /** Pausa visual: só mostra linhas anteriores a este `seq`. A captura continua no backend. */
  pausedAtSeq: number | null;
  /** "Limpar" esconde o que existia até aqui; o buffer do backend permanece intacto. */
  clearedBeforeSeq: number;
}

export const initialConsoleView: ConsoleView = { filter: "all", query: "", pausedAtSeq: null, clearedBeforeSeq: 0 };

/** Teto de linhas desenhadas por vez: o DOM não cresce com um processo muito verboso. */
export const RENDER_LIMIT = 1000;

export interface VisibleConsole {
  lines: LogLine[];
  /** Linhas que casam com o filtro mas ficaram acima do teto de renderização. */
  omitted: number;
  /** Linhas novas que chegaram durante a pausa. */
  pending: number;
}

export function visibleConsole(lines: LogLine[], view: ConsoleView): VisibleConsole {
  const query = view.query.trim().toLowerCase();
  const matched: LogLine[] = [];
  let pending = 0;
  for (const line of lines) {
    if (line.seq < view.clearedBeforeSeq) continue;
    if (view.pausedAtSeq !== null && line.seq >= view.pausedAtSeq) {
      pending += 1;
      continue;
    }
    if (view.filter !== "all" && line.stream !== view.filter) continue;
    if (query && !line.text.toLowerCase().includes(query)) continue;
    matched.push(line);
  }
  const omitted = Math.max(0, matched.length - RENDER_LIMIT);
  return { lines: omitted ? matched.slice(omitted) : matched, omitted, pending };
}

export const pause = (view: ConsoleView, nextSeq: number): ConsoleView => (view.pausedAtSeq === null ? { ...view, pausedAtSeq: nextSeq } : view);
export const resume = (view: ConsoleView): ConsoleView => ({ ...view, pausedAtSeq: null });
export const clearView = (view: ConsoleView, nextSeq: number): ConsoleView => ({ ...view, clearedBeforeSeq: nextSeq });

/** Texto copiável do que está VISÍVEL (já filtrado), uma linha por linha de log. */
export const copyText = (lines: LogLine[]) => lines.map((line) => line.text).join("\n");

/** Rola para o fim só se o leitor já estava perto dele (não rouba a rolagem de quem está lendo). */
export const shouldAutoscroll = (scrollHeight: number, scrollTop: number, clientHeight: number, threshold = 80) =>
  scrollHeight - scrollTop - clientHeight <= threshold;
