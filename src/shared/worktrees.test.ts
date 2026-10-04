import { describe, expect, it } from "vitest";
import {
  KIND_BADGE,
  createProblem,
  finalizeGitWarning,
  gitText,
  itemBranch,
  itemName,
  matchesGit,
  matchesQuery,
  pauseGitWarning,
  showPrimary,
  stateActions,
  statusBadge,
  suggestPath,
  syncText,
  visibleItems,
} from "./worktrees";
import type { Filters } from "./worktrees";
import type { GitSummary, Worktree, WorktreeItem, WorktreeManagedView, WorktreeStatus } from "./types";

const git = (over: Partial<GitSummary> = {}): GitSummary => ({
  isRepo: true, branch: "x", detached: false, upstream: null, ahead: null, behind: null,
  staged: 0, unstaged: 0, untracked: 0, conflicts: 0, changes: 0, clean: true, error: null, ...over,
});
const gw = (over: Partial<Worktree> = {}): Worktree => ({
  path: "C:/dev/wt", head: "abc1234", branch: "feature/x", isPrimary: false, detached: false, bare: false,
  locked: false, prunable: false, ...over,
});
const managed = (status: WorktreeStatus, over: Partial<WorktreeManagedView> = {}): WorktreeManagedView => ({
  id: "w-" + status, projectId: "p", displayName: "Meu " + status, status, createdAt: "", updatedAt: "",
  session: null, block: null, lastEventAt: null, ...over,
});
const item = (kind: WorktreeItem["kind"], over: Partial<WorktreeItem> = {}): WorktreeItem => ({
  kind, git: gw(), managed: null, gitSummary: { status: "available", data: git(), message: null }, warnings: [], ...over,
});
const FILTERS: Filters = { query: "", operational: "all", git: "all" };

const primary = item("primary", { git: gw({ isPrimary: true, branch: "main" }) });
const active = item("managed_available", { managed: managed("active", { session: { id: "s1", label: "SESSION-001", title: "Machine Context", status: "active" }, block: { id: "b1", title: "Concept 09", status: "in_progress" } }) });
const frozen = item("managed_available", { git: gw({ branch: "feature/frozen" }), managed: managed("frozen", { branchHint: "feature/frozen" }), gitSummary: { status: "available", data: git({ ahead: 2, behind: 1 }), message: null } });
const stopped = item("managed_available", { git: gw({ branch: "feature/stopped" }), managed: managed("stopped"), gitSummary: { status: "available", data: git({ unstaged: 3, changes: 3, clean: false }), message: null } });
const completed = item("managed_available", { git: gw({ branch: "feature/done" }), managed: managed("completed") });
const missing = item("managed_missing", { git: null, gitSummary: null, managed: managed("active", { displayName: "Sumido", branchHint: "feature/sumido" }) });
const unmanaged = item("unmanaged", { git: gw({ branch: "feature/solta", path: "C:/dev/solta" }) });
const ALL = [primary, active, frozen, stopped, completed, missing, unmanaged];

describe("rótulos", () => {
  it("estados operacionais em caixa alta", () => {
    expect(["active", "frozen", "stopped", "completed"].map((s) => statusBadge(s as WorktreeStatus))).toEqual(["ATIVO", "CONGELADO", "PARADO", "FINALIZADO"]);
  });
  it("tipo não é estado: principal, não gerenciado e não localizado têm selos próprios", () => {
    expect(KIND_BADGE.primary).toBe("PRINCIPAL");
    expect(KIND_BADGE.unmanaged).toBe("NÃO GERENCIADO");
    expect(KIND_BADGE.managed_missing).toBe("NÃO LOCALIZADO");
    expect(KIND_BADGE.managed_available).toBeUndefined();
  });
  it("nome e branch: persistido > branch > pasta; sem Git usa a dica", () => {
    expect(itemName(active)).toBe("Meu active");
    expect(itemName(unmanaged)).toBe("feature/solta");
    expect(itemName(item("unmanaged", { git: gw({ branch: "", detached: true, path: "C:/dev/pasta-x" }) }))).toBe("pasta-x");
    expect(itemBranch(missing)).toBe("feature/sumido");
    expect(itemBranch(item("unmanaged", { git: gw({ branch: "" }) }))).toBe("");
  });
});

