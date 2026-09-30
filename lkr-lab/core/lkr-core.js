/*
 * LKR LAB — core
 * Utilitários compartilhados entre módulos: DOM, ícones, formatação,
 * armazenamento local, toasts e diálogo de confirmação.
 * Script clássico (sem ES modules) para funcionar abrindo o arquivo direto via file://.
 */
(function (root) {
  "use strict";

  const LKR = (root.LKR = root.LKR || {});

  // ---------------------------------------------------------------- DOM

  /**
   * Cria elementos sem innerHTML, para que nenhum texto do usuário ou de um
   * backup importado seja interpretado como HTML.
   */
  function h(tag, props, ...children) {
    const el = document.createElement(tag);
    if (props) {
      for (const [key, value] of Object.entries(props)) {
        if (value === null || value === undefined || value === false) continue;
        if (key === "class") el.className = value;
        else if (key === "text") el.textContent = value;
        else if (key === "dataset") Object.assign(el.dataset, value);
        else if (key === "value" || key === "checked" || key === "htmlFor" || key === "disabled") el[key] = value;
        else if (key.startsWith("on") && typeof value === "function") el.addEventListener(key.slice(2), value);
        else el.setAttribute(key, value === true ? "" : String(value));
      }
    }
    appendChildren(el, children);
    return el;
  }

  function appendChildren(el, children) {
    for (const child of children) {
      if (child === null || child === undefined || child === false) continue;
      if (Array.isArray(child)) appendChildren(el, child);
      else el.append(child instanceof Node ? child : document.createTextNode(String(child)));
    }
  }

  // Ícones em traço (24×24). Strings estáticas e confiáveis.
  const ICONS = {
    plus: '<path d="M12 5v14M5 12h14"/>',
    search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>',
    check: '<path d="m5 12.5 4.5 4.5L19 7.5"/>',
    download: '<path d="M12 4v11m0 0-4-4m4 4 4-4M5 19h14"/>',
    upload: '<path d="M12 16V5m0 0-4 4m4-4 4 4M5 19h14"/>',
    reset: '<path d="M4 12a8 8 0 1 0 2.4-5.7M4 4v4.5h4.5"/>',
    trash: '<path d="M5 7h14M10 7V5h4v2m-7 0 .8 12h8.4L17 7"/>',
    close: '<path d="M6 6l12 12M18 6 6 18"/>',
    shield: '<path d="M12 3.5 5 6v5.5c0 4.3 3 7.6 7 9 4-1.4 7-4.7 7-9V6z"/>',
    alert: '<path d="M12 4 3 19.5h18zM12 10v4.5m0 2.5v.01"/>',
    info: '<circle cx="12" cy="12" r="8.5"/><path d="M12 11v5m0-8v.01"/>',
    github: '<path d="M12 19V7m0 0-4.5 4.5M12 7l4.5 4.5"/><path d="M5 4h14"/>',
    refresh: '<path d="M19.5 12a7.5 7.5 0 0 1-13 5.1M4.5 12a7.5 7.5 0 0 1 13-5.1M17.5 3.5v3.6h-3.6M6.5 20.5v-3.6h3.6"/>',
    undo: '<path d="M9 14 4.5 9.5 9 5"/><path d="M4.5 9.5H14a5.5 5.5 0 0 1 0 11h-3"/>',
  };

  function icon(name, extraClass) {
    const span = document.createElement("span");
    span.className = "lkr-icon" + (extraClass ? " " + extraClass : "");
    span.setAttribute("aria-hidden", "true");
    span.innerHTML = '<svg viewBox="0 0 24 24">' + (ICONS[name] || "") + "</svg>";
    return span;
  }

  /** Substitui elementos `<span data-icon="nome">` do HTML estático pelos SVGs. */
  function hydrateIcons(scope) {
    for (const el of (scope || document).querySelectorAll("[data-icon]")) {
      el.replaceWith(icon(el.dataset.icon, el.className || ""));
    }
  }

  // ---------------------------------------------------------- Formatação

  const brl = new Intl.NumberFormat("pt-BR", { style: "currency", currency: "BRL", maximumFractionDigits: 0 });
  const plain = new Intl.NumberFormat("pt-BR", { maximumFractionDigits: 0 });
  const dateTime = new Intl.DateTimeFormat("pt-BR", { day: "2-digit", month: "2-digit", year: "numeric", hour: "2-digit", minute: "2-digit" });
  const shortDate = new Intl.DateTimeFormat("pt-BR", { day: "2-digit", month: "2-digit", year: "2-digit" });
  const clock = new Intl.DateTimeFormat("pt-BR", { hour: "2-digit", minute: "2-digit" });

  /** "R$ 130–200", "R$ 50+", "R$ 40" ou null quando não há preço. */
  function formatRange(min, max, open) {
    if (min === null || min === undefined) return null;
    const minText = brl.format(min).replace(/ /g, " ");
    if (max === null || max === undefined) return minText + "+";
    if (max === min) return minText + (open ? "+" : "");
    return minText + "–" + plain.format(max) + (open ? "+" : "");
  }

  function formatDate(iso, style) {
    if (!iso) return "";
    const date = new Date(iso);
    if (Number.isNaN(date.getTime())) return "";
    if (style === "short") return shortDate.format(date);
    if (style === "time") return clock.format(date);
    return dateTime.format(date).replace(",", " ·");
  }

  /** Minúsculas e sem acentos, para busca tolerante ("eletronica" encontra "Eletrônica"). */
  function normalizeText(value) {
    return String(value || "")
      .normalize("NFD")
      .replace(/[̀-ͯ]/g, "")
      .toLowerCase()
      .trim();
  }

  function debounce(fn, wait) {
    let timer = null;
    const debounced = (...args) => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        timer = null;
        fn(...args);
      }, wait);
    };
    debounced.cancel = () => clearTimeout(timer);
    return debounced;
  }

  // -------------------------------------------------------- Armazenamento

  /**
   * Adaptador sobre localStorage. Se o navegador bloquear o acesso, cai para
   * memória e informa `available: false` para a interface avisar o usuário.
   */
  function createLocalStorage() {
    try {
      const probe = "__lkr_probe__";
      root.localStorage.setItem(probe, probe);
      root.localStorage.removeItem(probe);
      return {
        available: true,
        get: (key) => root.localStorage.getItem(key),
        set: (key, value) => {
          try {
            root.localStorage.setItem(key, value);
            return true;
          } catch {
            return false;
          }
        },
        remove: (key) => {
          try {
            root.localStorage.removeItem(key);
          } catch {
            /* sem ação */
          }
        },
      };
    } catch {
      const memory = new Map();
      return {
        available: false,
        get: (key) => (memory.has(key) ? memory.get(key) : null),
        set: (key, value) => {
          memory.set(key, value);
          return true;
        },
        remove: (key) => memory.delete(key),
      };
    }
  }

  function downloadJson(filename, data) {
    const blob = new Blob([JSON.stringify(data, null, 2) + "\n"], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const link = h("a", { href: url, download: filename, hidden: true });
    document.body.append(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  // --------------------------------------------------------------- Toasts

  let toastHost = null;

  function toast(message, tone) {
    if (!toastHost) {
      toastHost = h("div", { class: "lkr-toasts", role: "status", "aria-live": "polite" });
      document.body.append(toastHost);
    }
    const iconName = tone === "error" ? "alert" : tone === "success" ? "check" : "info";
    const node = h("div", { class: "lkr-toast" + (tone ? " lkr-toast--" + tone : "") }, icon(iconName), h("span", { text: message }));
    toastHost.append(node);
    setTimeout(() => {
      node.classList.add("is-leaving");
      setTimeout(() => node.remove(), 200);
    }, tone === "error" ? 5200 : 2800);
  }

  // ------------------------------------------------ Diálogo de confirmação

  let confirmEl = null;

  function buildConfirm() {
    const title = h("h2", { class: "lkr-dialog__title", id: "lkr-confirm-title" });
    const eyebrow = h("p", { class: "lkr-dialog__eyebrow" });
    const body = h("div", { class: "lkr-dialog__body", id: "lkr-confirm-body" });
    const cancel = h("button", { class: "lkr-btn", type: "button" }, "Cancelar");
    const accept = h("button", { class: "lkr-btn", type: "submit" });
    const form = h(
      "form",
      { class: "lkr-dialog__form" },
      h("div", { class: "lkr-dialog__head" }, eyebrow, title),
      body,
      h("div", { class: "lkr-dialog__foot" }, cancel, accept),
    );
    const dialog = h("dialog", { class: "lkr-dialog lkr-dialog--sm", "aria-labelledby": "lkr-confirm-title", "aria-describedby": "lkr-confirm-body" }, form);
    const el = { dialog, title, eyebrow, body, accept, cancel, settle: null };

    // A resposta sai direto de cada gesto, sem depender do evento "close".
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      if (el.settle) el.settle(true);
    });
    cancel.addEventListener("click", () => el.settle && el.settle(false));
    dialog.addEventListener("cancel", () => el.settle && el.settle(false)); // Esc
    dialog.addEventListener("click", (event) => {
      if (event.target === dialog && el.settle) el.settle(false); // clique no backdrop
    });
    dialog.addEventListener("close", () => el.settle && el.settle(false));
    document.body.append(dialog);
    return el;
  }

  /**
   * Modal de confirmação no lugar de window.confirm().
   * Resolve true somente quando o usuário confirma explicitamente.
   */
  function confirmDialog({ eyebrow, title, message, confirmLabel, tone }) {
    if (!confirmEl) confirmEl = buildConfirm();
    const el = confirmEl;
    if (el.settle) el.settle(false);
    el.eyebrow.textContent = eyebrow || "CONFIRMAR";
    el.title.textContent = title;
    el.body.replaceChildren(...(Array.isArray(message) ? message : [message]).map((line) => h("p", { text: line })));
    el.accept.textContent = confirmLabel || "Confirmar";
    el.accept.className = "lkr-btn " + (tone === "danger" ? "lkr-btn--danger" : "lkr-btn--primary");
    return new Promise((resolve) => {
      el.settle = (confirmed) => {
        el.settle = null;
        if (el.dialog.open) el.dialog.close();
        resolve(confirmed);
      };
      el.dialog.showModal();
      // Ação destrutiva começa com foco em "Cancelar".
      (tone === "danger" ? el.cancel : el.accept).focus();
    });
  }

  // ------------------------------------------------- bridge local (Git)

  /**
   * Cliente do bridge local (lkr-lab/bridge/server.mjs). Só é usado quando a
   * página é servida pelo próprio bridge: mesma origem, sem CORS e sem
   * nenhuma credencial no navegador — quem autentica no GitHub é o Git da máquina.
   */
  const bridge = {
    reachable: /^https?:$/.test(root.location ? root.location.protocol : ""),

    async request(pathname, { method = "GET", body, timeout = 90000 } = {}) {
      if (!bridge.reachable) return { ok: false, offline: true, error: { code: "OFFLINE", message: "Bridge local indisponível." } };
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), timeout);
      try {
        const response = await fetch(pathname, {
          method,
          headers: Object.assign({ "X-LKR-Lab": "1", Accept: "application/json" }, body ? { "Content-Type": "application/json" } : {}),
          body: body ? JSON.stringify(body) : undefined,
          cache: "no-store",
          credentials: "same-origin",
          signal: controller.signal,
        });
        const data = /application\/json/.test(response.headers.get("content-type") || "") ? await response.json() : null;
        // Sem o marcador, a página está num servidor que não é o bridge (ex.: Vite).
        if (!data || data.bridge !== "lkr-lab") return { ok: false, offline: true, error: { code: "OFFLINE", message: "Bridge local indisponível." } };
        const ok = response.ok && data.success !== false;
        return { ok, status: response.status, data, error: ok ? null : data.error || { code: "UNKNOWN", message: "Falha no bridge." } };
      } catch (error) {
        const aborted = error && error.name === "AbortError";
        return { ok: false, offline: !aborted, error: { code: aborted ? "TIMEOUT" : "OFFLINE", message: aborted ? "O bridge demorou demais para responder." : "Bridge local indisponível." } };
      } finally {
        clearTimeout(timer);
      }
    },
  };

  LKR.core = {
    bridge,
    h,
    icon,
    hydrateIcons,
    formatRange,
    formatDate,
    normalizeText,
    debounce,
    createLocalStorage,
    downloadJson,
    toast,
    confirmDialog,
  };
})(typeof globalThis !== "undefined" ? globalThis : window);
