import { deriveDdaeNextAction } from "./ddae";
import { changesLabel } from "./logic";
import { isDirty } from "./projectOverview";
import type { ProjectArea } from "../app/projectRoute";
import type { DdaeOverview, PlanningSummary, ProjectOverview, Worktree } from "./types";

/*
 * Lógica pura da Visão geral do Project Control Center (Concept 05).
 * Só fatos reais: o que não existe (Planejamento, estado operacional de worktree) não é derivado aqui.
 */

export type NextActionTarget =
  | { kind: "locate" }
  | { kind: "area"; area: ProjectArea }
  | { kind: "context" }
  | { kind: "none" };

export interface NextAction {
  id: "locate" | "conflicts" | "review-changes" | "review-runtime" | "continue-block" | "start-block" | "start-planning-item" | "generate-context" | "none";
  title: string;
  description: string;
  cta: string | null;
  target: NextActionTarget;
}

/**
 * Próxima ação DETERMINÍSTICA, sem IA: a primeira regra verdadeira vence, nesta ordem.
 *  1. Missing/Unbound            → Localizar projeto
 *  2. Git com conflitos          → Resolver conflitos (Git)
 *  3. Git com alterações locais  → Revisar alterações (Git)
 *  4. Runtime com erro de leitura→ Revisar Runtime
 *  5. DDAE: sessão ativa com bloco em andamento → Continuar <bloco>;
 *           sessão ativa sem bloco atual e com próximo → Iniciar <bloco>
 *           (só com o DDAE real carregado; `null`/ausente = a regra não dispara)
 *  6. Planejamento: SEM Session ativa e com item PLANEJADO → Iniciar <item> (o PRÓXIMO da fila).
 *           Com Session ativa a regra não dispara: o item não pode ser iniciado.
 *  7. Contexto IA nunca gerado   → Gerar contexto (só quando `contextGenerated === false`;
 *                                  `null` = desconhecido, a regra não dispara. "Defasado" não é observável.)
 *  8. Nada urgente               → nenhuma ação pendente
 * O DDAE e o Planejamento entram DEPOIS dos problemas prioritários (1–4). Não existe regra de IA.
 */
export function deriveProjectNextAction(
  project: ProjectOverview,
  contextGenerated: boolean | null,
  ddae?: DdaeOverview | null,
  planning?: PlanningSummary | null,
): NextAction {
  if (project.location !== "available") {
    return {
      id: "locate",
      title: "Localizar projeto",
      description:
        project.location === "missing"
          ? "A pasta vinculada não existe mais nesta máquina. Localize a pasta para voltar a usar o projeto."
          : "Este projeto ainda não tem pasta vinculada neste computador.",
      cta: "Localizar",
      target: { kind: "locate" },
    };
  }
  const git = project.git.status === "available" ? project.git.data : null;
  if (git && git.conflicts > 0) {
    return {
      id: "conflicts",
      title: "Resolver conflitos",
      description: `${git.conflicts} ${git.conflicts === 1 ? "arquivo em conflito" : "arquivos em conflito"} no repositório.`,
      cta: "Abrir Git",
      target: { kind: "area", area: "git" },
    };
  }
  if (isDirty(project) && git) {
    return {
      id: "review-changes",
      title: "Revisar alterações",
      description: `${changesLabel(git.changes || git.staged + git.unstaged + git.untracked)} locais ainda não commitadas.`,
      cta: "Abrir Git",
      target: { kind: "area", area: "git" },
    };
  }
  if (project.runtime.status === "error") {
    return {
      id: "review-runtime",
      title: "Revisar Runtime",
      description: project.runtime.message ?? "Não foi possível ler o estado de execução do projeto.",
      cta: "Abrir Runtime",
      target: { kind: "area", area: "runtime" },
    };
  }
  const work = deriveDdaeNextAction(ddae);
  if (work) {
    return {
      id: work.kind === "continue" ? "continue-block" : "start-block",
      title: work.title,
      description: work.description,
      cta: "Abrir DDAE",
      target: { kind: "area", area: "ddae" },
    };
  }
  if (planning?.next && !planning.activeSession) {
    return {
      id: "start-planning-item",
      title: `Iniciar ${planning.next.title}`,
      description: "É o próximo item planejado da fila e não há Session ativa neste projeto.",
      cta: "Abrir Planejamento",
      target: { kind: "area", area: "planning" },
    };
  }
  if (contextGenerated === false) {
    return {
      id: "generate-context",
      title: "Gerar contexto",
      description: "O contexto de IA deste projeto ainda não foi gerado.",
      cta: "Gerar contexto",
      target: { kind: "context" },
    };
  }
  return {
    id: "none",
    title: "Nenhuma ação pendente",
    description: "Sem conflitos, alterações locais ou problemas de runtime a tratar.",
    cta: null,
    target: { kind: "none" },
  };
}

/** Só fatos do Git: contagem, branch(es) e a worktree principal. Sem status operacional do LKR LAB. */
export function worktreeSummary(trees: Worktree[]) {
  return {
    count: trees.length,
    branches: trees.map((t) => t.branch).filter(Boolean),
    locked: trees.filter((t) => t.locked).length,
  };
}

/** Rótulo de runtime para o cabeçalho do projeto aberto. */
export function runtimeHeadline(project: ProjectOverview): string {
  if (project.runtime.status !== "available" || !project.runtime.data) return "Indisponível";
  const { running, listeningPorts, managedRuns } = project.runtime.data;
  if (!running) return "Parado";
  const parts = [managedRuns ? `${managedRuns} execução${managedRuns === 1 ? "" : "ões"}` : "", listeningPorts.length ? `portas ${listeningPorts.join(", ")}` : ""];
  return ["Em execução", ...parts.filter(Boolean)].join(" · ");
}
