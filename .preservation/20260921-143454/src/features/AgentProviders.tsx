import { useEffect } from "react";
import { desktop } from "../shared/api";
import { Badge, Empty, Refresh } from "../shared/ui";
import { SourceStatus } from "../components/SourceStatus";
import { useResource, workspace } from "../state/workspace";

export function AgentProviders() {
  const source = workspace.agentProviders;
  const { data, loading, lastUpdated, error } = useResource(source);
  useEffect(() => { if (desktop) void source.refresh(30_000); }, [source]);
  return <>
    <div className="row spread"><SourceStatus source={source} label="Providers" /><Refresh busy={loading} onClick={() => void source.refresh()} /></div>
    {data.map(provider => <section key={provider.provider} className="provider-entry">
      <div className="row spread"><strong>{provider.provider}</strong><Badge tone={error && lastUpdated ? "warn" : provider.availability === "available" ? "good" : "neutral"}>{error && lastUpdated ? "stale" : provider.availability}</Badge></div>
      <p className="muted">{provider.detail}</p>
      <dl className="facts">
        <div><dt>Conta</dt><dd>{provider.accountStatus}</dd></div>
        <div><dt>Uso / contexto</dt><dd>{provider.usageStatus}</dd></div>
        <div><dt>Sessões</dt><dd>{provider.sessionsStatus}</dd></div>
      </dl>
    </section>)}
    {!data.length && !loading && <Empty title="Providers ainda não consultados"><p>Atualize para detectar ferramentas locais.</p></Empty>}
    <p className="footnote">A presença da ferramenta não confirma autenticação ou atividade. Dados não expostos pela integração aparecem como unsupported.</p>
  </>;
}
