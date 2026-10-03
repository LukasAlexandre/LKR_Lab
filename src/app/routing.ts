import { Blocks, Bot, Box, Cpu, FileText, FolderOpen, GitBranch, Layers, LayoutDashboard, Network, Settings, Terminal } from "lucide-react";
import type { LucideIcon } from "lucide-react";
export const routes: {
  id: string;
  title: string;
  icon: LucideIcon;
  group?: string;
}[] = [
  { id: "dashboard", title: "Dashboard", icon: LayoutDashboard },
  { id: "projects", title: "Projetos", icon: FolderOpen },
  { id: "repositories", title: "Repositórios", icon: GitBranch },
  { id: "agents", title: "IA / Agents", icon: Bot, group: "INTELIGÊNCIA" },
  { id: "prompts", title: "Prompts", icon: FileText },
  { id: "ports", title: "Portas", icon: Network, group: "AMBIENTE LOCAL" },
  { id: "processes", title: "Processos", icon: Cpu },
  { id: "git", title: "Git / PRs", icon: GitBranch },
  { id: "worktrees", title: "Worktrees", icon: Box },
  { id: "environments", title: "Ambientes", icon: Layers },
  { id: "knowledge", title: "Conhecimento", icon: Blocks, group: "WORKSPACE" },
  { id: "terminal", title: "Terminal", icon: Terminal },
  { id: "settings", title: "Configurações", icon: Settings },
];
export const routeFromHash = () => {
  const value = window.location.hash.slice(1).split("/")[0];
  return routes.some((r) => r.id === value) ? value : "dashboard";
};
export const NEW_PROJECT_HASH = "projects/new";
export const isNewProjectHash = () => window.location.hash.slice(1) === NEW_PROJECT_HASH;
/** "Abrir projeto": visão do projeto selecionado (placeholder até o Project Control Center, Concept 05). */
export const OPEN_PROJECT_HASH = "projects/open";
export const isOpenProjectHash = () => window.location.hash.slice(1) === OPEN_PROJECT_HASH;
