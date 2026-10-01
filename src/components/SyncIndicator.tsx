import { useEffect } from "react";
import { ArrowDown, Check, CircleAlert, CircleDot, CloudOff, LoaderCircle, TriangleAlert } from "lucide-react";
import { desktop } from "../shared/api";
import { run, start, useSyncView, type SyncStateName } from "../state/sync";

const LOOK: Record<SyncStateName, { label: string; Icon: typeof Check; tone: string }> = {
  clean: { label: "Sincronizado", Icon: Check, tone: "good" },
  local_dirty: { label: "Alterações locais", Icon: CircleDot, tone: "warn" },
  remote_changed: { label: "Atualização disponível", Icon: ArrowDown, tone: "info" },
  diverged: { label: "Conflito de sincronização", Icon: TriangleAlert, tone: "danger" },
  offline: { label: "Offline", Icon: CloudOff, tone: "muted" },
  error: { label: "Erro de sync", Icon: CircleAlert, tone: "danger" },
};

function lastSync(value: string | null) {
  if (!value) return "Ainda não sincronizado nesta máquina.";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "" : `Último sync: ${date.toLocaleString()}.`;
}

/** Status discreto do sync do workspace; clicar executa "Sincronizar". */
export function SyncIndicator() {
  const { status, busy, notice } = useSyncView();
  useEffect(() => start(), []);
  if (!desktop) return null;

  const look = LOOK[status?.state ?? "clean"];
  const Icon = busy ? LoaderCircle : look.Icon;
  const label = busy ? "Sincronizando…" : status ? look.label : "Sync";
  const title = [status?.message, status ? lastSync(status.lastSyncedAt) : "", "Clique para sincronizar."].filter(Boolean).join(" ");

  function click() {
    if (busy) return;
    if (status?.state !== "diverged") return void run();
    // Divergência: nunca há vencedor automático. Cada escolha destrutiva é confirmada.
    if (window.confirm("Este computador e o repositório mudaram desde o último sync.\n\nUsar a versão do REPOSITÓRIO aqui? As alterações locais ainda não sincronizadas serão substituídas.")) {
      return void run("remote");
    }
    if (window.confirm("Manter a versão DESTE computador e publicá-la?\n\nA versão do repositório será substituída (o histórico do Git guarda a anterior).")) {
      void run("local");
    }
  }

  return (
    <button
      type="button"
      className={`sync-indicator sync-${busy ? "busy" : look.tone}`}
      onClick={click}
      disabled={busy}
      title={title}
      aria-label={`Sincronização do workspace: ${label}. ${title}`}
    >
      <Icon size={14} className={busy ? "spin" : undefined} />
      <span>{label}</span>
      {notice && !busy && <small role="status">{notice}</small>}
    </button>
  );
}
