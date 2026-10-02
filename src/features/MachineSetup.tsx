import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import {
  BookOpen,
  Box,
  Briefcase,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  CircleHelp,
  Clock,
  Cpu,
  FileText,
  FolderOpen,
  GitBranch,
  HardDrive,
  Home,
  Info,
  LayoutDashboard,
  LockOpen,
  MapPin,
  MemoryStick,
  Monitor,
  MonitorSmartphone,
  Network,
  RefreshCw,
  Search,
  Settings,
  Terminal,
  Workflow,
} from "lucide-react";
import {
  MACHINE_DESCRIPTION_MAX,
  MACHINE_NAME_MAX,
  USAGE_OPTIONS,
  cpuDetail,
  detectionAge,
  formatBytes,
  formatDateTime,
  formatMemory,
  osDetail,
  suggestedName,
  validateMachineName,
} from "../shared/machine";
import type { MachineSnapshot, MachineUsage } from "../shared/types";
import type { MachineRegistry } from "../state/machine";
import { usePreference } from "../shared/preferences";
import { Sidebar } from "../shell/Sidebar";

/** O que existe hoje e é liberado pelo cadastro (módulos reais, sem promessas). */
const UNLOCKS: { icon: ReactNode; label: string }[] = [
  { icon: <LayoutDashboard size={18} />, label: "Dashboard" },
  { icon: <FolderOpen size={18} />, label: "Projetos locais" },
  { icon: <GitBranch size={18} />, label: "Repositórios e Git" },
  { icon: <Box size={18} />, label: "Worktrees" },
  { icon: <Workflow size={18} />, label: "Runtime dos projetos" },
  { icon: <Network size={18} />, label: "Portas e Processos" },
  { icon: <FileText size={18} />, label: "Prompts e Conhecimento" },
  { icon: <Terminal size={18} />, label: "Terminal e Contexto IA" },
];

const usageIcon = (usage: MachineUsage) =>
  usage === "home" ? <Home size={15} /> : usage === "work" ? <Briefcase size={15} /> : <MapPin size={15} />;

function Fact({ icon, label, value, detail, busy }: {
  icon: ReactNode; label: string; value: string | null; detail?: string | null; busy: boolean;
}) {
  return (
    <div className="machine-fact">
      <span className="machine-fact-icon">{icon}</span>
      <div>
        <small>{label}</small>
        {busy && !value ? (
          <span className="machine-skeleton" aria-label="Detectando" />
        ) : (
          <strong className={value ? "" : "unavailable"}>{value ?? "Indisponível"}</strong>
        )}
        {detail && <span>{detail}</span>}
      </div>
    </div>
  );
}

function Facts({ snapshot, busy, now }: { snapshot: MachineSnapshot | null; busy: boolean; now: number }) {
  const s = snapshot;
  const gpu = s?.gpus[0] ?? null;
  const extraGpus = s && s.gpus.length > 1 ? ` +${s.gpus.length - 1}` : "";
  return (
    <div className="machine-facts">
      <Fact icon={<FolderOpen size={19} />} label="Hostname" value={s?.hostname ?? null} busy={busy} />
      <Fact icon={<Monitor size={19} />} label="Sistema" value={s?.osName ?? null} detail={s && osDetail(s)} busy={busy} />
      <Fact icon={<Cpu size={19} />} label="CPU" value={s?.cpuModel ?? null} detail={s && cpuDetail(s)} busy={busy} />
      <Fact icon={<MemoryStick size={19} />} label="Memória" value={formatMemory(s?.memoryTotal)} detail={s?.memoryTotal ? "RAM instalada" : null} busy={busy} />
      <Fact icon={<HardDrive size={19} />} label="GPU" value={gpu ? gpu.name + extraGpus : null} detail={gpu?.memory ? `${formatBytes(gpu.memory)} dedicada` : null} busy={busy} />
      <Fact icon={<Network size={19} />} label="IP local" value={s?.localIpv4 ?? null} detail={s?.localIpv4 ? "Rede local" : null} busy={busy} />
      <Fact icon={<MonitorSmartphone size={19} />} label="Interface ativa" value={s?.activeInterface ?? null} detail={s && s.networkInterfaces.length > 1 ? `${s.networkInterfaces.length} interfaces com IPv4` : null} busy={busy} />
      <Fact icon={<Clock size={19} />} label="Última detecção" value={s ? detectionAge(s.detectedAt, now) : null} detail={s ? formatDateTime(s.detectedAt) : null} busy={busy} />
    </div>
  );
}

