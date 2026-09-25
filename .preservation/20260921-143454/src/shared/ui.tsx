import { useEffect, useRef } from "react";
import type { ReactNode } from "react";
import { X, Info, RefreshCw } from "lucide-react";
export function Panel({
  title,
  icon,
  action,
  children,
  className = "",
}: {
  title: string;
  icon?: ReactNode;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={`panel ${className}`}>
      <div className="panel-title">
        <h2>
          {icon}
          {title}
        </h2>
        {action}
      </div>
      {children}
    </section>
  );
}
export function Empty({
  title,
  children,
}: {
  title: string;
  children?: ReactNode;
}) {
  return (
    <div className="empty">
      <Info size={25} />
      <h3>{title}</h3>
      <div>{children}</div>
    </div>
  );
}
export function Badge({
  children,
  tone = "neutral",
}: {
  children: ReactNode;
  tone?: "neutral" | "good" | "warn" | "blue";
}) {
  return <span className={`badge ${tone}`}>{children}</span>;
}
export function Refresh({
  onClick,
  busy = false,
}: {
  onClick: () => void;
  busy?: boolean;
}) {
  return (
    <button className="button subtle" disabled={busy} onClick={onClick}>
      <RefreshCw size={14} className={busy ? "spin" : ""} />
      Atualizar
    </button>
  );
}
export function Modal({
  title,
  close,
  children,
  wide = false,
}: {
  title: string;
  close: () => void;
  children: ReactNode;
  wide?: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const previous = document.activeElement;
    const input = ref.current?.querySelector<HTMLElement>("input,textarea,select");
    (input ?? ref.current)?.focus();
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
      if (e.key === "Tab") {
        const elements = ref.current?.querySelectorAll<HTMLElement>(
          'button:not(:disabled),input,textarea,select,[tabindex="0"]',
        );
        if (!elements?.length) return;
        const first = elements[0],
          last = elements[elements.length - 1];
        if (
          e.shiftKey &&
          (document.activeElement === first ||
            document.activeElement === ref.current)
        ) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    };
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("keydown", key);
      if (previous instanceof HTMLElement) previous.focus();
    };
  }, [close]);
  return (
    <div
      className="overlay"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      <div
        className={`modal ${wide ? "wide" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        ref={ref}
      >
        <div className="modal-header">
          <h2>{title}</h2>
          <button className="icon-button" onClick={close} aria-label="Fechar">
            <X size={20} />
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}
