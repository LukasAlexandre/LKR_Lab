/*
 * LKR LAB · Lab Setup — estado portátil e GitHub Sync
 *
 * O cache local (localStorage, via store.js) continua sendo o autosave
 * instantâneo. O estado portátil é data/lab-setup.json no repositório, lido e
 * escrito somente pelo bridge local (npm run lab):
 *
 *   Abrir / voltar à aba   GET  /api/lab-state   → reconciliação (lkr-portable.js)
 *   Sincronizar agora      POST /api/lab-sync    → data/lab-setup.json → commit → push
 *   Verificar atualização  GET  /api/git/status?fetch=1  (+ POST /api/lab-update, fast-forward)
 *   Restaurar backup       GET  /api/lab-state   → validação → confirmação → cache local
 *
 * Reconciliação: se só o repositório mudou (máquina nova, git pull), o estado
 * portátil é adotado automaticamente (com "Desfazer"); se só este navegador
 * mudou, fica "aguardando sync"; se os dois mudaram, nada é sobrescrito e o
 * usuário escolhe a versão. Publicar no Git é sempre uma ação explícita.
 *
 * Os metadados de sync são desta máquina (nunca vão para o Git).
 * Nenhuma credencial passa por aqui.
 * Depende de: ../core/lkr-core.js, ../core/lkr-portable.js, store.js, app.js
 */