/** Concept 01 — Primeiro acesso / Computador não cadastrado. */
export function MachineSetup({ registry }: { registry: MachineRegistry }) {
  const { phase, status, error, refresh, register } = registry;
  const snapshot = status?.snapshot ?? null;
  const [sidebarCompact, setSidebarCompact] = usePreference("sidebarCompact");
  const [name, setName] = useState("");
  const [touched, setTouched] = useState(false);
  const [usage, setUsage] = useState<MachineUsage>("home");
  const [description, setDescription] = useState("");
  const [now, setNow] = useState(() => Date.now());
  const detecting = phase === "loading" || phase === "detecting";
  const saving = phase === "saving";
  const preview = phase === "preview";

  // Sugere o hostname uma única vez; o que o usuário digitar nunca é sobrescrito.
  useEffect(() => {
    if (!touched && snapshot) setName((current) => current || suggestedName(snapshot));
  }, [snapshot, touched]);
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, []);
  useEffect(() => setNow(Date.now()), [snapshot]);

  const nameError = touched ? validateMachineName(name) : null;
  const canSubmit = !preview && !saving && !detecting && !!snapshot && !validateMachineName(name);
  const submit = () => {
    setTouched(true);
    if (!canSubmit) return;
    void register({ name: name.trim(), usage, description: description.trim() });
  };

  return (
    <div className={`app ${sidebarCompact ? "sidebar-compact" : ""} density-comfortable`}>
      <Sidebar route="setup" sidebarCompact={sidebarCompact} toggle={() => setSidebarCompact((v) => !v)} projectCount={0} locked />
      <div className="workspace">
        <header className="topbar">
          <div className="breadcrumb">
            Ambiente local <ChevronRight size={13} />
            <span>Configuração inicial</span>
          </div>
          <button className="search-trigger" disabled title="Disponível após o cadastro">
            <Search size={15} />
            <span>Buscar projetos e ações…</span>
            <kbd>Ctrl K</kbd>
          </button>
          <button className="project-context" disabled title="Disponível após o cadastro">
            <span className="status-orb idle" />
            <span>
              <strong>Nenhum projeto selecionado</strong>
              <small>Cadastre este computador para continuar</small>
            </span>
            <ChevronRight size={14} />
          </button>
          <div className="top-status">
            <span className={`dot ${preview ? "" : "amber"}`} />
            {preview ? "Prévia web" : "Nova máquina detectada"}
          </div>
          <div className="avatar">LK</div>
        </header>
        <main className="machine-setup">
          <div className="page-heading">
            <div>
              <div className="eyebrow">AMBIENTE LOCAL</div>
              <h1>Configure este computador</h1>
              <p>Antes de acessar projetos, sessões e ambientes, o LKR LAB precisa identificar esta workstation.</p>
            </div>
          </div>

          <div className="machine-lock-notice" role="note">
            <Info size={20} />
            <div>
              <strong>As funcionalidades do LKR LAB permanecerão bloqueadas até que este computador seja cadastrado.</strong>
              <span>Após o cadastro, você terá acesso a todos os módulos do ambiente de desenvolvimento.</span>
            </div>
          </div>

          {preview && (
            <div className="preview-banner">
              <CircleHelp size={16} />
              <div>
                <strong>Prévia da interface</strong> · A detecção e o cadastro do computador
                só existem no aplicativo desktop. Nenhum dado desta máquina é lido ou simulado aqui.
              </div>
            </div>
          )}

          <section className="panel machine-panel">
            <div className="machine-detected">
              <div className="machine-title">
                <Monitor size={26} />
                <h2>{preview ? "Detecção da máquina" : "Nova máquina detectada"}</h2>
              </div>
              <p className="machine-sub">
                {preview
                  ? "No desktop, o LKR LAB identifica automaticamente as informações desta workstation."
                  : detecting
                    ? "Identificando as informações desta workstation…"
                    : "O LKR LAB identificou automaticamente as informações desta workstation."}
              </p>
              <div className="machine-suggested">
                <span className="machine-fact-icon large"><Monitor size={22} /></span>
                <div>
                  <small>
                    Nome sugerido{" "}
                    {snapshot?.hostname && <span className="badge blue">DETECTADO</span>}
                  </small>
                  <strong>{suggestedName(snapshot) || (preview ? "Indisponível na prévia" : detecting ? "…" : "Este computador")}</strong>
                  <span>Você pode editar o nome antes de cadastrar.</span>
                </div>
              </div>
              <Facts snapshot={snapshot} busy={detecting} now={now} />
            </div>

            <form
              className="machine-form"
              onSubmit={(e) => {
                e.preventDefault();
                submit();
              }}
            >
              <div className="machine-form-title">
                <Settings size={18} />
                <div>
                  <h3>Confirme as informações</h3>
                  <p>Defina um nome e o local de uso para este computador.</p>
                </div>
              </div>
              <label>
                Nome deste computador
                <span className="input-icon">
                  <Monitor size={15} />
                  <input
                    value={name}
                    maxLength={MACHINE_NAME_MAX}
                    placeholder="Ex.: PC Casa"
                    disabled={preview || saving}
                    aria-invalid={!!nameError}
                    onChange={(e) => {
                      setTouched(true);
                      setName(e.target.value);
                    }}
                  />
                </span>
                <small className={nameError ? "field-error" : "field-hint"}>
                  {nameError ?? "Este nome será exibido no seu ambiente LKR LAB."}
                </small>
              </label>
              <label>
                Uso / Local
                <span className="input-icon select-icon">
                  {usageIcon(usage)}
                  <select value={usage} disabled={preview || saving} onChange={(e) => setUsage(e.target.value as MachineUsage)}>
                    {USAGE_OPTIONS.map((option) => (
                      <option key={option.value} value={option.value}>{option.label}</option>
                    ))}
                  </select>
                  <ChevronDown size={15} className="select-chevron" />
                </span>
                <small className="field-hint">Identifique onde este computador será utilizado.</small>
              </label>
              <label>
                Descrição <span className="optional">(opcional)</span>
                <textarea
                  rows={3}
                  value={description}
                  maxLength={MACHINE_DESCRIPTION_MAX}
                  disabled={preview || saving}
                  placeholder="Ex.: Computador principal para desenvolvimento pessoal…"
                  onChange={(e) => setDescription(e.target.value)}
                />
                <small className="field-counter">{description.length}/{MACHINE_DESCRIPTION_MAX}</small>
              </label>
              {error && (
                <div className="error" role="alert">
                  {error}
                </div>
              )}
              <div className="machine-actions">
                <button
                  type="button"
                  className="button"
                  disabled={preview || saving || detecting}
                  onClick={() => void refresh(true)}
                >
                  <RefreshCw size={15} className={detecting ? "spin" : ""} />
                  {detecting ? "Detectando…" : "Atualizar detecção"}
                </button>
                <button type="submit" className="button primary" disabled={!canSubmit}>
                  <CheckCircle2 size={15} />
                  {saving ? "Cadastrando…" : "Cadastrar computador"}
                </button>
              </div>
            </form>
          </section>

          <div className="machine-bottom">
            <section className="panel">
              <div className="machine-section-title">
                <LockOpen size={22} />
                <div>
                  <h3>O que será liberado</h3>
                  <p>Após o cadastro deste computador, os módulos do LKR LAB serão habilitados.</p>
                </div>
              </div>
              <div className="machine-unlocks">
                {UNLOCKS.map((item) => (
                  <div className="machine-unlock" key={item.label}>
                    {item.icon}
                    <span>{item.label}</span>
                  </div>
                ))}
              </div>
            </section>
            <section className="panel">
              <div className="machine-section-title">
                <BookOpen size={22} />
                <div>
                  <h3>Como funciona</h3>
                  <p>Processo rápido e local para liberar seu ambiente.</p>
                </div>
              </div>
              <ol className="machine-steps">
                <li>
                  <strong>Detectamos esta máquina</strong>
                  <span>O LKR LAB lê as informações de hardware e rede expostas pelo sistema, sem executar programas.</span>
                </li>
                <li>
                  <strong>Você confirma a identidade</strong>
                  <span>Defina um nome e o local de uso. IP e hostname são atributos, não a identidade.</span>
                </li>
                <li>
                  <strong>O ambiente é liberado</strong>
                  <span>O cadastro fica só neste computador e nunca é sincronizado.</span>
                </li>
              </ol>
            </section>
          </div>
        </main>
      </div>
    </div>
  );
}
