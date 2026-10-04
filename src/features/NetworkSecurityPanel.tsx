import { useState } from "react";
import { AlertTriangle, ShieldCheck } from "lucide-react";
import { Badge } from "../shared/ui";
import { Domain } from "./WindowsHealthPanel";
import { WIN_HEALTH_LABEL, UNKNOWN_TEXT, formatDateTime, sourceText } from "../shared/windowsHealth";
import {
  ACTION_LABEL,
  AV_PROVIDER_LABEL,
  BITLOCKER_LABEL,
  DEFAULT_EXPOSURE_FILTER,
  DEFENDER_STATE_LABEL,
  INTERFACE_KIND_LABEL,
  LISTENER_SCOPE_LABEL,
  NET_DOMAIN_LABEL,
  REMOTE_SCOPE_LABEL,
  SECURE_BOOT_LABEL,
  WSC_LABEL,
  exposureSummary,
  filterConnections,
  filterListeners,
  formatLinkSpeed,
  listenerAddress,
  listenerOwner,
  profileLabel,
  summaryItems,
  unavailableNetSources,
  type ConnectionFilter,
  type ExposureFilter,
  type ScopeFilter,
} from "../shared/networkSecurity";
import type { NetworkSecuritySnapshot } from "../shared/types";
import type { NetworkSecurityState } from "../state/networkSecurity";

const STATUS_DOT = { healthy: "good", attention: "warn", critical: "danger", unknown: "" } as const;
const MAX_VISIBLE = 40;

const when = (ms: number | null | undefined) => formatDateTime(ms) ?? UNKNOWN_TEXT;
const list = (items: string[]) => (items.length ? items.join(", ") : UNKNOWN_TEXT);

/** Network & Security: somente observação (nada é ativado, bloqueado, encerrado ou escaneado). */
export function NetworkSecurityPanel({ state, now }: { state: NetworkSecurityState; now: number }) {
  const { snapshot, error } = state;
  if (!snapshot) {
    return (
      <section className="panel mh-winhealth mh-netsec" aria-label="Network & Security">
        <div className="panel-title"><h2><ShieldCheck size={17} />Network &amp; Security</h2></div>
        <p className="muted" role="status">{error ?? "Lendo a rede e a postura de segurança…"}</p>
      </section>
    );
  }
  return <Loaded snapshot={snapshot} error={error} now={now} />;
}

