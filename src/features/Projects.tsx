import { useState } from "react";
import {
  FolderOpen,
  Plus,
  Code2,
  Terminal,
  GitPullRequest,
  Pencil,
  Trash2,
  FileText,
  GitBranch,
  ArrowUpRight,
} from "lucide-react";
import { api, desktop, errorText } from "../shared/api";
import { parsePorts } from "../shared/logic";
import { Badge, Empty, Modal } from "../shared/ui";
import type {
  Discovery,
  Project,
  ProjectCommand,
  ProjectInput,
} from "../shared/types";
export function ProjectCard({
  project: p,
  open,
  launch,
}: {
  project: Project;
  open: (id: string) => void;
  launch: (id: string, action: string) => void;
}) {
  return (
    <article className="project-card">
      <div className="row">
        <div className="project-icon">
          <FolderOpen size={21} />
        </div>
        <button className="project-name" onClick={() => open(p.id)}>
          {p.name}
        </button>
        <ArrowUpRight size={15} />
      </div>
      <p>{p.description || "Sem descrição"}</p>
      <div className="port-labels">
        {p.ports.length
          ? p.ports.map((p) => `${p.name} :${p.port}`).join(" / ")
          : "Nenhuma porta declarada"}
      </div>
      <div className="tags">
        {p.stack.map((s) => (
          <Badge key={s}>{s}</Badge>
        ))}
      </div>
      <div className="card-footer">
        <button onClick={() => open(p.id)}>
          <GitBranch size={14} />
          Ver estado Git
        </button>
        <div>
          <button
            aria-label={`Abrir pasta ${p.name}`}
            onClick={() => launch(p.id, "folder")}
          >
            <FolderOpen size={15} />
          </button>
          <button
            aria-label={`Abrir terminal ${p.name}`}
            onClick={() => launch(p.id, "terminal")}
          >
            <Terminal size={15} />
          </button>
        </div>
      </div>
    </article>
  );
}
export function ProjectForm({
  project,
  close,
  saved,
}: {
  project?: Project;
  close: () => void;
  saved: () => void;
}) {
  const [form, setForm] = useState<ProjectInput>(
    project ?? {
      name: "",
      description: "",
      localPath: "",
      repository: "",
      stack: [],
      tags: [],
      ports: [],
      commands: [],
    },
  );
  const [ports, setPorts] = useState(
    project?.ports.map((p) => `${p.name}:${p.port}`).join(", ") ?? "",
  );
  const [commands, setCommands] = useState(
    JSON.stringify(project?.commands ?? [], null, 2),
  );
  const [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [detected, setDetected] = useState(false);
  const patch = (p: Partial<ProjectInput>) => setForm((f) => ({ ...f, ...p }));
  async function detect(pick: boolean) {
    setBusy(true);
    setError("");
    try {
      const path = pick
        ? await api<string | null>("choose_folder")
        : form.localPath;
      if (!path) return;
      const d = await api<Discovery>("discover_project", { path });
      patch({
        localPath: d.localPath,
        name: form.name || d.name,
        repository: d.repository,
        stack: d.stack,
      });
      setDetected(true);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }
  async function save() {
    setBusy(true);
    setError("");
    try {
      const parsed: unknown = JSON.parse(commands);
      if (
        !Array.isArray(parsed) ||
        !parsed.every(
          (c: unknown) =>
            typeof c === "object" &&
            c !== null &&
            "name" in c &&
            typeof c.name === "string" &&
            "program" in c &&
            typeof c.program === "string" &&
            "args" in c &&
            Array.isArray(c.args) &&
            c.args.every((a: unknown) => typeof a === "string"),
        )
      )
        throw new Error(
          "Comandos: use uma lista de {name, program, args: []}.",
        );
      await api("save_project", {
        id: project?.id ?? null,
        input: {
          ...form,
          ports: parsePorts(ports),
          commands: parsed as ProjectCommand[],
        },
      });
      saved();
      close();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <Modal
      title={project ? "Editar projeto" : "Adicionar projeto existente"}
      close={close}
      wide
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <div className="form-body">
          <div className="notice">
            Cadastre uma pasta existente. Arquivos e comandos do projeto não
            serão executados durante a descoberta.
          </div>
          <label>
            Pasta local
            <div className="input-action">
              <input
                required
                value={form.localPath}
                placeholder="C:\Projetos\meu-projeto"
                onChange={(e) => patch({ localPath: e.target.value })}
              />
              <button
                type="button"
                className="button"
                disabled={busy || !desktop}
                onClick={() => void detect(true)}
              >
                <FolderOpen size={15} />
                Selecionar
              </button>
              <button
                type="button"
                className="button"
                disabled={busy || !desktop}
                onClick={() => void detect(false)}
              >
                Detectar
              </button>
            </div>
          </label>
          {detected && (
            <div className="notice good-text">
              Detecção concluída. Revise os campos antes de salvar.
            </div>
          )}
          <div className="form-grid">
            <label>
              Nome
              <input
                required
                maxLength={100}
                value={form.name}
                onChange={(e) => patch({ name: e.target.value })}
              />
            </label>
            <label>
              Repositório HTTPS
              <input
                value={form.repository}
                placeholder="https://github.com/usuario/repo"
                onChange={(e) => patch({ repository: e.target.value })}
              />
            </label>
          </div>
          <label>
            Descrição
            <textarea
              rows={2}
              maxLength={4000}
              value={form.description}
              onChange={(e) => patch({ description: e.target.value })}
            />
          </label>
          <div className="form-grid">
            <label>
              Stack · separada por vírgulas
              <input
                value={form.stack.join(", ")}
                onChange={(e) =>
                  patch({
                    stack: e.target.value.split(",").map((s) => s.trim()),
                  })
                }
              />
            </label>
            <label>
              Tags
              <input
                value={form.tags.join(", ")}
                onChange={(e) =>
                  patch({
                    tags: e.target.value.split(",").map((s) => s.trim()),
                  })
                }
              />
            </label>
          </div>
          <label>
            Portas esperadas
            <input
              value={ports}
              onChange={(e) => setPorts(e.target.value)}
              placeholder="frontend:3000, backend:4000"
            />
          </label>
          <details>
            <summary>
              Comandos declarados · execução manual nesta versão
            </summary>
            <p className="muted">
              Não inclua tokens ou senhas. Armazenamento local sem criptografia.
            </p>
            <textarea
              rows={5}
              className="mono"
              value={commands}
              onChange={(e) => setCommands(e.target.value)}
            />
          </details>
          {error && (
            <div className="error" role="alert">
              {error}
            </div>
          )}
        </div>
        <div className="modal-footer">
          <button type="button" className="button" onClick={close}>
            Cancelar
          </button>
          <button className="button primary" disabled={busy || !desktop}>
            {busy ? "Aguarde…" : "Salvar projeto"}
          </button>
        </div>
      </form>
    </Modal>
  );
}
export function Projects({
  projects,
  open,
  launch,
  add,
  edit,
  remove,
}: {
  projects: Project[];
  open: (id: string) => void;
  launch: (id: string, action: string) => void;
  add: () => void;
  edit: (p: Project) => void;
  remove: (p: Project) => void;
}) {
  const [search, setSearch] = useState("");
  const filtered = projects.filter((p) =>
    `${p.name} ${p.stack.join(" ")}`
      .toLowerCase()
      .includes(search.toLowerCase()),
  );
  return (
    <>
      <div className="toolbar">
        <input
          aria-label="Buscar projetos"
          placeholder="Buscar por nome ou stack…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        <button className="button primary" onClick={add}>
          <Plus size={16} />
          Adicionar projeto
        </button>
      </div>
      {!filtered.length ? (
        <Empty title="Seu workspace começa com um projeto">
          <p>
            Conecte uma pasta local para reunir Git, portas e contexto de IA.
          </p>
          <button className="button primary" onClick={add}>
            <FolderOpen size={16} />
            Adicionar projeto existente
          </button>
        </Empty>
      ) : (
        <div className="project-grid">
          {filtered.map((p) => (
            <div key={p.id}>
              <ProjectCard project={p} open={open} launch={launch} />
              <div className="project-actions">
                <button onClick={() => edit(p)}>
                  <Pencil size={14} />
                  Editar
                </button>
                <button onClick={() => remove(p)}>
                  <Trash2 size={14} />
                  Remover cadastro
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </>
  );
}
export function Launchers({
  id,
  launch,
  context,
}: {
  id: string;
  launch: (id: string, action: string) => void;
  context: () => void;
}) {
  return (
    <div className="quick-actions">
      {[
        { action: "folder", text: "Pasta", Icon: FolderOpen },
        { action: "terminal", text: "Terminal", Icon: Terminal },
        { action: "vscode", text: "VS Code", Icon: Code2 },
        { action: "claude", text: "Claude", Icon: Code2 },
        { action: "github", text: "GitHub", Icon: GitPullRequest },
      ].map(({ action, text, Icon }) => (
        <button
          className="button"
          key={action}
          onClick={() => launch(id, action)}
        >
          <Icon size={15} />
          {text}
        </button>
      ))}
      <button className="button primary" onClick={context}>
        <FileText size={15} />
        Gerar contexto
      </button>
    </div>
  );
}
