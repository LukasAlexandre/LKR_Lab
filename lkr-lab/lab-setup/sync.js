/*
 * LKR LAB · Lab Setup — GitHub Sync
 *
 * localStorage continua sendo a persistência instantânea. Esta camada só
 * conversa com o bridge local (npm run lab) quando o usuário pede:
 *
 *   Sincronizar agora      POST /api/lab-sync    → data/lab-setup.json → commit → push
 *   Verificar atualização  GET  /api/git/status?fetch=1  (+ POST /api/lab-update, fast-forward)
 *   Restaurar backup       GET  /api/lab-state   → validação → confirmação → localStorage
 *
 * Detecta alterações pendentes comparando o hash do conteúdo atual com o hash
 * do último estado sincronizado (lastSyncedHash). Nenhuma credencial passa por aqui.
 * Depende de: ../core/lkr-core.js, store.js, app.js
 */
(function () {
  "use strict";

  const { bridge, formatDate, toast, confirmDialog, createLocalStorage } = window.LKR.core;
  const lab = window.LKR.labSetup;
  const app = lab.app;
  const store = app.store;

  // Metadados do sync deste navegador (não fazem parte do estado exportado/versionado).
  const META_KEY = lab.STORAGE_KEY + ":sync";
  const BRIDGE_URL = "http://127.0.0.1:4317/lab-setup/";
  const metaStorage = createLocalStorage();

  const $ = (id) => document.getElementById(id);
  const els = {
    root: $("ls-sync"),
    trigger: $("ls-sync-trigger"),
    pop: $("ls-sync-pop"),
    repo: $("ls-sync-repo"),
    branch: $("ls-sync-branch"),
    state: $("ls-sync-state"),
    last: $("ls-sync-last"),
    commit: $("ls-sync-commit"),
    notice: $("ls-sync-notice"),
    now: $("ls-sync-now"),
    check: $("ls-sync-check"),
    restore: $("ls-sync-restore"),
    undo: $("ls-sync-undo"),
    undoLabel: $("ls-sync-undo-label"),
  };

  let meta = loadMeta(); // { lastSyncedHash, lastSuccess: {at, commit, branch, repo}, lastAttempt: {at, ok, code, message} }
  let remote = null; // resposta mais recente de /api/git/status
  let online = null; // null = ainda não verificado
  let busy = null; // "sync" | "check" | "restore"
  let failed = false; // última tentativa falhou e nada mudou desde então

  function loadMeta() {
    try {
      const value = JSON.parse(metaStorage.get(META_KEY));
      return value && typeof value === "object" && !Array.isArray(value) ? value : {};
    } catch {
      return {};
    }
  }

  function saveMeta() {
    metaStorage.set(META_KEY, JSON.stringify(meta));
  }

  const plural = (count, one, many) => count + " " + (count === 1 ? one : many);

  // ================================================================ estado

  function isEmptyState() {
    return !store.state.custom.length && Object.values(store.state.items).every((e) => !e.completed && !e.notes);
  }

  /** O arquivo do repositório já contém exatamente o conteúdo local e está publicado. */
  function repoMatchesLocal(hash) {
    return Boolean(remote && remote.repository && remote.backup && remote.backup.stateHash === hash && !remote.labStateModified && remote.ahead === 0);
  }

  /** null = só "Salvo localmente" (sem histórico de sync e nada a enviar). */
  function syncState() {
    if (busy === "sync") return "syncing";
    if (failed) return "error";
    const hash = store.contentHash();
    if (meta.lastSyncedHash === hash || repoMatchesLocal(hash)) return "synced";
    if (meta.lastSyncedHash) return "unsynced";
    if (!online) return null;
    return isEmptyState() && !(remote && remote.backup && remote.backup.exists) ? null : "unsynced";
  }

  const STATE_LABEL = {
    synced: "✓ Sincronizado",
    unsynced: "● Alterações locais não sincronizadas",
    syncing: "◌ Sincronizando…",
    error: "! Não foi possível sincronizar",
  };

  function render() {
    const state = syncState();
    app.setSyncIndicator(state);
    els.root.dataset.state = state || "local";

    const info = remote && remote.repository ? remote : null;
    els.repo.textContent = (info && info.repo) || (meta.lastSuccess && meta.lastSuccess.repo) || "—";
    els.branch.textContent = (info && info.branch) || (meta.lastSuccess && meta.lastSuccess.branch) || "—";
    els.state.textContent = online === false ? "Bridge local desligado" : STATE_LABEL[state] || "● Salvo localmente";
    els.state.dataset.state = online === false ? "offline" : state || "local";

    const lastCommit = info && info.lastBackupCommit;
    const lastAt = (lastCommit && lastCommit.date) || (meta.lastSuccess && meta.lastSuccess.at);
    els.last.textContent = lastAt ? formatDate(lastAt) : "—";
    els.commit.textContent = (lastCommit && lastCommit.hash) || (meta.lastSuccess && meta.lastSuccess.commit) || "—";

    renderNotice();

    const blocked = !online || Boolean(busy) || Boolean(info === null && online);
    els.now.disabled = blocked;
    els.check.disabled = blocked;
    els.restore.disabled = blocked || !(info && info.backup && info.backup.exists);
    els.now.lastElementChild.textContent = busy === "sync" ? "Sincronizando…" : "Sincronizar agora";
    els.check.lastElementChild.textContent = busy === "check" ? "Verificando…" : "Verificar atualização";
    els.restore.lastElementChild.textContent = busy === "restore" ? "Carregando…" : "Restaurar backup";

    const snapshot = store.getSnapshot();
    els.undo.hidden = !snapshot;
    if (snapshot) {
      els.undoLabel.textContent = "Desfazer " + (snapshot.reason === "restore" ? "restauração" : "importação") + " de " + formatDate(snapshot.takenAt);
    }
  }

  function renderNotice() {
    let text = "";
    let tone = "";
    if (online === false) {
      text =
        (location.protocol === "file:" ? "Página aberta como arquivo local." : "Esta página não está sendo servida pelo bridge do LKR LAB.") +
        " Para usar o GitHub Sync, rode “npm run lab” e abra " + BRIDGE_URL +
        ". O localStorage é separado por endereço: use Exportar/Importar para levar os dados até lá.";
    } else if (remote && !remote.repository) {
      text = (remote.error && remote.error.message) || "Repositório Git não encontrado.";
      tone = "error";
    } else if (remote && remote.error) {
      text = remote.error.message;
      tone = "error";
    } else if (remote && remote.repository && !remote.remote) {
      text = "O repositório não tem o remote “origin” configurado.";
      tone = "error";
    } else if (failed && meta.lastAttempt) {
      text = meta.lastAttempt.message;
      tone = "error";
    } else if (remote && remote.behind > 0) {
      text = "O GitHub tem " + plural(remote.behind, "commit mais recente", "commits mais recentes") + ". Use “Verificar atualização”.";
      tone = "warning";
    } else if (remote && remote.dirty) {
      text = "Há outras alterações no projeto. O sync commita somente " + remote.backup.file + ".";
    }
    els.notice.hidden = !text;
    els.notice.textContent = text;
    els.notice.dataset.tone = tone;
  }

  async function refreshStatus(fetchRemote) {
    const res = await bridge.request("/api/git/status" + (fetchRemote ? "?fetch=1" : ""));
    online = !res.offline;
    remote = res.data || null;
    return res;
  }

  // ================================================================ ações

  async function syncNow() {
    if (busy) return;
    app.flushNotes();
    busy = "sync";
    failed = false;
    render();
    const res = await bridge.request("/api/lab-sync", { method: "POST", body: { schemaVersion: lab.SCHEMA_VERSION, state: store.persistable() } });
    busy = null;
    const at = new Date().toISOString();
    if (res.ok) {
      const data = res.data;
      meta.lastSyncedHash = data.stateHash;
      meta.lastSuccess = { at: data.timestamp || at, commit: data.commit, branch: data.branch, repo: data.repo };
      meta.lastAttempt = { at, ok: true, code: data.alreadySynced ? "UP_TO_DATE" : "PUSHED", message: data.message };
      saveMeta();
      toast(data.alreadySynced ? "Já está sincronizado." : "Sincronizado com GitHub · commit " + data.commit, "success");
    } else {
      if (res.offline) online = false;
      failed = true;
      meta.lastAttempt = { at, ok: false, code: res.error.code, message: res.error.message };
      saveMeta();
      toast(res.error.message, "error");
    }
    await refreshStatus(false);
    render();
  }

  async function checkUpdate() {
    if (busy) return;
    busy = "check";
    render();
    const res = await refreshStatus(true);
    busy = null;
    render();
    if (res.offline) return toast("Bridge local indisponível.", "error");
    const data = res.data || {};
    if (!data.repository || data.error) return toast((data.error && data.error.message) || "Repositório Git não encontrado.", "error");
    if (data.fetchError) return toast(data.fetchError.message, "error");
    if (!data.remoteBranch) return toast("A branch " + data.branch + " ainda não existe no GitHub.", "error");
    if (data.behind > 0 && data.ahead > 0) {
      return toast("A branch local e a do GitHub divergiram. Resolva no terminal (sem force) antes de continuar.", "error");
    }
    if (!data.behind) return toast("O repositório local já está atualizado com o GitHub.", "success");

    const confirmed = await confirmDialog({
      eyebrow: "ATUALIZAR DO GITHUB",
      title: "Trazer as alterações do GitHub?",
      message: [
        "Há " + plural(data.behind, "commit novo", "commits novos") + " em origin/" + data.branch + ".",
        "O repositório local será atualizado somente por fast-forward: nenhum arquivo local é descartado. Se houver risco de conflito, a operação é interrompida sem alterar nada.",
        "Os dados deste navegador não mudam agora; depois você decide se restaura o backup.",
      ],
      confirmLabel: "Atualizar",
    });
    if (!confirmed) return;

    busy = "check";
    render();
    const update = await bridge.request("/api/lab-update", { method: "POST", body: {} });
    busy = null;
    await refreshStatus(false);
    render();
    if (!update.ok) return toast(update.error.message, "error");
    toast(update.data.message, "success");
    if (update.data.labStateChanged) await restoreBackup();
  }

  async function restoreBackup() {
    if (busy) return;
    busy = "restore";
    render();
    const res = await bridge.request("/api/lab-state");
    busy = null;
    render();
    if (!res.ok) return toast(res.error.message, "error");

    const data = res.data;
    const preview = store.previewImport(data.backup);
    if (!preview.ok) return toast("Backup do repositório inválido: " + preview.error, "error");
    const s = preview.summary;
    const commit = data.commit ? " · commit " + data.commit.hash : "";
    const confirmed = await confirmDialog({
      eyebrow: "RESTAURAR BACKUP",
      title: "Restaurar backup do repositório?",
      message: [
        "Esta ação substituirá o estado atual do Lab Setup pelo último backup salvo no repositório.",
        "Backup de " + (formatDate(data.backup.updatedAt) || "data desconhecida") + commit + " · branch " + data.branch + ".",
        "Contém " + plural(s.completed, "item concluído", "itens concluídos") + ", " + plural(s.notes, "observação", "observações") + " e " +
          plural(s.custom, "item personalizado", "itens personalizados") + ".",
        "O estado atual fica guardado neste navegador e pode ser recuperado em “Desfazer restauração”.",
      ],
      confirmLabel: "Restaurar",
      tone: "danger",
    });
    if (!confirmed) return;

    const result = app.replaceState(data.backup, "restore");
    if (!result.ok) return toast(result.error || "Não foi possível restaurar o backup.", "error");
    failed = false;
    if (data.synced && store.contentHash() === data.stateHash) {
      meta.lastSyncedHash = data.stateHash;
      saveMeta();
    }
    render();
    toast("Backup restaurado.", "success");
  }

  async function undoRestore() {
    const snapshot = store.getSnapshot();
    if (!snapshot) return;
    const confirmed = await confirmDialog({
      eyebrow: "DESFAZER",
      title: "Voltar ao estado anterior?",
      message: [
        "Recupera o estado que existia antes da " + (snapshot.reason === "restore" ? "restauração" : "importação") + " de " + formatDate(snapshot.takenAt) + ".",
        "O estado atual do Lab Setup será substituído.",
      ],
      confirmLabel: "Desfazer",
      tone: "danger",
    });
    if (!confirmed) return;
    const result = app.replaceState(null, "undo");
    if (!result.ok) return toast(result.error || "Não foi possível desfazer.", "error");
    render();
    toast("Estado anterior recuperado.", "success");
  }

  // =============================================================== popover

  function isOpen() {
    return !els.pop.hidden;
  }

  function open() {
    els.pop.hidden = false;
    els.trigger.setAttribute("aria-expanded", "true");
    render();
    if (bridge.reachable) refreshStatus(false).then(render);
    else {
      online = false;
      render();
    }
  }

  function close(returnFocus) {
    if (!isOpen()) return;
    els.pop.hidden = true;
    els.trigger.setAttribute("aria-expanded", "false");
    if (returnFocus) els.trigger.focus();
  }

  function bind() {
    els.trigger.addEventListener("click", () => (isOpen() ? close(false) : open()));
    els.now.addEventListener("click", syncNow);
    els.check.addEventListener("click", checkUpdate);
    els.restore.addEventListener("click", restoreBackup);
    els.undo.addEventListener("click", undoRestore);

    // Fecha ao clicar fora, exceto quando um modal de confirmação está aberto por cima.
    document.addEventListener("pointerdown", (event) => {
      if (isOpen() && !els.root.contains(event.target) && !event.target.closest("dialog")) close(false);
    });
    els.root.addEventListener("keydown", (event) => {
      if (event.key === "Escape" && isOpen()) {
        event.stopPropagation();
        close(true);
      }
    });

    app.onPersist(() => {
      failed = false;
      render();
    });
  }

  bind();
  if (!bridge.reachable) online = false;
  render();
  if (bridge.reachable) refreshStatus(false).then(render);
})();
