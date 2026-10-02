import { useEffect, useState } from "react";
import { Monitor, RefreshCw } from "lucide-react";
import { Panel } from "../shared/ui";
import {
  detectionAge,
  formatDateTime,
  formatMemory,
  usageLabel,
} from "../shared/machine";
import { useMachine } from "../state/machine";

/** Cadastro desta máquina e "Atualizar agora". O Machine Health completo é o Concept 02. */
export function ThisMachine() {
  const { status, refreshing, error, refresh } = useMachine();
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => setNow(Date.now()), [status]);
  const machine = status?.machine;
  const snapshot = status?.snapshot;
  if (!machine) return null;
  return (
    <Panel
      title="Este computador"
      icon={<Monitor size={18} />}
      className="this-machine"
      action={
        <button className="button subtle" disabled={refreshing} onClick={() => void refresh(true)}>
          <RefreshCw size={14} className={refreshing ? "spin" : ""} />
          {refreshing ? "Detectando…" : "Atualizar agora"}
        </button>
      }
    >
      <dl className="facts">
        <div>
          <dt>Nome</dt>
          <dd>{machine.name}</dd>
        </div>
        <div>
          <dt>Uso / Local</dt>
          <dd>{usageLabel(machine.usage)}</dd>
        </div>
        {machine.description && (
          <div>
            <dt>Descrição</dt>
            <dd>{machine.description}</dd>
          </div>
        )}
        <div>
          <dt>Machine ID</dt>
          <dd className="mono">{machine.machineId}</dd>
        </div>
        <div>
          <dt>Hostname</dt>
          <dd>{snapshot?.hostname ?? "Indisponível"}</dd>
        </div>
        <div>
          <dt>Sistema</dt>
          <dd>{snapshot?.osName ?? "Indisponível"}</dd>
        </div>
        <div>
          <dt>Memória</dt>
          <dd>{formatMemory(snapshot?.memoryTotal) ?? "Indisponível"}</dd>
        </div>
        <div>
          <dt>IP local</dt>
          <dd>
            {snapshot?.localIpv4 ?? "Indisponível"}
            {snapshot?.activeInterface ? ` · ${snapshot.activeInterface}` : ""}
          </dd>
        </div>
        <div>
          <dt>Última detecção</dt>
          <dd>
            {machine.lastDetectedAt
              ? `${detectionAge(machine.lastDetectedAt, now)} · ${formatDateTime(machine.lastDetectedAt)}`
              : "Nunca"}
          </dd>
        </div>
      </dl>
      {error && <p className="warn-text">{error}</p>}
      <p className="footnote">
        Identidade local e estável. IP, hostname e hardware são atualizados ao abrir o
        app e, com ele aberto, quando a detecção passa de 6 horas. Nunca entra no workspace portátil.
      </p>
    </Panel>
  );
}
