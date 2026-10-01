import { useState } from "react";
import { MapPin } from "lucide-react";
import { api, errorText } from "../shared/api";
import { Badge } from "../shared/ui";
import { workspace } from "../state/workspace";
import type { BindResult, Project } from "../shared/types";

/**
 * Situação do projeto nesta máquina + "Localizar".
 * O caminho é um vínculo local: um projeto do workspace pode existir aqui sem pasta.
 */
export function LocationNotice({ project, report }: { project: Project; report: (error: unknown) => void }) {
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  if (project.location === "available") return null;

  async function locate() {
    setBusy(true);
    setMessage("");
    try {
      const path = await api<string | null>("choose_folder");
      if (!path) return;
      let result = await api<BindResult>("bind_project", { id: project.id, path, confirmed: false });
      if (result.needsConfirmation) {
        if (!window.confirm(`${result.message}\n\nVincular mesmo assim a:\n${path}`)) return;
        result = await api<BindResult>("bind_project", { id: project.id, path, confirmed: true });
      }
      setMessage(result.message);
      await workspace.loadRegistry();
      await workspace.refreshRepositories(0);
    } catch (error) {
      setMessage(errorText(error));
      report(error);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="location-notice">
      <Badge tone="warn">
        {project.location === "unbound" ? "Ainda não localizado nesta máquina" : "Pasta não encontrada nesta máquina"}
      </Badge>
      <button type="button" className="button subtle" disabled={busy} onClick={() => void locate()}>
        <MapPin size={14} /> Localizar
      </button>
      {message && <small role="status">{message}</small>}
    </div>
  );
}
