import type { Resource } from "../state/resource";
import { useResource } from "../state/workspace";

export function SourceStatus<T>({ source, label }: { source: Resource<T>; label: string }) {
  const state = useResource(source);
  return <div className="source-status" role={state.error ? "alert" : "status"}>
    <span>{label}: {state.loading ? "atualizando em segundo plano…" : state.error ? state.error : state.lastUpdated ? `atualizado às ${new Date(state.lastUpdated).toLocaleTimeString("pt-BR")}` : "ainda não consultado"}</span>
    {state.error && <button className="text-button" onClick={() => void source.refresh()}>Tentar novamente</button>}
    {state.error && state.lastUpdated !== null && <small>Exibindo a última consulta disponível.</small>}
    {import.meta.env.DEV && state.durationMs !== null && <small>{Math.round(state.durationMs)} ms</small>}
  </div>;
}
