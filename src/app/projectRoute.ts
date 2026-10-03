/*
 * Roteador por hash com o Project na URL (Concept 05).
 *
 *   #project/<id>/<área>   contexto de um Project (a ROTA é a fonte de verdade)
 *   #project/<id>/ddae/<session-uuid>   detalhe de uma Session DDAE (o UUID é a identidade;
 *                          SESSION-NNN é só rótulo e nunca entra na rota)
 *   #projects              lista (Concept 03)
 *   #projects/new          cadastro (Concept 04)
 *   #projects/open         LEGADO: resolve o Project ativo e redireciona
 *   #<rota global>         Dashboard, Portas, Git/PRs…
 *
 * Funções puras: nada aqui lê window, preferências ou o banco.
 */
export const PROJECT_AREAS = ["overview", "ddae", "worktrees", "planning", "runtime", "git", "logs", "context"] as const;
export type ProjectArea = (typeof PROJECT_AREAS)[number];

export const PROJECT_AREA_TITLES: Record<ProjectArea, string> = {
  overview: "Visão geral",
  ddae: "DDAE / Sessões",
  worktrees: "Worktrees",
  planning: "Planejamento",
  runtime: "Runtime",
  git: "Git",
  logs: "Logs",
  context: "Contexto IA",
};

export const PROJECTS_HASH = "#projects";
export const NEW_PROJECT_HASH = "projects/new";
export const LEGACY_OPEN_HASH = "projects/open";
export const PROJECT_NOT_FOUND = "Projeto não encontrado.";

/** Ids são UUIDs, mas o roteador só exige um token seguro (sem barras nem espaços). */
const ID_PATTERN = /^[A-Za-z0-9_-]{1,64}$/;
export const isProjectArea = (value: string | undefined): value is ProjectArea =>
  !!value && (PROJECT_AREAS as readonly string[]).includes(value);

export type ParsedRoute =
  | { kind: "global"; route: string }
  | { kind: "new-project" }
  | { kind: "legacy-open" }
  /** `normalized` = a área da URL era inválida/ausente e caiu em overview (a URL deve ser corrigida). */
  | { kind: "project"; projectId: string; area: ProjectArea; normalized: boolean; sessionId?: string }
  | { kind: "invalid-project" };

export function parseHash(hash: string, globalRoutes: readonly string[]): ParsedRoute {
  const parts = hash.replace(/^#/, "").split("/");
  const head = parts[0];
  if (head === "project") {
    const id = parts[1];
    if (!id || !ID_PATTERN.test(id)) return { kind: "invalid-project" };
    const area = parts[2];
    if (!isProjectArea(area)) return { kind: "project", projectId: id, area: "overview", normalized: true };
    // Só o DDAE tem um 4º segmento: o id da Session. Qualquer outra forma é corrigida na URL.
    if (area === "ddae" && parts.length === 4 && ID_PATTERN.test(parts[3])) {
      return { kind: "project", projectId: id, area, sessionId: parts[3], normalized: false };
    }
    return { kind: "project", projectId: id, area, normalized: parts.length > 3 };
  }
  if (head === "projects" && parts[1] === "new") return { kind: "new-project" };
  if (head === "projects" && parts[1] === "open") return { kind: "legacy-open" };
  return { kind: "global", route: globalRoutes.includes(head) ? head : "dashboard" };
}

export const projectHash = (projectId: string, area: ProjectArea = "overview", sessionId?: string) =>
  sessionId && area === "ddae" ? `#project/${projectId}/${area}/${sessionId}` : `#project/${projectId}/${area}`;

/** Aviso quando a Session da rota não existe (ou é de outro Project). */
export const SESSION_NOT_FOUND = "Sessão não encontrada neste projeto.";

export type RouteResolution =
  | { action: "render" }
  /** O registro de Projects ainda não carregou: não decidir (nem eleger um Project) por ele. */
  | { action: "wait" }
  | { action: "redirect"; hash: string; notice?: string };

/** Decide o que fazer com a rota diante dos Projects conhecidos. Nunca escolhe um Project arbitrário. */
export function resolveRoute(
  route: ParsedRoute,
  registry: { loaded: boolean; ids: readonly string[] },
  activeId: string,
): RouteResolution {
  if (route.kind === "invalid-project") return { action: "redirect", hash: PROJECTS_HASH, notice: PROJECT_NOT_FOUND };
  if (route.kind === "legacy-open") {
    if (!registry.loaded) return { action: "wait" };
    return registry.ids.includes(activeId)
      ? { action: "redirect", hash: projectHash(activeId) }
      : { action: "redirect", hash: PROJECTS_HASH };
  }
  if (route.kind === "project") {
    if (!registry.loaded) return { action: "wait" };
    if (!registry.ids.includes(route.projectId)) return { action: "redirect", hash: PROJECTS_HASH, notice: PROJECT_NOT_FOUND };
    if (route.normalized) return { action: "redirect", hash: projectHash(route.projectId, route.area) };
  }
  return { action: "render" };
}

/** Trocar de Project dentro do contexto mantém a área; fora dele não há rota a trocar (`null`). */
export const switchProjectHash = (route: ParsedRoute, newId: string): string | null =>
  route.kind === "project" ? projectHash(newId, route.area) : null;