describe("filtros operacionais (só gerenciados) e o principal", () => {
  it("Todos mostra gerenciados, não localizados e não gerenciados — nunca o principal no grid", () => {
    expect(visibleItems(ALL, FILTERS)).toEqual([active, frozen, stopped, completed, missing, unmanaged]);
  });
  it("cada estado filtra só os gerenciados com aquele estado", () => {
    expect(visibleItems(ALL, { ...FILTERS, operational: "active" })).toEqual([active, missing]);
    expect(visibleItems(ALL, { ...FILTERS, operational: "frozen" })).toEqual([frozen]);
    expect(visibleItems(ALL, { ...FILTERS, operational: "stopped" })).toEqual([stopped]);
    expect(visibleItems(ALL, { ...FILTERS, operational: "completed" })).toEqual([completed]);
    expect(visibleItems(ALL, { ...FILTERS, operational: "missing" })).toEqual([missing]);
    expect(visibleItems(ALL, { ...FILTERS, operational: "unmanaged" })).toEqual([unmanaged]);
  });
  it("o principal só aparece com 'Todos' e respeita busca e filtro Git", () => {
    expect(showPrimary(primary, FILTERS)).toBe(true);
    expect(showPrimary(primary, { ...FILTERS, operational: "active" })).toBe(false);
    expect(showPrimary(primary, { ...FILTERS, query: "main" })).toBe(true);
    expect(showPrimary(primary, { ...FILTERS, query: "xyz" })).toBe(false);
    expect(showPrimary(undefined, FILTERS)).toBe(false);
  });
});

describe("filtro Git (separado do operacional)", () => {
  it("Clean, Alterações, Ahead, Behind e Conflitos", () => {
    expect(visibleItems(ALL, { ...FILTERS, git: "changes" })).toEqual([stopped]);
    expect(visibleItems(ALL, { ...FILTERS, git: "ahead" })).toEqual([frozen]);
    expect(visibleItems(ALL, { ...FILTERS, git: "behind" })).toEqual([frozen]);
    expect(visibleItems(ALL, { ...FILTERS, git: "conflicts" })).toEqual([]);
    expect(visibleItems(ALL, { ...FILTERS, git: "clean" }).map((i) => i.kind)).toContain("unmanaged");
    // sem Git lido (não localizado) nenhum filtro Git o acusa
    expect(matchesGit(missing, "clean")).toBe(false);
    expect(matchesGit(missing, "changes")).toBe(false);
    expect(matchesGit(missing, "all")).toBe(true);
  });
  it("combina operacional + Git + busca", () => {
    expect(visibleItems(ALL, { query: "stopped", operational: "stopped", git: "changes" })).toEqual([stopped]);
    expect(visibleItems(ALL, { query: "", operational: "active", git: "changes" })).toEqual([]);
  });
  it("Git sujo NÃO muda o estado operacional: o item continua parado", () => {
    expect(stopped.managed?.status).toBe("stopped");
    expect(visibleItems(ALL, { ...FILTERS, operational: "stopped", git: "changes" })).toHaveLength(1);
  });
});

describe("busca", () => {
  it("por nome, branch, Session e Block (sem acento nem caixa)", () => {
    expect(matchesQuery(active, "MEU ACTIVE")).toBe(true);
    expect(matchesQuery(frozen, "feature/frozen")).toBe(true);
    expect(matchesQuery(active, "session-001")).toBe(true);
    expect(matchesQuery(active, "machine context")).toBe(true);
    expect(matchesQuery(active, "concept 09")).toBe(true);
    expect(matchesQuery(missing, "sumido")).toBe(true);
    expect(matchesQuery(unmanaged, "SESSION-001")).toBe(false);
    expect(matchesQuery(active, "   ")).toBe(true);
  });
});

