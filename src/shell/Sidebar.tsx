import { memo } from "react";
import { PanelLeftClose, PanelLeftOpen, ShieldCheck } from "lucide-react";
import { routes } from "../app/routing";
export const Sidebar = memo(function Sidebar({ route, sidebarCompact, toggle, projectCount }: {
 route: string; sidebarCompact: boolean; toggle: () => void; projectCount: number;
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
              <a
                href={`#${id}`}
                className={`nav-item ${route === id ? "active" : ""}`}
                aria-current={route === id ? "page" : undefined}
                title={sidebarCompact ? title : undefined}
              >
                <Icon size={17} />
                <span className="nav-label">{title}</span>
                {id === "projects" && projectCount > 0 && (
                  <span className="nav-count">{projectCount}</span>
                )}
              </a>
            </div>
          ))}
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
