import { useEffect, useReducer, useState } from "react";
import { CheckCircle2, Monitor, Pencil, RefreshCw } from "lucide-react";
import { Panel } from "../shared/ui";
import { errorText } from "../shared/api";
import {
  EDIT_IDLE,
  MACHINE_DESCRIPTION_MAX,
  MACHINE_NAME_MAX,
  USAGE_OPTIONS,
  detectionAge,
  draftChanged,
  draftError,
  draftInput,
  formatDateTime,
  formatMemory,
  machineEdit,
  usageLabel,
  validateMachineName,
} from "../shared/machine";
import type { MachineUsage } from "../shared/types";
import { useMachine } from "../state/machine";

/**
 * Cadastro desta máquina. Nome, uso e descrição são editáveis aqui (metadata do LKR LAB);
 * Machine ID e tudo que é detectado ficam só leitura. O Machine Health completo é o Concept 02.
 */
export function ThisMachine() {
  const { status, refreshing, error, refresh, update } = useMachine();
  const [edit, dispatch] = useReducer(machineEdit, EDIT_IDLE);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => setNow(Date.now()), [status]);
  useEffect(() => {
    if (edit.mode !== "success") return;
    const timer = window.setTimeout(() => dispatch({ type: "settle" }), 3000);
    return () => window.clearTimeout(timer);
  }, [edit.mode]);
  const machine = status?.machine;
  const snapshot = status?.snapshot;
  if (!machine) return null;

  const editing = edit.draft !== null;
  const saving = edit.mode === "saving";
  const validation = edit.draft ? draftError(edit.draft) : null;
  const changed = edit.draft ? draftChanged(machine, edit.draft) : false;
  const save = () => {
    if (!edit.draft || validation || saving) return;
    if (!changed) return dispatch({ type: "cancel" });
    dispatch({ type: "save" });
    update(draftInput(edit.draft)).then(
      () => dispatch({ type: "saved" }),
      (e: unknown) => dispatch({ type: "failed", error: errorText(e) }),
    );
  };
  const detectionStatus = refreshing ? "Detectando…" : status.stale ? "Detecção expirada" : "Atualizado";

  return (
    <Panel
      title="Este computador"
      icon={<Monitor size={18} />}
      className="this-machine"
      action={
        <div className="this-machine-actions">
          {!editing && (
            <button className="button subtle" onClick={() => dispatch({ type: "edit", machine })}>
              <Pencil size={14} />
              Editar
            </button>
          )}
          <button className="button subtle" disabled={refreshing || saving} onClick={() => void refresh(true)}>
            <RefreshCw size={14} className={refreshing ? "spin" : ""} />
            {refreshing ? "Detectando…" : "Atualizar agora"}
          </button>
        </div>
      }
    >
      {edit.draft ? (
        <form
          className="this-machine-form"
          aria-label="Editar este computador"
          onSubmit={(e) => {
            e.preventDefault();
            save();
          }}
        >
          <label>
            Nome deste computador
            <input
              value={edit.draft.name}
              maxLength={MACHINE_NAME_MAX}
              disabled={saving}
              aria-invalid={!!validateMachineName(edit.draft.name)}
              autoFocus
              onChange={(e) => dispatch({ type: "change", patch: { name: e.target.value } })}
            />
          </label>
          <label>
            Uso / Local
            <select
              value={edit.draft.usage}
              disabled={saving}
              onChange={(e) => dispatch({ type: "change", patch: { usage: e.target.value as MachineUsage } })}
            >
              {USAGE_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>{option.label}</option>
              ))}
            </select>
          </label>
          <label className="span-2">
            Descrição <span className="optional">(opcional)</span>
            <textarea
              rows={2}
              value={edit.draft.description}
              maxLength={MACHINE_DESCRIPTION_MAX}
              disabled={saving}
              onChange={(e) => dispatch({ type: "change", patch: { description: e.target.value } })}
            />
            <small className="field-counter">{edit.draft.description.length}/{MACHINE_DESCRIPTION_MAX}</small>
          </label>
          {(validation || edit.error) && (
            <div className="error span-2" role="alert">{edit.error ?? validation}</div>
          )}
          <div className="this-machine-form-actions span-2">
            <span className="muted">Machine ID, hostname e dados detectados não são editáveis.</span>
            <button type="button" className="button" disabled={saving} onClick={() => dispatch({ type: "cancel" })}>
              Cancelar
            </button>
            <button type="submit" className="button primary" disabled={saving || !!validation}>
              {saving ? "Salvando…" : "Salvar alterações"}
            </button>
          </div>
        </form>
      ) : (
        <dl className="facts">
          <div>
            <dt>Nome</dt>
            <dd>{machine.name}</dd>
          </div>
          <div>
            <dt>Uso / Local</dt>
            <dd>{usageLabel(machine.usage)}</dd>
          </div>
          <div>
            <dt>Descrição</dt>
            <dd>{machine.description || "—"}</dd>
          </div>
        </dl>
      )}
      {edit.mode === "success" && (
        <p className="this-machine-saved" role="status">
          <CheckCircle2 size={14} /> Alterações salvas.
        </p>
      )}
      <dl className="facts this-machine-detected">
        <div>
          <dt>Machine ID</dt>
          <dd className="mono" title={machine.machineId}>{machine.machineId.slice(0, 8)}…</dd>
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
        <div>
          <dt>Status</dt>
          <dd>{detectionStatus}</dd>
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
