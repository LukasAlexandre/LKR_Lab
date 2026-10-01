import { memo, useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";

const report = (error: unknown) => console.error("[janela]", error);

/*
 * Titlebar da janela desktop (a decoração nativa está desligada em tauri.conf.json).
 * Arrastar e duplo clique (maximizar/restaurar) são do próprio Tauri via
 * data-tauri-drag-region="deep": qualquer área vazia arrasta; botões e outros
 * elementos interativos ficam fora da região automaticamente.
 */
export const WindowTitleBar = memo(function WindowTitleBar() {
  const [maximized, setMaximized] = useState(false);

  // Estado real da janela: consulta no início e a cada evento de resize do Tauri
  // (maximizar, restaurar, Win+setas, snap). Sem polling.
  useEffect(() => {
    const appWindow = getCurrentWindow();
    let alive = true;
    let frame = 0;
    let unlisten: (() => void) | undefined;
    const sync = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        appWindow.isMaximized().then((value) => { if (alive) setMaximized(value); }).catch(report);
      });
    };
    sync();
    appWindow.onResized(sync).then((stop) => { if (alive) unlisten = stop; else stop(); }).catch(report);
    return () => {
      alive = false;
      cancelAnimationFrame(frame);
      unlisten?.();
    };
  }, []);

  const appWindow = getCurrentWindow();
  const maximizeLabel = maximized ? "Restaurar janela" : "Maximizar janela";
  return (
    <div className="titlebar" data-tauri-drag-region="deep">
      <div className="titlebar-identity">
        <span className="brand-mark titlebar-mark" aria-hidden="true"><i /><i /><i /><i /></span>
        <span className="titlebar-title">LKR LAB</span>
      </div>
      <div className="window-controls">
        <button type="button" className="window-control" aria-label="Minimizar janela" title="Minimizar" onClick={() => void appWindow.minimize().catch(report)}>
          <Minus size={14} strokeWidth={1.4} />
        </button>
        <button type="button" className="window-control" aria-label={maximizeLabel} title={maximized ? "Restaurar" : "Maximizar"} onClick={() => void appWindow.toggleMaximize().catch(report)}>
          {maximized ? <Copy size={12} strokeWidth={1.4} className="window-restore-icon" /> : <Square size={12} strokeWidth={1.4} />}
        </button>
        <button type="button" className="window-control window-control-close" aria-label="Fechar janela" title="Fechar" onClick={() => void appWindow.close().catch(report)}>
          <X size={15} strokeWidth={1.4} />
        </button>
      </div>
    </div>
  );
});