function Loaded({ snapshot: s, error, now }: { snapshot: NetworkSecuritySnapshot; error: string | null; now: number }) {
  const overall = s.overall;
  const missing = unavailableNetSources(s);
  return (
    <section className="panel mh-winhealth mh-netsec" aria-label="Network & Security">
      <div className="panel-title">
        <h2><ShieldCheck size={17} />Network &amp; Security</h2>
        <span className={`mh-win-overall status-${overall.status}`}>
          <span className={`dot ${STATUS_DOT[overall.status]}`} />
          {WIN_HEALTH_LABEL[overall.status]}
          <small>{overall.evaluated} de {overall.rateable} verificações avaliadas</small>
        </span>
      </div>

      {overall.reasons.length > 0 && (
        <ul className={`mh-win-reasons ${overall.status}`} role="alert" aria-label="Por que este estado">
          {overall.reasons.map((reason) => (
            <li key={`${reason.domain}-${reason.text}`}>
              <AlertTriangle size={14} />
              <span><strong>{NET_DOMAIN_LABEL[reason.domain] ?? reason.domain}:</strong> {reason.text}</span>
            </li>
          ))}
        </ul>
      )}
      {overall.status === "unknown" && (
        <p className="muted" role="status">Não foi possível avaliar nenhuma verificação com segurança; nada é tratado como seguro.</p>
      )}
      {error && <p className="muted" role="status">A última atualização falhou ({error}); mostrando a leitura anterior.</p>}

      <div className="mh-win-summary" aria-label="Resumo de rede e segurança">
        {summaryItems(s).map((item) => (
          <div key={item.id} className={`mh-win-item ${item.status ?? ""}`}>
            <small>{item.label}</small>
            <strong>{item.value}</strong>
          </div>
        ))}
      </div>

      <Domain title="Rede" section={s.network} now={now} summary={s.network.activeInterface ?? "Sem conexão ativa"}>
        <dl className="facts">
          <div><dt>Interface ativa</dt><dd>{s.network.activeInterface ?? UNKNOWN_TEXT}</dd></div>
          <div><dt>IP local (IPv4)</dt><dd className="mono">{s.network.localIpv4 ?? UNKNOWN_TEXT}</dd></div>
          <div><dt>Gateway padrão</dt><dd className="mono">{s.network.gateway ?? UNKNOWN_TEXT}</dd></div>
          <div><dt>DNS</dt><dd className="mono">{list(s.network.dns)}</dd></div>
          <div><dt>Categoria da rede</dt><dd>{profileLabel(s.network.profile)}</dd></div>
          <div><dt>IP público</dt><dd title={s.network.publicIp.note}>{s.network.publicIp.queried ? UNKNOWN_TEXT : "Não consultado"}</dd></div>
        </dl>
        <p className="muted">{s.network.publicIp.note}</p>
        <ul className="mh-win-list" aria-label="Interfaces">
          {s.network.interfaces.map((net) => (
            <li key={net.name} className={net.active ? "active" : ""}>
              <span>
                <strong>{net.name}</strong>
                <small className="muted"> · {INTERFACE_KIND_LABEL[net.kind] ?? net.kind}{net.up ? "" : " · desconectada"}</small>
                {net.active && <Badge tone="good">Rota padrão</Badge>}
              </span>
              <span>
                <span className="mono">{list(net.ipv4)}{net.prefix != null && net.ipv4.length ? `/${net.prefix}` : ""}</span>
                {net.ipv6.length > 0 && <span className="mono"> · IPv6 {net.ipv6[0]}</span>}
                {net.gateways.length > 0 && <span> · gateway <span className="mono">{net.gateways[0]}</span></span>}
                {net.up && <span> · {formatLinkSpeed(net.linkSpeedBps)}{net.dhcp ? " · DHCP" : ""}</span>}
              </span>
            </li>
          ))}
        </ul>
      </Domain>

      <ExposureDomain s={s} now={now} />
      <ConnectionsDomain s={s} now={now} />

      <Domain title="Firewall do Windows" section={s.firewall} now={now} summary={s.firewall.activeProfile ? `perfil ${profileLabel(s.firewall.activeProfile)}` : undefined}>
        <ul className="mh-win-list" aria-label="Perfis do firewall">
          {s.firewall.profiles.map((profile) => (
            <li key={profile.kind} className={profile.enabled === false && profile.active ? "critical" : ""}>
              <span>
                <strong>{profile.label}</strong>
                {profile.active && <Badge tone="good">Perfil ativo</Badge>}
              </span>
              <span>
                {profile.enabled == null ? UNKNOWN_TEXT : profile.enabled ? "Ativado" : "Desativado"}
                {profile.defaultInbound && ` · entrada: ${ACTION_LABEL[profile.defaultInbound]}`}
                {profile.defaultOutbound && ` · saída: ${ACTION_LABEL[profile.defaultOutbound]}`}
              </span>
            </li>
          ))}
        </ul>
        <dl className="facts">
          <div><dt>Saúde (Security Center)</dt><dd>{s.firewall.securityCenter ? WSC_LABEL[s.firewall.securityCenter] : UNKNOWN_TEXT}</dd></div>
        </dl>
        {s.firewall.notes.map((note) => <p className="muted" key={note}>{note}</p>)}
        {s.firewall.activeProfile == null && (
          <p className="muted">A categoria da rede ativa não foi informada; o estado reflete todos os perfis lidos.</p>
        )}
      </Domain>

      <Domain title="Antivírus e Defender" section={s.antivirus} now={now} summary={AV_PROVIDER_LABEL[s.antivirus.provider]}>
        <dl className="facts">
          <div><dt>Provedor ativo</dt><dd>{AV_PROVIDER_LABEL[s.antivirus.provider]}</dd></div>
          <div><dt>Microsoft Defender</dt><dd>{DEFENDER_STATE_LABEL[s.antivirus.defender.state]}</dd></div>
          <div><dt>Proteção em tempo real</dt><dd>{s.antivirus.defender.realtimeProtection == null ? UNKNOWN_TEXT : s.antivirus.defender.realtimeProtection ? "Ativa" : "Desativada"}</dd></div>
          <div><dt>Saúde (Security Center)</dt><dd>{s.antivirus.securityCenter ? WSC_LABEL[s.antivirus.securityCenter] : UNKNOWN_TEXT}</dd></div>
          <div><dt>Antivírus de terceiros</dt><dd>{s.antivirus.thirdPartyCount == null ? UNKNOWN_TEXT : s.antivirus.thirdPartyCount === 0 ? "Nenhum registrado" : `${s.antivirus.thirdPartyCount} registrado(s)`}</dd></div>
          <div><dt>Versão das assinaturas</dt><dd className="mono">{s.antivirus.defender.signatureVersion ?? UNKNOWN_TEXT}</dd></div>
          <div><dt>Assinaturas atualizadas</dt><dd>{when(s.antivirus.defender.signaturesUpdatedAt)}{s.antivirus.defender.signatureAgeDays != null ? ` (${s.antivirus.defender.signatureAgeDays} dia(s))` : ""}</dd></div>
          <div><dt>Versão do mecanismo</dt><dd className="mono">{s.antivirus.defender.engineVersion ?? UNKNOWN_TEXT}</dd></div>
          <div><dt>Ameaças ativas</dt><dd>{s.antivirus.defender.activeThreats == null ? "Não consultado" : s.antivirus.defender.activeThreats}</dd></div>
        </dl>
        {s.antivirus.notes.map((note) => <p className="muted" key={note}>{note}</p>)}
      </Domain>

      <Domain title="Criptografia (BitLocker)" section={s.encryption} now={now}>
        {s.encryption.volumes.length > 0 ? (
          <ul className="mh-win-list" aria-label="Volumes BitLocker">
            {s.encryption.volumes.map((volume) => (
              <li key={volume.mount}>
                <span><strong>{volume.mount}</strong>{volume.system && <small className="muted"> · sistema</small>}</span>
                <span>{BITLOCKER_LABEL[volume.state]}</span>
              </li>
            ))}
          </ul>
        ) : (
          <p className="muted">Estado do BitLocker por volume: {UNKNOWN_TEXT.toLowerCase()}. Chaves de recuperação nunca são lidas.</p>
        )}
      </Domain>

      <Domain title="Secure Boot" section={s.secureBoot} now={now} summary={SECURE_BOOT_LABEL[s.secureBoot.state]}>
        <dl className="facts">
          <div><dt>Secure Boot</dt><dd>{SECURE_BOOT_LABEL[s.secureBoot.state]}</dd></div>
          <div><dt>Firmware</dt><dd>{s.secureBoot.uefi == null ? UNKNOWN_TEXT : s.secureBoot.uefi ? "UEFI" : "BIOS legado"}</dd></div>
        </dl>
      </Domain>

      <Domain title="TPM" section={s.tpm} now={now} summary={s.tpm.present == null ? undefined : s.tpm.present ? "presente" : "não detectado"}>
        <dl className="facts">
          <div><dt>TPM</dt><dd>{s.tpm.present == null ? UNKNOWN_TEXT : s.tpm.present ? "Presente" : "Não detectado"}</dd></div>
          <div><dt>Versão</dt><dd>{s.tpm.version ?? UNKNOWN_TEXT}</dd></div>
        </dl>
        <p className="muted">Somente a presença e a versão são lidas; nenhum comando é enviado ao chip.</p>
      </Domain>

      {missing.length > 0 && (
        <details className="mh-win-missing">
          <summary>Fontes não disponíveis ({missing.length})</summary>
          <ul className="mh-win-list" aria-label="Fontes indisponíveis">
            {missing.map((note) => <li key={note.id}><span>{note.label}</span><span className="muted">{sourceText(note)}</span></li>)}
          </ul>
        </details>
      )}
      <p className="footnote">
        Somente observação: o LKR LAB não ativa firewall, não altera regras, não bloqueia portas, não encerra processos, não inicia varredura do Defender nem altera DNS, rotas ou adaptadores. Conexões e endereços remotos ficam só nesta máquina.
      </p>
    </section>
  );
}

