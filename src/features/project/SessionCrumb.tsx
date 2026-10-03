import { useResource, workspace } from "../../state/workspace";

/** Último segmento do breadcrumb: o SESSION-NNN humano da Session da rota (a rota usa o UUID). */
export function SessionCrumb({ projectId, sessionId }: { projectId: string; sessionId: string }) {
  const { data } = useResource(workspace.forProject(projectId).ddae);
  const found = data?.sessions.find((s) => s.id === sessionId);
  return <span aria-current="page">{found?.label ?? "Sessão"}</span>;
}
