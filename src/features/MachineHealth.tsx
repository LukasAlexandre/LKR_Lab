import { useState } from "react";
import type { ReactNode } from "react";
import {
  Activity,
  AlertTriangle,
  ArrowDown,
  ArrowUp,
  Battery,
  BatteryCharging,
  CheckCircle2,
  Clock,
  Cpu,
  Gauge,
  HardDrive,
  HeartPulse,
  Info,
  LayoutGrid,
  MemoryStick,
  Monitor,
  RefreshCw,
  Thermometer,
  Wifi,
  XCircle,
} from "lucide-react";
import { desktop } from "../shared/api";
import { cpuDetail, formatDateTime, osDetail } from "../shared/machine";
import {
  DEFAULT_PROCESS_TAB,
  HEALTH_DETAIL,
  HEALTH_LABEL,
  INTERFACE_KIND,
  LEVEL_LABEL,
  PROCESS_TABS,
  UNAVAILABLE,
  WARMING,
  available,
  batteryStatus,
  coreBars,
  cpuClockDetail,
  diskRows,
  domainNote,
  gpuIdentity,
  memoryVirtual,
  rateText,
  splitInterfaces,
  splitTemperatures,
  formatBits,
  formatCelsius,
  formatRate,
  formatSize,
  formatUptime,
  gpuMemory,
  metricValue,
  pct,
  processMemory,
  processRows,
  share,
  diskLabel,
  sparkline,
  volumeUsage,
} from "../shared/telemetry";
import type { Domain, ProcessMetric, Telemetry, TelemetryPoint } from "../shared/types";
import { useMachine } from "../state/machine";
import { refreshTelemetry, useTelemetry, useTelemetryWatch } from "../state/telemetry";
import { refreshWindowsHealth, useWindowsHealth } from "../state/windowsHealth";
import { WindowsHealthPanel } from "./WindowsHealthPanel";
import { NetworkSecurityPanel } from "./NetworkSecurityPanel";
import { AlertsPanel, AlertsSummaryStrip } from "./AlertsPanel";
import { loadAlerts, useAlerts } from "../state/alerts";
import { refreshNetworkSecurity, useNetworkSecurity } from "../state/networkSecurity";

const STATUS_TONE = { healthy: "good", attention: "warn", critical: "danger" } as const;

function Spark({ values, max = 100, tone = "blue" }: { values: (number | null)[]; max?: number; tone?: string }) {
  const path = sparkline(values, 120, 28, max);
  return (
    <svg className={`mh-spark ${tone}`} viewBox="0 0 120 28" preserveAspectRatio="none" aria-hidden="true">
      {path && <path d={path} />}
    </svg>
  );
}

function Ring({ value, label, detail, history, tone }: {
  value: number | null; label: string; detail: string; history: (number | null)[]; tone: string;
}) {
  const radius = 34;
  const length = 2 * Math.PI * radius;
  const filled = value == null ? 0 : (Math.min(Math.max(value, 0), 100) / 100) * length;
  return (
    <div className="mh-ring-card">
      <svg className={`mh-ring ${tone}`} viewBox="0 0 84 84" role="img" aria-label={`${label}: ${pct(value) ?? UNAVAILABLE}`}>
        <circle className="track" cx="42" cy="42" r={radius} />
        <circle className="value" cx="42" cy="42" r={radius} strokeDasharray={`${filled} ${length}`} />
        <text x="42" y="47" textAnchor="middle">{pct(value) ?? "—"}</text>
      </svg>
      <strong>{label}</strong>
      <small>{detail}</small>
      <Spark values={history} tone={tone} />
    </div>
  );
}

function Summary({ icon, label, value, detail }: { icon: ReactNode; label: string; value: string | null; detail?: string | null }) {
  return (
    <div className="mh-summary-item">
      <span className="mh-icon">{icon}</span>
      <div>
        <small>{label}</small>
        <strong className={value ? "" : "unavailable"}>{value ?? UNAVAILABLE}</strong>
        {detail && <span>{detail}</span>}
      </div>
    </div>
  );
}

const series = (history: TelemetryPoint[], key: keyof TelemetryPoint) => history.map((p) => p[key] as number | null);

