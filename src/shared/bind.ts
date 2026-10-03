import { api } from "./api";
import type { BindResult } from "./types";

/**
 * "Localizar / Associar": ÚNICO caminho de vínculo da interface (Novo projeto e card de Projetos).
 * Quem decide é o backend (`bind_project`): confere repositório/subprojeto pelo locator, recusa
 * pasta de outro projeto e nunca troca um vínculo válido. Devolve `null` se o usuário desistiu
 * da confirmação; erros do backend sobem como exceção com a explicação.
 */
export async function bindWithConfirmation(id: string, folder: string): Promise<BindResult | null> {
  let result = await api<BindResult>("bind_project", { id, path: folder, confirmed: false });
  if (result.needsConfirmation) {
    if (!window.confirm(`${result.message}\n\nAssociar mesmo assim a:\n${folder}`)) return null;
    result = await api<BindResult>("bind_project", { id, path: folder, confirmed: true });
  }
  return result;
}

/** Seleciona a pasta no seletor nativo e associa ao projeto. `null` = cancelado em qualquer etapa. */
export async function locateProject(id: string): Promise<BindResult | null> {
  const folder = await api<string | null>("choose_folder");
  return folder ? bindWithConfirmation(id, folder) : null;
}
