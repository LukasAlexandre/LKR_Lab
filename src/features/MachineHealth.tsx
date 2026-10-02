import { useState } from "react";
import type { ReactNode } from "react";
import {
  Activity,
  AlertTriangle,
  ArrowDown,
  ArrowUp,
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
  LEVEL_LABEL,
  PROCESS_TABS,
  UNAVAILABLE,
  available,
  formatBits,
  formatCelsius,
  formatClock,
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
import type { ProcessMetric, Telemetry, TelemetryPoint } from "../shared/types";
import { useMachine } from "../state/machine";
import { refreshTelemetry, useTelemetry, useTelemetryWatch } from "../state/telemetry";

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

/** Concept 02 — Dashboard da Máquina / Machine Health. */
export function MachineHealth() {
  useTelemetryWatch();
  const { latest: t, history } = useTelemetry();
  const machine = useMachine();
  const [tab, setTab] = useState<ProcessMetric>(DEFAULT_PROCESS_TAB);
  const name = machine.status?.machine?.name ?? "Este computador";
  const snapshot = machine.status?.snapshot ?? null;

  const refresh = () => {
    void machine.refresh(true);
    void refreshTelemetry().catch(() => undefined);
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
            <Ring value={t.cpu.usage} label="CPU" detail={formatClock(t.cpu.clockMhz) ?? "Uso atual"} history={series(history, "cpu")} tone="blue" />
            <Ring value={t.memory.percent} label="Memória" detail={`${formatSize(t.memory.used)} / ${formatSize(t.memory.total)}`} history={series(history, "memory")} tone="green" />
            <Ring
              value={available(caps.diskActivity) ? t.diskIo.activity : null}
              label="Disco I/O"
              detail={`${diskLabel(t.diskIo.busiestDisk) ?? "Total"} · L ${formatRate(t.diskIo.readPerSec)} · E ${formatRate(t.diskIo.writePerSec)}`}
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
          {t.gpus.length > 0 && (
            <table className="mh-gpus" aria-label="GPUs">
              <thead>
                <tr><th>GPU</th><th>Uso</th><th>Memória dedicada</th><th>Memória compartilhada</th><th>Temperatura</th></tr>
              </thead>
              <tbody>
                {t.gpus.map((gpu) => {
                  const memory = gpuMemory(gpu);
                  return (
                    <tr key={gpu.name}>
                      <td>{gpu.name}</td>
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
          <p className="footnote">Dedicada = segmento informado pelo driver; em GPU integrada é pequeno e a GPU usa a memória compartilhada do sistema.</p>
        </section>

        <section className="panel">
          <div className="panel-title"><h2><Thermometer size={17} />Temperaturas</h2></div>
          <div className="mh-temps">
            {t.temperatures.map((reading) => {
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
          <p className="footnote">Só sensores expostos pelo Windows. Temperatura do pacote da CPU e da placa-mãe não têm fonte sem drivers de terceiros; o sensor ACPI não é a CPU e não entra na saúde.</p>
        </section>

        <section className="panel">
          <div className="panel-title"><h2><Wifi size={17} />Rede</h2></div>
          <div className="mh-network">
            <div>
              <small><ArrowDown size={13} /> Download</small>
              <strong>{formatBits(t.network.downloadBps)}</strong>
              <Spark values={series(history, "downloadBps")} max={0} tone="blue" />
            </div>
            <div>
              <small><ArrowUp size={13} /> Upload</small>
              <strong>{formatBits(t.network.uploadBps)}</strong>
              <Spark values={series(history, "uploadBps")} max={0} tone="purple" />
            </div>
          </div>
          <dl className="mh-network-facts">
            <div><dt>IP local</dt><dd>{t.network.ipv4 ?? UNAVAILABLE}</dd></div>
            <div><dt>Interface</dt><dd>{t.network.interface ?? UNAVAILABLE}</dd></div>
          </dl>
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
          <div className="panel-title"><h2><HardDrive size={17} />Discos e armazenamento</h2></div>
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
          <p className="footnote">Capacidade dos volumes. Saúde física (SMART): {UNAVAILABLE.toLowerCase()}.</p>
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
          <section className="panel">
            <div className="panel-title"><h2><Info size={17} />Informações adicionais</h2></div>
            <dl className="facts">
              <div><dt>Hostname</dt><dd>{snapshot?.hostname ?? UNAVAILABLE}</dd></div>
              <div><dt>Inicialização</dt><dd>{t.bootTime ? formatDateTime(t.bootTime * 1000) : UNAVAILABLE}</dd></div>
              <div><dt>Inventário detectado</dt><dd>{machine.status?.machine?.lastDetectedAt ? formatDateTime(machine.status.machine.lastDetectedAt) : UNAVAILABLE}</dd></div>
              <div><dt>Processos</dt><dd>{t.processes?.total ?? "—"}</dd></div>
              <div><dt>LKR LAB</dt><dd>v0.1.0</dd></div>
            </dl>
          </section>
        </div>
      </div>
    </>
  );
}