/** Selo discreto só quando o domínio é parcial/indisponível: a máquina não entrega tudo, e isso não é falha. */
function DomainTag({ domain }: { domain: Domain | undefined }) {
  const note = domainNote(domain);
  return note ? <span className="mh-domain" title="O que esta máquina consegue medir; ausência de sensor não é falha.">{note}</span> : null;
}

/** Concept 02 — Dashboard da Máquina / Machine Health. */
export function MachineHealth() {
  useTelemetryWatch();
  const { latest: t, history } = useTelemetry();
  const windowsHealth = useWindowsHealth();
  const networkSecurity = useNetworkSecurity();
  const alerts = useAlerts();
  const machine = useMachine();
  const [tab, setTab] = useState<ProcessMetric>(DEFAULT_PROCESS_TAB);
  const name = machine.status?.machine?.name ?? "Este computador";
  const snapshot = machine.status?.snapshot ?? null;

  const refresh = () => {
    void machine.refresh(true);
    void refreshTelemetry().catch(() => undefined);
    void refreshWindowsHealth();
    void refreshNetworkSecurity();
    void loadAlerts();
  };

  const header = (
    <div className="page-heading mh-heading">
      <div>
        <div className="eyebrow">AMBIENTE LOCAL</div>
        <h1>{name}</h1>
        <p>Visão em tempo real da saúde e dos recursos desta workstation.</p>
      </div>
      <div className="heading-actions">
        {t && (
          <span className="mh-updated">
            <span className={`dot ${t.active ? "green" : ""}`} />
            <span>
              {t.active ? "Atualizando ao vivo" : "Atualização reduzida"}
              <small>{new Date(t.timestamp).toLocaleTimeString("pt-BR")}</small>
            </span>
          </span>
        )}
        <button className="button primary" disabled={!desktop || machine.refreshing} onClick={refresh} title="Nova detecção do inventário e amostra imediata da telemetria">
          <RefreshCw size={15} className={machine.refreshing ? "spin" : ""} />
          Atualizar agora
        </button>
      </div>
    </div>
  );

  if (!desktop || !t)
    return (
      <>
        {header}
        <div className="panel mh-waiting" role="status">
          <Activity size={22} />
          {desktop ? "Coletando a telemetria desta máquina…" : "Telemetria disponível no aplicativo desktop."}
        </div>
      </>
    );

  const caps = t.capabilities;
  const discrete = [...t.gpus].sort((a, b) => (b.dedicatedTotal ?? 0) - (a.dedicatedTotal ?? 0))[0];
  const busiestGpu = t.gpus.reduce<Telemetry["gpus"][number] | undefined>(
    (best, gpu) => (gpu.usage != null && (best?.usage == null || gpu.usage > best.usage) ? gpu : best),
    undefined,
  );
  const fixed = t.volumes.filter((v) => !v.removable);
  const storageTotal = fixed.reduce((sum, v) => sum + v.total, 0);
  const { rows, empty } = processRows(t, tab);
  const top = rows.length ? Math.max(...rows.map((r) => metricValue(r, tab)), 1e-9) : 1;
  const status = t.health.status;

  return (
    <>
      {header}
      <AlertsSummaryStrip state={alerts} />
      <section className="panel mh-identity">
        <div className="mh-machine">
          <div className="mh-machine-art"><Monitor size={40} /></div>
          <div>
            <h2>{name} <span className="badge good">Este computador</span></h2>
            <p className="mono">{snapshot?.hostname ?? UNAVAILABLE}</p>
            <p>
              <strong>{snapshot?.osName ?? UNAVAILABLE}</strong>
              {snapshot && osDetail(snapshot) && <small>{osDetail(snapshot)}</small>}
            </p>
          </div>
        </div>
        <div className="mh-summary">
          <Summary icon={<Cpu size={20} />} label="CPU" value={snapshot?.cpuModel ?? null} detail={snapshot && cpuDetail(snapshot)} />
          <Summary
            icon={<LayoutGrid size={20} />}
            label="GPU"
            value={discrete?.name ?? null}
            detail={discrete?.dedicatedTotal ? `${formatSize(discrete.dedicatedTotal)} dedicada${t.gpus.length > 1 ? ` · ${t.gpus.length} GPUs` : ""}` : t.gpus.length > 1 ? `${t.gpus.length} GPUs` : null}
          />
          <Summary icon={<MemoryStick size={20} />} label="Memória" value={formatSize(t.memory.total)} detail="RAM utilizável" />
          <Summary icon={<HardDrive size={20} />} label="Armazenamento" value={storageTotal ? formatSize(storageTotal) : null} detail={fixed.length ? `${fixed.length} ${fixed.length === 1 ? "volume" : "volumes"}` : null} />
          <Summary icon={<Clock size={20} />} label="Uptime" value={formatUptime(t.uptime)} />
          <div className="mh-status">
            <small>Status do sistema</small>
            <strong className={`mh-status-${status}`}><span className={`dot ${STATUS_TONE[status]}`} />{HEALTH_LABEL[status]}</strong>
            <span>{HEALTH_DETAIL[status]}</span>
          </div>
        </div>
      </section>

      <div className="mh-row mh-row-top">
        <section className="panel">
          <div className="panel-title"><h2><Gauge size={17} />Utilização de recursos</h2></div>
          <div className="mh-rings">
            <Ring value={t.cpu.ready ? t.cpu.usage : null} label="CPU" detail={cpuClockDetail(t.cpu)} history={series(history, "cpu")} tone="blue" />
            <Ring value={t.memory.percent} label="Memória" detail={`${formatSize(t.memory.used)} / ${formatSize(t.memory.total)}`} history={series(history, "memory")} tone="green" />
            <Ring
              value={available(caps.diskActivity) ? t.diskIo.activity : null}
              label="Disco I/O"
              detail={t.diskIo.ready ? `${diskLabel(t.diskIo.busiestDisk) ?? "Total"} · L ${formatRate(t.diskIo.readPerSec)} · E ${formatRate(t.diskIo.writePerSec)}` : WARMING}
              history={series(history, "disk")}
              tone="orange"
            />
            <Ring
              value={available(caps.gpuUsage) ? (busiestGpu?.usage ?? null) : null}
              label="GPU"
              detail={available(caps.gpuUsage) ? (busiestGpu?.name ?? "Uso atual") : UNAVAILABLE}
              history={series(history, "gpu")}
              tone="purple"
            />
          </div>
          <div className="mh-detail-grid">
            <div className="mh-cores" aria-label="Uso por núcleo">
              <div className="row spread">
                <strong>Núcleos <DomainTag domain={t.availability.cpu} /></strong>
                <small className="muted">{t.cpu.cores.length ? `${t.cpu.cores.length} processadores lógicos` : UNAVAILABLE}</small>
              </div>
              {t.cpu.ready && t.cpu.cores.length > 0 ? (
                <div className="mh-core-bars">
                  {coreBars(t.cpu.cores).map((core) => (
                    <span key={core.index} className="mh-core" title={`Núcleo ${core.index}: ${Math.round(core.percent)}%`}>
                      <i style={{ height: `${core.percent}%` }} />
                    </span>
                  ))}
                </div>
              ) : (
                <p className="muted mh-empty">{t.cpu.ready ? UNAVAILABLE : WARMING}</p>
              )}
            </div>
            <dl className="mh-memory-facts" aria-label="Memória">
              <div><dt>RAM em uso</dt><dd>{formatSize(t.memory.used)} / {formatSize(t.memory.total)}</dd></div>
              <div><dt>RAM disponível</dt><dd>{formatSize(t.memory.available)}</dd></div>
              <div><dt>Commit (RAM + pagefile)</dt><dd>{memoryVirtual(t.memory).commit ?? UNAVAILABLE}</dd></div>
              <div><dt>Pagefile em uso</dt><dd>{memoryVirtual(t.memory).pagefile ?? UNAVAILABLE}</dd></div>
            </dl>
          </div>
          {t.gpus.length > 0 && (
            <table className="mh-gpus" aria-label="GPUs">
              <thead>
                <tr><th>GPU</th><th>Uso</th><th>Memória dedicada</th><th>Memória compartilhada</th><th>Temperatura</th></tr>
              </thead>
              <tbody>
                {t.gpus.map((gpu) => {
                  const memory = gpuMemory(gpu);
                  return (
                    <tr key={gpu.id}>
                      <td>
                        {gpu.name}
                        {gpuIdentity(gpu) && <small className="mh-gpu-id">{gpuIdentity(gpu)}</small>}
                      </td>
                      <td>{available(gpu.capabilities.usage) ? pct(gpu.usage) : UNAVAILABLE}</td>
                      <td>{memory.dedicated ?? UNAVAILABLE}</td>
                      <td>{memory.shared ?? UNAVAILABLE}</td>
                      <td>{formatCelsius(gpu.temperature) ?? UNAVAILABLE}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
          <p className="footnote">Dedicada = segmento informado pelo driver; em GPU integrada é pequeno e a GPU usa a memória compartilhada do sistema. RAM, commit e pagefile são memórias diferentes: o commit inclui a memória virtual prometida aos processos.</p>
        </section>

        <section className="panel">
          <div className="panel-title"><h2><Thermometer size={17} />Temperaturas</h2><DomainTag domain={t.availability.temperatures} /></div>
          <div className="mh-temps">
            {splitTemperatures(t.temperatures).found.map((reading) => {
              const limit = reading.critical ?? 100;
              const title = reading.level === "unrated"
                ? "Leitura do firmware sem limite conhecido: exibida, não avaliada na saúde."
                : reading.warning ? `Aviso ${reading.warning} °C · crítico ${reading.critical ?? "—"} °C` : undefined;
              return (
                <div className={`mh-temp level-${reading.level}`} key={reading.id} title={title}>
                  <span className="mh-temp-label">{reading.label}</span>
                  <strong>{formatCelsius(reading.celsius) ?? UNAVAILABLE}</strong>
                  <span className="mh-bar"><i style={{ width: `${reading.celsius == null ? 0 : Math.min((reading.celsius / limit) * 100, 100)}%` }} /></span>
                  <small>{reading.level === "unavailable" ? "" : LEVEL_LABEL[reading.level]}</small>
                </div>
              );
            })}
          </div>
          {splitTemperatures(t.temperatures).missing.length > 0 && (
            <p className="muted mh-missing" aria-label="Sensores sem leitura">
              {UNAVAILABLE} nesta máquina: {splitTemperatures(t.temperatures).missing.join(" · ")}.
            </p>
          )}
          <p className="footnote">Só sensores expostos pelo Windows. Temperatura do pacote da CPU e da placa-mãe não têm fonte sem drivers de terceiros; o sensor ACPI não é a CPU e não entra na saúde.</p>
        </section>

        <section className="panel">
          <div className="panel-title"><h2><Wifi size={17} />Rede</h2><DomainTag domain={t.availability.network} /></div>
          <div className="mh-network">
            <div>
              <small><ArrowDown size={13} /> Download</small>
              <strong>{rateText(t.network.downloadBps, t.network.ready, formatBits)}</strong>
              <Spark values={series(history, "downloadBps")} max={0} tone="blue" />
            </div>
            <div>
              <small><ArrowUp size={13} /> Upload</small>
              <strong>{rateText(t.network.uploadBps, t.network.ready, formatBits)}</strong>
              <Spark values={series(history, "uploadBps")} max={0} tone="purple" />
            </div>
          </div>
          <dl className="mh-network-facts">
            <div><dt>IP local</dt><dd>{t.network.ipv4 ?? UNAVAILABLE}</dd></div>
            <div><dt>Interface</dt><dd>{t.network.interface ?? UNAVAILABLE}</dd></div>
          </dl>
          {t.network.interfaces.length > 0 && (() => {
            const { connected, idle } = splitInterfaces(t.network.interfaces);
            const row = (i: Telemetry["network"]["interfaces"][number]) => (
              <li key={i.name} className={i.active ? "active" : ""}>
                <div className="row spread">
                  <strong>{i.name}</strong>
                  <small className="muted">{INTERFACE_KIND[i.kind] ?? INTERFACE_KIND.other}{i.active ? " · ativa" : ""}</small>
                </div>
                <div className="row spread muted">
                  <small>{i.ipv4[0] ?? i.ipv6[0] ?? "Sem endereço"}{i.ipv6.length && i.ipv4.length ? " · IPv6" : ""}</small>
                  <small>{i.up === true ? (formatBits(i.linkSpeedBps) ?? UNAVAILABLE) : i.up === false ? "Desconectada" : UNAVAILABLE}</small>
                </div>
                {i.up === true && (
                  <small className="mh-iface-rate">
                    ↓ {rateText(i.downloadBps, t.network.ready, formatBits)} · ↑ {rateText(i.uploadBps, t.network.ready, formatBits)}
                  </small>
                )}
              </li>
            );
            return (
              <>
                <ul className="mh-ifaces" aria-label="Interfaces de rede">{connected.map(row)}</ul>
                {idle.length > 0 && (
                  <details className="mh-ifaces-idle">
                    <summary>{idle.length} {idle.length === 1 ? "interface desconectada" : "interfaces desconectadas"}</summary>
                    <ul className="mh-ifaces">{idle.map(row)}</ul>
                  </details>
                )}
              </>
            );
          })()}
        </section>
      </div>

      <div className="mh-row mh-row-bottom">
        <section className="panel mh-processes">
          <div className="panel-title">
            <h2><Activity size={17} />Processos em destaque</h2>
            <div className="segmented" role="tablist" aria-label="Ordenar processos por">
              {PROCESS_TABS.map((option) => (
                <button key={option.id} role="tab" aria-selected={tab === option.id} className={tab === option.id ? "active" : ""} onClick={() => setTab(option.id)}>
                  {option.label}
                </button>
              ))}
            </div>
          </div>
          {empty ? (
            <p className="muted mh-empty">{empty}</p>
          ) : (
            <table className="mh-table">
              <thead>
                <tr>
                  <th>Processo</th>
                  <th className={tab === "cpu" ? "sorted" : ""}>CPU</th>
                  <th className={tab === "memory" ? "sorted" : ""}>Memória</th>
                  <th className={tab === "gpu" ? "sorted" : ""}>GPU</th>
                  <th className={tab === "disk" ? "sorted" : ""}>Disco (L / E)</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => (
                  <tr key={row.pid}>
                    <td>
                      <span className="mh-process">{row.name}</span>
                      <small className="mono">PID {row.pid}</small>
                      <span className="mh-bar"><i style={{ width: `${(metricValue(row, tab) / top) * 100}%` }} /></span>
                    </td>
                    <td>{share(row.cpu)}</td>
                    <td title={row.memory ? undefined : "Processo protegido: o Windows não informa a memória"}>{processMemory(row.memory) ?? "—"}</td>
                    <td>{share(row.gpu)}</td>
                    <td>{formatRate(row.diskRead)} / {formatRate(row.diskWrite)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <p className="footnote">Valores do momento, por processo. Disco = E/S de arquivo do processo, como no Gerenciador de Tarefas.</p>
        </section>

        <section className="panel">
          <div className="panel-title"><h2><HardDrive size={17} />Discos e armazenamento</h2><DomainTag domain={t.availability.disk} /></div>
          {diskRows(t).length > 0 && (
            <ul className="mh-devices" aria-label="Discos físicos">
              {diskRows(t).map(({ device, label, volumes, total, available: free }) => (
                <li key={device.instance}>
                  <div className="row spread">
                    <strong>{label}</strong>
                    <small className="muted">{device.nvme ? "NVMe" : "Disco"} · {device.volumes.join(" ") || "sem letra"}{total ? ` · ${formatSize(total)}` : ""}</small>
                  </div>
                  <div className="row spread muted">
                    <small>L {rateText(device.readPerSec, t.diskIo.ready, formatRate)} · E {rateText(device.writePerSec, t.diskIo.ready, formatRate)}</small>
                    <small>{device.readOpsPerSec != null && device.writeOpsPerSec != null ? `${Math.round(device.readOpsPerSec)} / ${Math.round(device.writeOpsPerSec)} ops/s` : "ops/s —"} · {pct(device.activity) ?? "—"} ativo</small>
                  </div>
                  {volumes.length > 0 && <small className="muted">{formatSize(free)} livres de {formatSize(total)}</small>}
                </li>
              ))}
            </ul>
          )}
          <div className="mh-volumes">
            {t.volumes.map((volume) => {
              const usage = volumeUsage(volume.total, volume.available);
              return (
                <div className="mh-volume" key={volume.mount}>
                  <div className="row spread">
                    <strong>{volume.mount}</strong>
                    <span>{formatSize(volume.available)} livres</span>
                  </div>
                  <div className="row spread muted">
                    <small>{volume.kind === "unknown" ? "Tipo não informado" : volume.kind.toUpperCase()}{volume.removable ? " · removível" : ""}</small>
                    <small>{formatSize(usage.used)} / {formatSize(volume.total)}</small>
                  </div>
                  <span className={`mh-bar ${usage.percent >= 95 ? "critical" : usage.percent >= 90 ? "attention" : ""}`}><i style={{ width: `${usage.percent}%` }} /></span>
                </div>
              );
            })}
          </div>
          <p className="footnote">Capacidade dos volumes (acima, a atividade de cada disco físico). Saúde física (SMART): {UNAVAILABLE.toLowerCase()}.</p>
        </section>

        <div className="mh-side">
          <section className="panel">
            <div className="panel-title"><h2><HeartPulse size={17} />Saúde e alertas</h2></div>
            <ul className="mh-checks">
              {t.health.checks.map((check) => (
                <li key={check.id}>
                  {check.ok ? <CheckCircle2 size={16} className="ok" /> : <XCircle size={16} className="bad" />}
                  {check.label}
                </li>
              ))}
            </ul>
            {t.health.alerts.map((alert) => (
              <div className={`mh-alert ${alert.severity}`} key={`${alert.source}-${alert.title}`} role="alert">
                <AlertTriangle size={15} />
                <div><strong>{alert.title}</strong><span>{alert.detail}</span></div>
              </div>
            ))}
          </section>
          {t.battery.present && (() => {
            const battery = batteryStatus(t.battery);
            return (
              <section className="panel mh-battery" aria-label="Bateria">
                <div className="panel-title">
                  <h2>{t.battery.charging ? <BatteryCharging size={17} /> : <Battery size={17} />}Bateria e energia</h2>
                </div>
                <div className="mh-battery-level">
                  <strong>{battery.percent ?? UNAVAILABLE}</strong>
                  <span className="mh-bar"><i style={{ width: `${t.battery.percent ?? 0}%` }} /></span>
                </div>
                <dl className="facts">
                  <div><dt>Estado</dt><dd>{battery.state}</dd></div>
                  <div><dt>Tomada</dt><dd>{t.battery.acOnline == null ? UNAVAILABLE : t.battery.acOnline ? "Conectada" : "Desconectada"}</dd></div>
                  {battery.remaining && <div><dt>Autonomia</dt><dd>{battery.remaining}</dd></div>}
                  <div><dt>Saúde da bateria</dt><dd title="O Windows não informa capacidade de projeto/carga cheia sem driver ou elevação.">{UNAVAILABLE}</dd></div>
                </dl>
              </section>
            );
          })()}
          <section className="panel">
            <div className="panel-title"><h2><Info size={17} />Informações adicionais</h2></div>
            <dl className="facts">
              <div><dt>Hostname</dt><dd>{snapshot?.hostname ?? UNAVAILABLE}</dd></div>
              <div><dt>Inicialização</dt><dd>{t.bootTime ? formatDateTime(t.bootTime * 1000) : UNAVAILABLE}</dd></div>
              <div><dt>Inventário detectado</dt><dd>{machine.status?.machine?.lastDetectedAt ? formatDateTime(machine.status.machine.lastDetectedAt) : UNAVAILABLE}</dd></div>
              <div><dt>Processos</dt><dd>{t.processes?.total ?? "—"}</dd></div>
              {!t.battery.present && <div><dt>Bateria</dt><dd>Não aplicável</dd></div>}
              <div><dt>LKR LAB</dt><dd>v0.1.0</dd></div>
            </dl>
          </section>
        </div>
      </div>

      <AlertsPanel state={alerts} now={Date.now()} />
      <WindowsHealthPanel state={windowsHealth} now={Date.now()} />
      <NetworkSecurityPanel state={networkSecurity} now={Date.now()} />
    </>
  );
}
