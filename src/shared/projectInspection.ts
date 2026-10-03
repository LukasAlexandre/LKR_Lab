import type { MatchedProject, ProjectInspection } from "./types";

export const NAME_MAX = 50;
export const DESCRIPTION_MAX = 200;

/** Estados da tela "Novo projeto": a interface só reflete o que o backend decidiu. */
export type NewProjectPhase =
  | "empty" // nenhuma pasta escolhida
  | "analyzing" // inspeção em andamento
  | "invalid" // pasta inválida
  | "ready" // inspeção ok, pode cadastrar
  | "known" // projeto já conhecido: Localizar/Associar
  | "already_here" // esta pasta já está cadastrada
  | "ambiguous" // mais de um projeto com a mesma identidade
  | "registering"
  | "error"; // falha inesperada (inspeção ou cadastro)

export interface NewProjectView {
  folder: string;
  name: string;
  description: string;
  /** O usuário mexeu no nome: Reanalisar não pode sobrescrevê-lo. */
  nameTouched: boolean;
  inspection: ProjectInspection | null;
  analyzing: boolean;
  registering: boolean;
  error: string;
}

export const initialView: NewProjectView = {
  folder: "",
  name: "",
  description: "",
  nameTouched: false,
  inspection: null,
  analyzing: false,
  registering: false,
  error: "",
};

export function validateName(name: string): string | null {
  const trimmed = name.trim();
  if (!trimmed) return "Informe o nome do projeto.";
  if (trimmed.length > NAME_MAX) return `Nome com até ${NAME_MAX} caracteres.`;
  return null;
}
export function validateDescription(description: string): string | null {
  return description.length > DESCRIPTION_MAX ? `Descrição com até ${DESCRIPTION_MAX} caracteres.` : null;
}
export function counter(value: string, max: number): string {
  return `${value.length}/${max}`;
}

export function phaseOf(view: NewProjectView): NewProjectPhase {
  if (view.registering) return "registering";
  if (view.analyzing) return "analyzing";
  if (view.error) return "error";
  const inspection = view.inspection;
  if (!inspection) return "empty";
  if (!inspection.valid) return "invalid";
  switch (inspection.registration.status) {
    case "known":
      return "known";
    case "already_here":
      return "already_here";
    case "ambiguous":
      return "ambiguous";
    case "invalid":
      return "invalid";
    default:
      return "ready";
  }
}

/** Só `ready` cadastra, e só com nome e descrição válidos. Qualquer outro estado nunca oferece "Cadastrar". */
export function canRegister(view: NewProjectView): boolean {
  return phaseOf(view) === "ready" && !validateName(view.name) && !validateDescription(view.description);
}

/** Projeto conhecido que pode ser associado a esta pasta (vínculo ausente ou pasta antiga sumida). */
export function locateTarget(view: NewProjectView): MatchedProject | null {
  if (phaseOf(view) !== "known") return null;
  const matches = view.inspection?.registration.matches ?? [];
  return matches.length === 1 && matches[0].canLocate ? matches[0] : null;
}

export type PrimaryAction = "register" | "locate" | "none";
export function primaryAction(view: NewProjectView): PrimaryAction {
  const phase = phaseOf(view);
  if (phase === "ready") return "register";
  if (phase === "known" && locateTarget(view)) return "locate";
  return "none";
}

/** Resultado de uma (re)análise: preserva o que o usuário digitou; só sugere o nome se ele não mexeu. */
export function applyInspection(view: NewProjectView, inspection: ProjectInspection): NewProjectView {
  return {
    ...view,
    inspection,
    analyzing: false,
    error: "",
    folder: inspection.valid ? inspection.folder : view.folder,
    name: view.nameTouched || view.name.trim() ? view.name : inspection.suggestedName.slice(0, NAME_MAX),
  };
}
export function startAnalysis(view: NewProjectView, folder: string): NewProjectView {
  return { ...view, folder, analyzing: true, error: "" };
}
export function editName(view: NewProjectView, name: string): NewProjectView {
  return { ...view, name, nameTouched: true };
}
export function editDescription(view: NewProjectView, description: string): NewProjectView {
  return { ...view, description };
}
