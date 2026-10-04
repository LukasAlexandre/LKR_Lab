import { describe, expect, it } from "vitest";
import {
  RENDER_LIMIT,
  clearView,
  copyText,
  initialConsoleView,
  pause,
  resume,
  shouldAutoscroll,
  visibleConsole,
} from "./runtimeConsole";
import type { LogLine } from "./types";

const line = (seq: number, stream: "out" | "err", text: string): LogLine => ({ seq, ts: 1, stream, text, source: null });
const lines = [line(0, "out", "VITE ready"), line(1, "err", "Warning: deprecated"), line(2, "out", "GET /index"), line(3, "err", "Error: boom")];

describe("console de runtime — filtros", () => {
  it("ALL mostra stdout e stderr na ordem recebida", () => {
    expect(visibleConsole(lines, initialConsoleView).lines.map((l) => l.seq)).toEqual([0, 1, 2, 3]);
  });
  it("STDOUT e STDERR separam os fluxos", () => {
    expect(visibleConsole(lines, { ...initialConsoleView, filter: "out" }).lines.map((l) => l.seq)).toEqual([0, 2]);
    expect(visibleConsole(lines, { ...initialConsoleView, filter: "err" }).lines.map((l) => l.seq)).toEqual([1, 3]);
  });
  it("a busca ignora caixa e combina com o filtro de fluxo", () => {
    expect(visibleConsole(lines, { ...initialConsoleView, query: "ERROR" }).lines.map((l) => l.seq)).toEqual([3]);
    expect(visibleConsole(lines, { ...initialConsoleView, filter: "out", query: "error" }).lines).toEqual([]);
    expect(visibleConsole(lines, { ...initialConsoleView, query: "   " }).lines).toHaveLength(4);
  });
  it("console vazio devolve listas vazias", () => {
    expect(visibleConsole([], initialConsoleView)).toEqual({ lines: [], omitted: 0, pending: 0 });
  });
});

describe("console de runtime — pausa e limpeza (só visual)", () => {
  it("pausar congela a vista e conta o que chegou; retomar continua do buffer disponível", () => {
    const paused = pause(initialConsoleView, 2);
    const view = visibleConsole(lines, paused);
    expect(view.lines.map((l) => l.seq)).toEqual([0, 1]);
    expect(view.pending).toBe(2);
    const resumed = visibleConsole(lines, resume(paused));
    expect(resumed.lines.map((l) => l.seq)).toEqual([0, 1, 2, 3]);
    expect(resumed.pending).toBe(0);
  });
  it("pausar duas vezes não move o ponto de pausa", () => {
    const first = pause(initialConsoleView, 2);
    expect(pause(first, 99).pausedAtSeq).toBe(2);
  });
  it("limpar esconde só o que existia; linhas novas aparecem e o buffer original segue intacto", () => {
    const cleared = clearView(initialConsoleView, 4);
    expect(visibleConsole(lines, cleared).lines).toEqual([]);
    const more = [...lines, line(4, "out", "novo")];
    expect(visibleConsole(more, cleared).lines.map((l) => l.seq)).toEqual([4]);
    expect(lines).toHaveLength(4);
  });
});

describe("console de runtime — desempenho e cópia", () => {
  it("nunca desenha mais que o teto, mantendo as linhas mais recentes", () => {
    const many = Array.from({ length: RENDER_LIMIT + 250 }, (_, i) => line(i, "out", `l${i}`));
    const view = visibleConsole(many, initialConsoleView);
    expect(view.lines).toHaveLength(RENDER_LIMIT);
    expect(view.omitted).toBe(250);
    expect(view.lines[0].seq).toBe(250);
    expect(view.lines.at(-1)?.seq).toBe(RENDER_LIMIT + 249);
  });
  it("copia só o que está visível, uma linha por linha", () => {
    const view = visibleConsole(lines, { ...initialConsoleView, filter: "err" });
    expect(copyText(view.lines)).toBe("Warning: deprecated\nError: boom");
  });
  it("autoscroll só acompanha quem já está perto do fim", () => {
    expect(shouldAutoscroll(1000, 880, 100)).toBe(true);
    expect(shouldAutoscroll(1000, 300, 100)).toBe(false);
  });
});
