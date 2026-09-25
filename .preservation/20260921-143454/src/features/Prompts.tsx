import { useState } from "react";
import { Copy, Plus, Save, Star } from "lucide-react";
import { api, desktop } from "../shared/api";
import { renderPrompt } from "../shared/logic";
import { Badge, Empty } from "../shared/ui";
import type { GitState, Project, Prompt } from "../shared/types";
import { usePersistentState } from "../shared/preferences";
import { normalizeSearch } from "../shared/search";
export function Prompts({
  prompts,
  projects,
  selected,
  git,
  refresh,
  report,
  notify,
}: {
  prompts: Prompt[];
  projects: Project[];
  selected?: Project;
  git: GitState | null;
  refresh: () => void;
  report: (e: unknown) => void;
  notify: (s: string) => void;
}) {
  const [editing, setEditing] = useState<Prompt | null>(null),
    [projectId, setProjectId] = useState(selected?.id ?? ""),
    [busy, setBusy] = useState(false);
  const project = projects.find((p) => p.id === projectId);
  const [query, setQuery] = useState("");
  const [goal, setGoal] = useState("");
  const [favorites, setFavorites] = usePersistentState<string[]>("lk.prompt.favorites", []);
  const generated =
    editing && project
      ? renderPrompt(
          editing.body,
          project,
          git && selected?.id === project.id ? git : null,
          undefined,
          goal,
        )
      : null;
  async function save() {
    if (!editing) return;
    setBusy(true);
    try {
      const id = await api<string>("save_prompt", { prompt: editing });
      setEditing(current => current ? { ...current, id } : current);
      refresh();
      notify("Template salvo.");
    } catch (e) {
      report(e);
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="prompt-layout">
      <aside className="prompt-list">
        <button
          className="button primary"
          disabled={busy}
          onClick={() =>
            setEditing({
              id: "",
              title: "",
              category: "Development",
              projectId: null,
              body: "",
            })
          }
        >
          <Plus size={15} />
          Novo template
        </button>
        <input aria-label="Buscar prompts" placeholder="Buscar templates…" value={query} onChange={event => setQuery(event.target.value)} />
        {prompts
          .filter((p) => (!p.projectId || p.projectId === projectId) && normalizeSearch(`${p.title} ${p.category} ${p.body}`).includes(normalizeSearch(query)))
          .sort((a, b) => Number(favorites.includes(b.id)) - Number(favorites.includes(a.id)))
          .map((p) => (
            <button
              key={p.id}
              disabled={busy}
              className={`prompt-item ${editing?.id === p.id ? "selected" : ""}`}
              onClick={() => setEditing(p)}
            >
              <strong>{favorites.includes(p.id) && <Star size={12} />} {p.title}</strong>
              <small>
                {p.category} · {p.projectId ? "Projeto" : "Global"}
              </small>
            </button>
          ))}
      </aside>
      <section className="panel prompt-editor">
        {!editing ? (
          <Empty title="Prompts com contexto, prontos para usar">
            <p>
              Escolha um template ou crie o seu. Nenhum agente é executado
              automaticamente.
            </p>
          </Empty>
        ) : (
          <>
            <div className="form-grid">
              <label>
                Título
                <input
                  value={editing.title}
                  onChange={(e) =>
                    setEditing({ ...editing, title: e.target.value })
                  }
                />
              </label>
              <label>
                Categoria
                <select
                  value={editing.category}
                  onChange={(e) =>
                    setEditing({ ...editing, category: e.target.value })
                  }
                >
                  {["Development", "GitHub", "Security", "Deployment"].map(
                    (c) => (
                      <option key={c}>{c}</option>
                    ),
                  )}
                </select>
              </label>
            </div>
            <label>
              Escopo
              <select
                value={editing.projectId ?? ""}
                onChange={(e) =>
                  setEditing({ ...editing, projectId: e.target.value || null })
                }
              >
                <option value="">Global</option>
                {projects.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Template
              <textarea
                className="mono"
                rows={8}
                value={editing.body}
                onChange={(e) =>
                  setEditing({ ...editing, body: e.target.value })
                }
              />
            </label>
            <div className="row">
              <Badge>{"{{project.name}}"}</Badge>
              <Badge>{"{{git.branch}}"}</Badge>
              {!!editing.id && <button type="button" className="button subtle" aria-pressed={favorites.includes(editing.id)} onClick={() => setFavorites(values => values.includes(editing.id) ? values.filter(id => id !== editing.id) : [...values, editing.id])}><Star size={14} /> Favorito</button>}
              <button
                className="button"
                disabled={!desktop || busy}
                onClick={() => void save()}
              >
                <Save size={14} />
                Salvar
              </button>
            </div>
            <label>
              Gerar para o projeto
              <select
                value={projectId}
                onChange={(e) => setProjectId(e.target.value)}
              >
                <option value="">Selecione…</option>
                {projects.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
            </label>
            <label>Objetivo para {"{{goal}}"}<input value={goal} onChange={event => setGoal(event.target.value)} placeholder="Descreva a tarefa deste prompt" /></label>
            {generated && (
              <>
                <pre className="context-preview">{generated.text}</pre>
                {generated.unresolved.length > 0 && (
                  <p className="warn-text">
                    Variáveis desconhecidas: {generated.unresolved.join(", ")}
                  </p>
                )}
                <button
                  className="button primary"
                  onClick={() =>
                    void navigator.clipboard
                      .writeText(generated.text)
                      .then(() => notify("Prompt copiado."))
                      .catch(report)
                  }
                >
                  <Copy size={14} />
                  Copiar prompt
                </button>
              </>
            )}
          </>
        )}
      </section>
    </div>
  );
}
