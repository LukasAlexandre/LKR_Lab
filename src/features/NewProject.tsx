import { useState } from "react";
import { ArrowLeft, CheckCircle2, FolderOpen, GitBranch, Loader2, MapPin, RefreshCw, TriangleAlert } from "lucide-react";
import { api, desktop, errorText } from "../shared/api";
import { Badge } from "../shared/ui";
import { workspace } from "../state/workspace";
import {
  applyInspection,
  canRegister,
  counter,
  DESCRIPTION_MAX,
  editDescription,
  editName,
  initialView,
  locateTarget,
  NAME_MAX,
  phaseOf,
  startAnalysis,
  validateDescription,
  validateName,
} from "../shared/projectInspection";
import type { BindResult, ProjectInspection, RegisterResult } from "../shared/types";

/** Concept 04 · Cadastro de Projeto. A inspeção é passiva (arquivos + Git somente leitura) e feita pelo backend. */
export function NewProject({ close, done }: { close: () => void; done: (projectId: string) => void }) {
  const [view, setView] = useState(initialView);
  const phase = phaseOf(view);
  const inspection = view.inspection;
  const target = locateTarget(view);

  async function analyze(folder: string) {
    setView((v) => startAnalysis(v, folder));
    try {
      const result = await api<ProjectInspection>("inspect_project_folder", { path: folder });
      setView((v) => applyInspection(v, result));
    } catch (error) {
      setView((v) => ({ ...v, analyzing: false, error: errorText(error) }));
    }
  }
  async function pick() {
    try {
      const folder = await api<string | null>("choose_folder");
      if (folder) await analyze(folder);
    } catch (error) {
      setView((v) => ({ ...v, error: errorText(error) }));
    }
  }
  async function register() {
    if (!canRegister(view)) return;
    setView((v) => ({ ...v, registering: true, error: "" }));
    try {
      const result = await api<RegisterResult>("register_project", {
        input: { path: view.folder, name: view.name.trim(), description: view.description },
      });
      if (result.registered && result.project) {
        await workspace.loadRegistry();
        done(result.project.id);
        return;
      }
      // O backend decidiu que não é novo (outra janela cadastrou antes): mostra o caminho certo.
      setView((v) => ({
        ...v,
        registering: false,
        inspection: v.inspection ? { ...v.inspection, registration: result.registration } : v.inspection,
      }));
    } catch (error) {
      setView((v) => ({ ...v, registering: false, error: errorText(error) }));
    }
  }
  async function locate() {
    if (!target) return;
    setView((v) => ({ ...v, registering: true, error: "" }));
    try {
      let result = await api<BindResult>("bind_project", { id: target.id, path: view.folder, confirmed: false });
      if (result.needsConfirmation) {
        if (!window.confirm(`${result.message}\n\nAssociar mesmo assim a:\n${view.folder}`)) {
          setView((v) => ({ ...v, registering: false }));
          return;
        }
        result = await api<BindResult>("bind_project", { id: target.id, path: view.folder, confirmed: true });
      }
      await workspace.loadRegistry();
      done(target.id);
    } catch (error) {
      setView((v) => ({ ...v, registering: false, error: errorText(error) }));
    }
  }

  const nameError = view.nameTouched ? validateName(view.name) : null;
  const descriptionError = validateDescription(view.description);
  const busy = phase === "analyzing" || phase === "registering";

  return (
    <section className="new-project" aria-label="Novo projeto">
      <div className="breadcrumb">
        <button type="button" className="link-button" onClick={close}>
          <ArrowLeft size={14} /> Workspace
        </button>
        <span>›</span>
        <button type="button" className="link-button" onClick={close}>
          Projetos
        </button>
        <span>›</span>
        <strong>Novo projeto</strong>
      </div>
      <div className="page-heading">
        <div>
          <div className="eyebrow">CADASTRO DE PROJETO</div>
          <h1>Novo projeto</h1>
          <p>Escolha a pasta. O LKR LAB lê arquivos e o Git em modo somente leitura; nada do projeto é executado.</p>
        </div>
      </div>
      {!desktop && (
        <div className="notice" role="status">
          A seleção de pasta e a inspeção são recursos do aplicativo desktop. A prévia web não simula inspeção.
        </div>
      )}
      <div className="new-project-grid">
        <form
          className="panel new-project-form"
          onSubmit={(event) => {
            event.preventDefault();
            void register();
          }}
        >
          <label>
            Pasta
            <div className="input-action">
              <input readOnly value={view.folder} placeholder="Nenhuma pasta selecionada" aria-label="Pasta do projeto" />
              <button type="button" className="button" disabled={!desktop || busy} onClick={() => void pick()}>
                <FolderOpen size={15} /> Selecionar
              </button>
            </div>
          </label>
          <label>
            Nome
            <input
              maxLength={NAME_MAX}
              value={view.name}
              aria-invalid={nameError ? true : undefined}
              onChange={(e) => setView((v) => editName(v, e.target.value))}
            />
            <small className="counter">{counter(view.name, NAME_MAX)}</small>
            {nameError && (
              <small className="field-error" role="alert">
                {nameError}
              </small>
            )}
          </label>
          <label>
            Descrição <span className="muted">(opcional)</span>
            <textarea
              rows={3}
              maxLength={DESCRIPTION_MAX}
              value={view.description}
              onChange={(e) => setView((v) => editDescription(v, e.target.value))}
            />
            <small className="counter">{counter(view.description, DESCRIPTION_MAX)}</small>
            {descriptionError && (
              <small className="field-error" role="alert">
                {descriptionError}
              </small>
            )}
          </label>
        </form>

        <aside className="panel inspection" aria-live="polite">
          <header className="inspection-head">
            <h2>Inspeção automática</h2>
            {phase === "analyzing" && (
              <Badge tone="warn">
                <Loader2 size={12} className="spin" /> Analisando…
              </Badge>
            )}
            {inspection?.valid && !busy && (
              <Badge tone="good">
                <CheckCircle2 size={12} /> Inspeção concluída
              </Badge>
            )}
            <button
              type="button"
              className="button subtle"
              disabled={!view.folder || busy || !desktop}
              onClick={() => void analyze(view.folder)}
            >
              <RefreshCw size={14} /> Reanalisar
            </button>
          </header>

          {phase === "empty" && <p className="muted">Selecione uma pasta para ver o que foi detectado.</p>}
          {phase === "error" && (
            <div className="notice danger-text" role="alert">
              {view.error}
            </div>
          )}
          {phase === "invalid" && (
            <div className="notice danger-text" role="alert">
              <TriangleAlert size={14} /> {inspection?.error ?? inspection?.registration.message ?? "Pasta inválida."}
            </div>
          )}

          {inspection?.valid && (
            <dl className="inspection-list">
              <div>
                <dt>
                  <GitBranch size={13} /> Git
                </dt>
                <dd>
                  {inspection.git ? (
                    <>
                      {inspection.git.detached ? "HEAD destacado" : inspection.git.branch}
                      {" · "}
                      {inspection.git.clean ? "limpo" : `${inspection.git.changes} alterações`}
                      <div className="mono muted">
                        {inspection.locator
                          ? `${inspection.locator.remote}${inspection.locator.path ? ` / ${inspection.locator.path}` : ""}`
                          : (inspection.locatorNote ?? "Sem remote: identidade do repositório indisponível.")}
                      </div>
                    </>
                  ) : (
                    "Não é um repositório Git"
                  )}
                </dd>
              </div>
              <div>
                <dt>Stack</dt>
                <dd>
                  {inspection.stack.length
                    ? inspection.stack.map((s) => <Badge key={s.id}>{s.label}</Badge>)
                    : "Stack não detectada"}
                </dd>
              </div>
              <div>
                <dt>Package manager</dt>
                <dd>{inspection.packageManager?.name ?? inspection.packageManagerNote ?? "—"}</dd>
              </div>
              <div>
                <dt>
                  Scripts <span className="muted">(não executados)</span>
                </dt>
                <dd>{inspection.scripts.length ? inspection.scripts.map((s) => s.name).join(", ") : "—"}</dd>
              </div>
              <div>
                <dt>Arquivos principais</dt>
                <dd className="mono">{inspection.importantFiles.length ? inspection.importantFiles.join(", ") : "—"}</dd>
              </div>
              <div>
                <dt>Estrutura</dt>
                <dd className="mono">
                  {inspection.structure.length
                    ? inspection.structure.map((e) => (e.kind === "dir" ? `${e.name}/` : e.name)).join("  ")
                    : "Pasta vazia"}
                </dd>
              </div>
              {inspection.warnings.map((w) => (
                <div key={w}>
                  <dt>
                    <TriangleAlert size={13} /> Aviso
                  </dt>
                  <dd>{w}</dd>
                </div>
              ))}
            </dl>
          )}

          {phase === "known" && inspection && (
            <div className="notice" role="status">
              <strong>Projeto já conhecido</strong>
              <p>{inspection.registration.message}</p>
              {target ? (
                <button type="button" className="button primary" disabled={busy} onClick={() => void locate()}>
                  <MapPin size={14} /> Localizar / Associar a {target.name}
                </button>
              ) : (
                <p className="muted">
                  {inspection.registration.matches[0]?.reason ?? "Nenhuma associação automática é segura."}
                </p>
              )}
            </div>
          )}
          {phase === "already_here" && inspection && (
            <div className="notice" role="status">
              <strong>Projeto já cadastrado nesta máquina</strong>
              <p>{inspection.registration.message}</p>
            </div>
          )}
          {phase === "ambiguous" && inspection && (
            <div className="notice danger-text" role="alert">
              <strong>Mais de um projeto com a mesma identidade</strong>
              <p>{inspection.registration.message}</p>
            </div>
          )}

          <footer className="inspection-foot">
            {phase === "ready" || phase === "registering" ? (
              <>
                {phase === "ready" && <Badge tone="good">Pronto para cadastrar</Badge>}
                <button
                  type="button"
                  className="button primary"
                  disabled={!canRegister(view) || busy}
                  onClick={() => void register()}
                >
                  {phase === "registering" ? "Cadastrando…" : "Cadastrar projeto"}
                </button>
              </>
            ) : null}
            <button type="button" className="button" onClick={close} disabled={phase === "registering"}>
              Cancelar
            </button>
          </footer>
          {phase !== "error" && view.error && (
            <div className="notice danger-text" role="alert">
              {view.error}
            </div>
          )}
        </aside>
      </div>
    </section>
  );
}
