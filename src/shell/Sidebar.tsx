import { memo } from "react";
import type { ReactNode } from "react";
import { Lock, Monitor, PanelLeftClose, PanelLeftOpen, ShieldCheck } from "lucide-react";
import { routes } from "../app/routing";
/** `locked`: computador não cadastrado. Os módulos aparecem, mas não são links. */
export const Sidebar = memo(function Sidebar({ route, sidebarCompact, toggle, projectCount, locked = false, projectNav, parentRoute }: {
 route: string; sidebarCompact: boolean; toggle: () => void; projectCount: number; locked?: boolean;
 /** Seção "PROJETO ATUAL" (só dentro de #project/<id>/…). */
 projectNav?: ReactNode;
 /** Item global que contém o contexto aberto ("Projetos"): fica sutil, não competindo com a área atual. */
 parentRoute?: string;
}) {
 return (<aside className="sidebar" aria-label="Navegação do workspace">
        <div className="brand">
          <div className="brand-mark">
            <i />
            <i />
            <i />
            <i />
          </div>
          <div>
            <strong>LKR LAB</strong>
            <small>DEVELOPER WORKSPACE</small>
          </div>
          <button
            className="sidebar-toggle icon-button"
            onClick={toggle}
            aria-label={sidebarCompact ? "Expandir sidebar" : "Recolher sidebar"}
            title={sidebarCompact ? "Expandir sidebar" : "Recolher sidebar"}
          >
            {sidebarCompact ? (
              <PanelLeftOpen size={16} />
            ) : (
              <PanelLeftClose size={16} />
            )}
          </button>
        </div>
        <nav aria-label="Navegação principal">
          {routes.map(({ id, title, icon: Icon, group }) => (
            <div key={id}>
              {group && <div className="nav-group">{group}</div>}
              {locked && id === "ports" && (
                <span className="nav-item active" aria-current="page" title={sidebarCompact ? "Configuração inicial" : undefined}>
                  <Monitor size={17} />
                  <span className="nav-label">Configuração inicial</span>
                </span>
              )}
              {locked ? (
                <span
                  className="nav-item locked"
                  aria-disabled="true"
                  title={`${title} · disponível após cadastrar este computador`}
                >
                  <Icon size={17} />
                  <span className="nav-label">{title}</span>
                  <Lock size={13} className="nav-lock" aria-label="Bloqueado" />
                </span>
              ) : (
                <a
                  href={`#${id}`}
                  className={`nav-item ${route === id ? "active" : ""} ${parentRoute === id ? "parent" : ""}`}
                  aria-current={route === id ? "page" : undefined}
                  title={sidebarCompact ? title : undefined}
                >
                  <Icon size={17} />
                  <span className="nav-label">{title}</span>
                  {id === "projects" && projectCount > 0 && (
                    <span className="nav-count">{projectCount}</span>
                  )}
                </a>
              )}
            </div>
          ))}
          {projectNav}
        </nav>
        <div className="sidebar-footer">
          <div className="local-mark">
            <ShieldCheck size={15} />
            Local-first
          </div>
          <p>
            Seu ambiente.
            <br />
            Seu contexto. Seu controle.
          </p>
          <small>LK TECHNOLOGIES BRASIL · 0.1</small>
        </div>
      </aside>);
});