describe("ações de estado", () => {
  it("espelham o core; finalizado é terminal", () => {
    expect(stateActions("active")).toEqual(["freeze", "stop", "complete"]);
    expect(stateActions("frozen")).toEqual(["resume", "complete"]);
    expect(stateActions("stopped")).toEqual(["resume", "complete"]);
    expect(stateActions("completed")).toEqual([]);
  });
});

describe("avisos de Git (nunca bloqueiam)", () => {
  it("finalizar com alterações: texto exato; conflitos: aviso mais forte; ahead também informa", () => {
    expect(finalizeGitWarning(git({ changes: 2, unstaged: 2 }))).toEqual({ level: "changes", text: "Esta worktree possui alterações Git. Finalizar no LKR LAB não fará commit, merge, push ou remoção." });
    expect(finalizeGitWarning(git({ untracked: 1 })).level).toBe("changes");
    expect(finalizeGitWarning(git({ ahead: 3 })).level).toBe("changes");
    const c = finalizeGitWarning(git({ conflicts: 2, changes: 2 }));
    expect(c.level).toBe("conflicts");
    expect(c.text).toContain("conflitos");
    expect(finalizeGitWarning(git()).level).toBe("none");
    expect(finalizeGitWarning(null).level).toBe("none");
  });
  it("parar/congelar sujo é permitido, só informa", () => {
    expect(pauseGitWarning(git({ changes: 1, unstaged: 1 }))).toContain("alterações Git");
    expect(pauseGitWarning(git())).toBe("");
  });
});

describe("Git do card", () => {
  it("Clean, alterações com plural, conflitos e sem dado", () => {
    expect(gitText(unmanaged)).toEqual({ text: "Clean", tone: "good" });
    expect(gitText(stopped)).toEqual({ text: "3 alterações", tone: "warn" });
    expect(gitText(item("unmanaged", { gitSummary: { status: "available", data: git({ unstaged: 1, changes: 1 }), message: null } })).text).toBe("1 alteração");
    expect(gitText(item("unmanaged", { gitSummary: { status: "available", data: git({ conflicts: 2 }), message: null } })).text).toBe("Conflitos (2)");
    expect(gitText(missing)).toEqual({ text: "—", tone: "neutral" });
    expect(gitText(item("unmanaged", { gitSummary: { status: "error", data: null, message: "x" } })).text).toBe("Indisponível");
  });
  it("sync", () => {
    expect(syncText(git({ ahead: 2, behind: 1 }))).toBe("↑2 ↓1");
    expect(syncText(git())).toBe("");
    expect(syncText(null)).toBe("");
  });
});

describe("novo worktree", () => {
  it("sugere uma pasta irmã (local) a partir do projeto e da branch", () => {
    expect(suggestPath("C:\\Dev\\LKR_Lab", "feature/x")).toBe("C:\\Dev\\LKR_Lab-feature-x");
    expect(suggestPath("\\\\?\\C:\\Dev\\LKR_Lab\\", "fix/y z")).toBe("C:\\Dev\\LKR_Lab-fix-y-z");
    expect(suggestPath("/home/u/lkr", "feature/x")).toBe("/home/u/lkr-feature-x");
    expect(suggestPath("", "x")).toBe("");
    expect(suggestPath("C:\\Dev\\p", "  ")).toBe("");
  });
  it("valida o formulário (modos nova branch e branch existente)", () => {
    const base = { mode: "new_branch" as const, branch: "feature/x", baseRef: "HEAD", path: "C:\\Dev\\p-x", blockId: "", sessionId: "" };
    expect(createProblem(base)).toBeNull();
    expect(createProblem({ ...base, branch: " " })).toBeTruthy();
    expect(createProblem({ ...base, branch: "-x" })).toBeTruthy();
    expect(createProblem({ ...base, baseRef: "" })).toBeTruthy();
    expect(createProblem({ ...base, mode: "existing_branch", baseRef: "" })).toBeNull();
    expect(createProblem({ ...base, path: "relativo" })).toBeTruthy();
    expect(createProblem({ ...base, path: "" })).toBeTruthy();
    expect(createProblem({ ...base, blockId: "b1" })).toBeTruthy();
    expect(createProblem({ ...base, blockId: "b1", sessionId: "s1" })).toBeNull();
  });
});