const SCOPE_OPTIONS: { value: ScopeFilter; label: string }[] = [
  { value: "all", label: "Todos os escopos" },
  { value: "all_interfaces", label: LISTENER_SCOPE_LABEL.all_interfaces },
  { value: "specific", label: LISTENER_SCOPE_LABEL.specific },
  { value: "loopback", label: LISTENER_SCOPE_LABEL.loopback },
];

/** Portas em escuta, com busca e filtro de escopo. Descritivo: nunca diz "exposto à internet". */
function ExposureDomain({ s, now }: { s: NetworkSecuritySnapshot; now: number }) {
  const [filter, setFilter] = useState<ExposureFilter>(DEFAULT_EXPOSURE_FILTER);
  const [showAll, setShowAll] = useState(false);
  const shown = filterListeners(s.exposure.listeners, filter);
  const visible = showAll ? shown : shown.slice(0, MAX_VISIBLE);
  const counts = s.exposure.counts;
  return (
    <Domain title="Exposição (portas em escuta)" section={s.exposure} now={now} summary={exposureSummary(s)}>
      <dl className="facts">
        <div><dt>Total</dt><dd>{counts.total}</dd></div>
        <div><dt>{LISTENER_SCOPE_LABEL.all_interfaces}</dt><dd>{counts.allInterfaces}</dd></div>
        <div><dt>{LISTENER_SCOPE_LABEL.specific}</dt><dd>{counts.specific}</dd></div>
        <div><dt>{LISTENER_SCOPE_LABEL.loopback}</dt><dd>{counts.loopback}</dd></div>
        {counts.unidentified > 0 && <div><dt>Dono não identificado</dt><dd>{counts.unidentified}</dd></div>}
      </dl>
      <p className="muted">
        “Todas as interfaces” (0.0.0.0 ou ::) não significa exposição à internet: depende do firewall e do roteador, que o LKR LAB não testa. Nenhuma varredura de portas é feita.
      </p>
      <div className="mh-net-filters">
        <input
          type="search"
          aria-label="Buscar porta, processo ou projeto"
          placeholder="Buscar porta, processo ou projeto"
          value={filter.query}
          onChange={(e) => { setFilter({ ...filter, query: e.target.value }); setShowAll(false); }}
        />
        <select
          aria-label="Filtrar por escopo"
          value={filter.scope}
          onChange={(e) => { setFilter({ ...filter, scope: e.target.value as ScopeFilter }); setShowAll(false); }}
        >
          {SCOPE_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
        </select>
      </div>
      {shown.length === 0 ? (
        <p className="muted" role="status">{s.exposure.listeners.length === 0 ? "Nenhuma porta em escuta." : "Nenhuma porta corresponde ao filtro."}</p>
      ) : (
        <ul className="mh-win-list" aria-label="Portas em escuta">
          {visible.map((l) => (
            <li key={`${l.address}-${l.port}-${l.pid ?? "x"}`}>
              <span>
                <strong className="mono">{listenerAddress(l)}</strong>
                <small className="muted"> · {LISTENER_SCOPE_LABEL[l.scope]}</small>
                {l.projectName && <Badge tone="good">{l.projectName}</Badge>}
              </span>
              <span>
                {listenerOwner(l)}
                {l.executable && <small className="muted mono"> · {l.executable}</small>}
              </span>
              <small className="muted">{l.note}</small>
            </li>
          ))}
        </ul>
      )}
      {shown.length > MAX_VISIBLE && !showAll && (
        <button type="button" className="button" onClick={() => setShowAll(true)}>Mostrar todas ({shown.length})</button>
      )}
      {s.exposure.notes.map((note) => <p className="muted" key={note}>{note}</p>)}
    </Domain>
  );
}

const CONNECTION_OPTIONS: { value: ConnectionFilter; label: string }[] = [
  { value: "remote", label: "Remotas" },
  { value: "local", label: "Rede local e loopback" },
  { value: "all", label: "Todas" },
];

/** Conexões TCP estabelecidas (visão compacta). Estado da máquina: nunca sai daqui. */
function ConnectionsDomain({ s, now }: { s: NetworkSecuritySnapshot; now: number }) {
  const [filter, setFilter] = useState<ConnectionFilter>("remote");
  const shown = filterConnections(s.connections.items, filter);
  const visible = shown.slice(0, MAX_VISIBLE);
  return (
    <Domain title="Conexões estabelecidas" section={s.connections} now={now} summary={s.connections.status === "unknown" && s.connections.sources.some((n) => n.state !== "available") ? undefined : `${s.connections.total} conexões · ${s.connections.remote} remotas`}>
      <dl className="facts">
        <div><dt>Total</dt><dd>{s.connections.total}</dd></div>
        <div><dt>{REMOTE_SCOPE_LABEL.remote}</dt><dd>{s.connections.remote}</dd></div>
        <div><dt>{REMOTE_SCOPE_LABEL.local}</dt><dd>{s.connections.local}</dd></div>
        <div><dt>{REMOTE_SCOPE_LABEL.loopback}</dt><dd>{s.connections.loopback}</dd></div>
      </dl>
      <div className="mh-net-filters">
        <select aria-label="Filtrar conexões" value={filter} onChange={(e) => setFilter(e.target.value as ConnectionFilter)}>
          {CONNECTION_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
        </select>
      </div>
      {shown.length === 0 ? (
        <p className="muted" role="status">{s.connections.total === 0 ? "Nenhuma conexão estabelecida." : "Nenhuma conexão neste filtro."}</p>
      ) : (
        <ul className="mh-win-list" aria-label="Conexões">
          {visible.map((c) => (
            <li key={`${c.localAddress}-${c.localPort}-${c.remoteAddress}-${c.remotePort}`}>
              <span>
                <strong className="mono">{c.remoteAddress}:{c.remotePort}</strong>
                <small className="muted"> · {REMOTE_SCOPE_LABEL[c.scope]}</small>
                {c.projectName && <Badge tone="good">{c.projectName}</Badge>}
              </span>
              <span>{c.processName ? `${c.processName}${c.pid != null ? ` (PID ${c.pid})` : ""}` : "Processo não identificado"}</span>
            </li>
          ))}
        </ul>
      )}
      {(shown.length > MAX_VISIBLE || s.connections.truncated) && (
        <p className="muted">Mostrando {visible.length} de {shown.length}{s.connections.truncated ? " (leitura limitada às mais relevantes)" : ""}.</p>
      )}
      {s.connections.notes.map((note) => <p className="muted" key={note}>{note}</p>)}
    </Domain>
  );
}