(function () {
  "use strict";

  const { bridge, formatDate, toast, confirmDialog, createLocalStorage } = window.LKR.core;
  const portable = window.LKR.portable;
  const lab = window.LKR.labSetup;
  const app = lab.app;
  const store = app.store;

  const META_KEY = lab.STORAGE_KEY + ":sync";
  const BRIDGE_URL = "http://127.0.0.1:4317/lab-setup/";
  const RECHECK_MIN_MS = 3000;
  const metaStore = portable.createMetaStore(createLocalStorage(), META_KEY);

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
    conflict: $("ls-sync-conflict"),
    takeRepo: $("ls-sync-take-repo"),
    keepLocal: $("ls-sync-keep-local"),
    now: $("ls-sync-now"),
    check: $("ls-sync-check"),
    restore: $("ls-sync-restore"),
    undo: $("ls-sync-undo"),
    undoLabel: $("ls-sync-undo-label"),
  };

  let remote = null; // resposta mais recente de /api/git/status
  let online = null; // null = ainda não verificado
  let busy = null; // "sync" | "check" | "restore"
  let failed = false; // última tentativa falhou e nada mudou desde então
  let conflict = null; // { hash, commit, updatedAt }: repositório e navegador mudaram
  let portableInvalid = false; // data/lab-setup.json existe mas está ilegível
  let lastCheck = 0;

  const meta = () => metaStore.get();
  const plural = (count, one, many) => count + " " + (count === 1 ? one : many);

  // ================================================================ estado

  /** O arquivo do repositório já contém exatamente o conteúdo local e está publicado. */
  function repoMatchesLocal(hash) {
    return Boolean(remote && remote.repository && remote.backup && remote.backup.stateHash === hash && !remote.labStateModified && remote.ahead === 0);
  }

  /** null = só "Salvo localmente" (sem histórico de sync e nada a enviar). */
  function syncState() {
    if (busy === "sync") return "syncing";
    if (failed) return "error";
    if (conflict) return "conflict";
    const hash = store.contentHash();
    if (remote && remote.repository && remote.backup) {
      if (repoMatchesLocal(hash)) return "synced";
      return store.isEmpty() && !remote.backup.exists ? null : "unsynced";
    }
    // Sem resposta do bridge: usa o que esta máquina sabe da última reconciliação.
    if (meta().baseHash === hash) return "synced";
    if (meta().baseHash) return "unsynced";
    return null;
  }

  const STATE_LABEL = {
    synced: "✓ Sincronizado",
    unsynced: "● Alterações locais não sincronizadas",
    syncing: "◌ Sincronizando…",
    conflict: "! Conflito com o repositório",
    error: "! Não foi possível sincronizar",
  };

  function render() {
    const state = syncState();
    app.setSyncIndicator(state);
    els.root.dataset.state = state || "local";

    const info = remote && remote.repository ? remote : null;
    const last = meta().lastSuccess;
    els.repo.textContent = (info && info.repo) || (last && last.repo) || "—";
    els.branch.textContent = (info && info.branch) || (last && last.branch) || "—";
    els.state.textContent = online === false ? "Bridge local desligado" : STATE_LABEL[state] || "● Salvo localmente";
    els.state.dataset.state = online === false ? "offline" : state || "local";

    const lastCommit = info && info.lastBackupCommit;
    const lastAt = (lastCommit && lastCommit.date) || (last && last.at);
    els.last.textContent = lastAt ? formatDate(lastAt) : "—";
    els.commit.textContent = (lastCommit && lastCommit.hash) || (last && last.commit) || "—";

    renderNotice();
    els.conflict.hidden = !conflict;

    const blocked = !online || Boolean(busy) || Boolean(info === null && online);
    els.now.disabled = blocked || Boolean(conflict) || portableInvalid;
    els.check.disabled = blocked;
    els.restore.disabled = blocked || !(info && info.backup && info.backup.exists && info.backup.valid);
    els.takeRepo.disabled = Boolean(busy);
    els.keepLocal.disabled = Boolean(busy);
    els.now.lastElementChild.textContent = busy === "sync" ? "Sincronizando…" : "Sincronizar agora";
    els.check.lastElementChild.textContent = busy === "check" ? "Verificando…" : "Verificar atualização";
    els.restore.lastElementChild.textContent = busy === "restore" ? "Carregando…" : "Restaurar backup";

    const snapshot = store.getSnapshot();
    els.undo.hidden = !snapshot;
    if (snapshot) els.undoLabel.textContent = "Desfazer " + reasonLabel(snapshot.reason) + " de " + formatDate(snapshot.takenAt);
  }

  function reasonLabel(reason) {
    if (reason === "restore") return "restauração";
    if (reason === "repo") return "atualização do repositório";
    return "importação";
  }

  function renderNotice() {
    let text = "";
    let tone = "";
    const file = (remote && remote.backup && remote.backup.file) || "data/lab-setup.json";
    if (online === false) {
      text =
        (location.protocol === "file:" ? "Página aberta como arquivo local." : "Esta página não está sendo servida pelo bridge do LKR LAB.") +
        " Para usar o estado portátil, rode “npm run lab” e abra " + BRIDGE_URL +
        ". O localStorage é separado por endereço: use Exportar/Importar para levar os dados até lá.";
    } else if (conflict) {
      text =
        "O " + file + " mudou no repositório" + (conflict.commit ? " (commit " + conflict.commit.hash + ")" : "") +
        " e este navegador também tem alterações não publicadas. Nada foi sobrescrito: escolha qual versão manter.";
      tone = "warning";
    } else if (portableInvalid) {
      text = "O arquivo " + file + " do repositório está inválido. Nada foi sobrescrito; corrija-o no Git ou sincronize depois de corrigir.";
      tone = "error";
    } else if (remote && !remote.repository) {
      text = (remote.error && remote.error.message) || "Repositório Git não encontrado.";
      tone = "error";
    } else if (remote && remote.error) {
      text = remote.error.message;
      tone = "error";
    } else if (remote && remote.repository && !remote.remote) {
      text = "O repositório não tem o remote “origin” configurado.";
      tone = "error";
    } else if (failed && meta().lastAttempt) {
      text = meta().lastAttempt.message;
      tone = "error";
    } else if (remote && remote.behind > 0) {
      text = "O GitHub tem " + plural(remote.behind, "commit mais recente", "commits mais recentes") + ". Use “Verificar atualização”.";
      tone = "warning";
    } else if (remote && remote.dirty) {
      text = "Há outras alterações no projeto. O sync commita somente " + file + ".";
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

  // ======================================================== reconciliação

  /**
   * Compara o cache local com data/lab-setup.json e aplica a decisão de
   * lkr-portable.js. Nunca escreve no repositório.
   */
  async function reconcileWithRepo() {
    if (busy || !bridge.reachable) return;
    lastCheck = Date.now();
    const res = await bridge.request("/api/lab-state");
    if (res.offline) {
      online = false;
      return render();
    }
    const file = portable.fileFromResponse(res);
    if (!file || busy) return render(); // erro de Git: o status já informa
    app.flushNotes();
    portableInvalid = file.status === "invalid";
    const decision = portable.reconcile({ localHash: store.contentHash(), localEmpty: store.isEmpty(), baseHash: meta().baseHash, file });
    conflict = decision.status === "conflict" ? { hash: file.hash, commit: res.data.commit, updatedAt: res.data.backup.updatedAt } : null;
    if (!meta().migratedAt) metaStore.update({ migratedAt: new Date().toISOString() });

    if (decision.action === "mark-base") {
      metaStore.update({ baseHash: file.hash });
    } else if (decision.action === "adopt") {
      const firstLoad = store.isEmpty();
      const result = app.replaceState(res.data.backup, "repo");
      if (result.ok) {
        metaStore.update({ baseHash: file.hash });
        const commit = res.data.commit ? " · commit " + res.data.commit.hash : "";
        toast((firstLoad ? "Estado restaurado do repositório" : "Atualizado com a versão do repositório") + commit + ".", "success");
      } else {
        toast(result.error || "Não foi possível aplicar o estado do repositório.", "error");
      }
    }
    render();
  }

  function recheckSoon() {
    if (document.visibilityState !== "visible" || Date.now() - lastCheck < RECHECK_MIN_MS) return;
    refreshStatus(false).then(reconcileWithRepo);
  }

  // ================================================================ ações

  async function syncNow() {
    if (busy || conflict) return;
    app.flushNotes();
    busy = "sync";
    failed = false;
    render();
    const res = await bridge.request("/api/lab-sync", { method: "POST", body: { schemaVersion: lab.SCHEMA_VERSION, state: store.persistable() } });
    busy = null;
    const at = new Date().toISOString();
    if (res.ok) {
      const data = res.data;
      metaStore.update({
        baseHash: data.stateHash,
        lastSuccess: { at: data.timestamp || at, commit: data.commit, branch: data.branch, repo: data.repo },
        lastAttempt: { at, ok: true, code: data.alreadySynced ? "UP_TO_DATE" : "PUSHED", message: data.message },
      });
      toast(data.alreadySynced ? "Já está sincronizado." : "Sincronizado com GitHub · commit " + data.commit, "success");
    } else {
      if (res.offline) online = false;
      failed = true;
      metaStore.update({ lastAttempt: { at, ok: false, code: res.error.code, message: res.error.message } });
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
        "Se este navegador não tiver alterações pendentes, o Lab Setup passa a usar a nova versão (com opção de desfazer). Caso contrário, você escolhe qual manter.",
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
    if (update.data.labStateChanged) await reconcileWithRepo();
  }

  /** Substitui o cache local pelo arquivo do repositório, com confirmação. */
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
    conflict = null;
    metaStore.update({ baseHash: data.stateHash });
    render();
    toast("Backup restaurado.", "success");
  }

  /** Conflito resolvido a favor deste navegador: o cache vira "aguardando sync". */
  function keepLocal() {
    if (!conflict || busy) return;
    metaStore.update({ baseHash: conflict.hash });
    conflict = null;
    render();
    toast("Mantida a versão deste navegador. Use “Sincronizar agora” para publicá-la.", "success");
  }

  async function undoRestore() {
    const snapshot = store.getSnapshot();
    if (!snapshot) return;
    const confirmed = await confirmDialog({
      eyebrow: "DESFAZER",
      title: "Voltar ao estado anterior?",
      message: [
        "Recupera o estado que existia antes da " + reasonLabel(snapshot.reason) + " de " + formatDate(snapshot.takenAt) + ".",
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
    els.takeRepo.addEventListener("click", restoreBackup);
    els.keepLocal.addEventListener("click", keepLocal);
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

    // Voltar à aba (ex.: depois de um git pull no terminal) confere o repositório de novo.
    if (bridge.reachable) document.addEventListener("visibilitychange", recheckSoon);
  }

  bind();
  if (!bridge.reachable) online = false;
  render();
  if (bridge.reachable) refreshStatus(false).then(reconcileWithRepo);
})();
