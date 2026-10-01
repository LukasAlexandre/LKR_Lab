/*
 * LKR LAB · Lab Setup — interface
 * Renderização, eventos, autosave, filtros, modais e backup.
 * Depende de: ../core/lkr-core.js, catalog.js, store.js
 */
(function () {
  "use strict";

  const { h, icon, hydrateIcons, formatRange, formatDate, normalizeText, debounce, createLocalStorage, downloadJson, toast, confirmDialog } =
    window.LKR.core;
  const { catalog, createStore } = window.LKR.labSetup;

  const NOTE_DEBOUNCE_MS = 400;
  const SAVED_FLASH_MS = 1800;

  const store = createStore({ catalog, storage: createLocalStorage() });
  const categoryById = new Map(catalog.categories.map((c) => [c.id, c]));

  const view = {
    status: store.state.ui.status,
    category: store.state.ui.category,
    query: "",
  };

  const $ = (id) => document.getElementById(id);
  const els = {
    save: $("ls-save"),
    saveText: document.querySelector("#ls-save .ls-save__text"),
    storageWarning: $("ls-storage-warning"),
    done: $("ls-done"),
    total: $("ls-total"),
    pct: $("ls-pct"),
    bar: $("ls-bar"),
    updated: $("ls-updated"),
    statDone: $("ls-stat-done"),
    statPending: $("ls-stat-pending"),
    statCategories: $("ls-stat-categories"),
    statCategoriesMeta: $("ls-stat-categories-meta"),
    statCost: $("ls-stat-cost"),
    cats: $("ls-cats"),
    costTotal: $("ls-cost-total"),
    costDone: $("ls-cost-done"),
    costPending: $("ls-cost-pending"),
    costSplit: $("ls-cost-split"),
    costNote: $("ls-cost-note"),
    toolbar: $("ls-toolbar"),
    status: $("ls-status"),
    chips: $("ls-chip-list"),
    search: $("ls-search"),
    add: $("ls-add"),
    results: $("ls-results"),
    list: $("ls-items"),
    exportBtn: $("ls-export"),
    importBtn: $("ls-import"),
    importFile: $("ls-import-file"),
    resetBtn: $("ls-reset"),
    addDialog: $("ls-add-dialog"),
    addForm: $("ls-add-form"),
    addCancel: $("ls-add-cancel"),
    addCategory: $("ls-add-category"),
    addPriority: $("ls-add-priority"),
    nameError: $("ls-add-name-error"),
    priceError: $("ls-add-price-error"),
  };

  const STATUS_OPTIONS = [
    { id: "all", label: "Todos" },
    { id: "pending", label: "Pendentes" },
    { id: "done", label: "Concluídos" },
  ];

  // ================================================================ helpers

  const plural = (count, one, many) => count + " " + (count === 1 ? one : many);

  function setProgress(el, pct) {
    el.style.setProperty("--progress", String(Math.max(0, Math.min(1, pct / 100))));
  }

  function priceText(item) {
    return formatRange(item.priceMin, item.priceMax);
  }

  function costText(range) {
    if (!range.priced) return "R$ 0";
    return formatRange(range.min, range.max, range.open);
  }

  function matchesQuery(item, query) {
    if (!query) return true;
    const category = categoryById.get(item.category);
    const haystack = normalizeText(
      [item.name, item.description, item.hint, item.quantity, category && category.title, category && category.label, item.notes].join(" "),
    );
    return query.split(/\s+/).every((term) => haystack.includes(term));
  }

  function matchesFilters(item) {
    if (view.status === "pending" && item.completed) return false;
    if (view.status === "done" && !item.completed) return false;
    if (view.category !== "all" && item.category !== view.category) return false;
    return matchesQuery(item, normalizeText(view.query));
  }

  function hasActiveFilters() {
    return view.status !== "all" || view.category !== "all" || view.query.trim() !== "";
  }

  // ============================================================ save status

  /*
   * O indicador do topo combina duas camadas:
   *  - local: autosave no cache local (sempre instantâneo);
   *  - sync:  estado portátil/GitHub, informado por sync.js (null = sem sincronização).
   * Problema local tem prioridade; depois vem o estado do GitHub.
   */
  const indicator = { local: store.state.updatedAt ? "saved" : "idle", sync: null };
  const persistListeners = [];

  const INDICATOR_TEXT = {
    saving: "Salvando…",
    "local-error": "Falha ao salvar",
    syncing: "Sincronizando…",
    "sync-error": "Não foi possível sincronizar",
    conflict: "Conflito com o repositório",
    unsynced: "Alterações não sincronizadas",
    synced: "Sincronizado com GitHub",
    reset: "Checklist resetado",
    idle: "Salvo localmente",
  };

  function renderIndicator() {
    let state;
    if (indicator.local === "saving") state = "saving";
    else if (indicator.local === "error") state = "local-error";
    else if (indicator.sync === "syncing") state = "syncing";
    else if (indicator.sync === "error") state = "sync-error";
    else if (indicator.sync === "conflict") state = "conflict";
    else if (indicator.sync === "unsynced") state = "unsynced";
    else if (indicator.sync === "synced") state = "synced";
    else if (indicator.local === "reset") state = "reset";
    else state = indicator.local === "saved" ? "saved" : "idle";
    const text =
      state === "saved" ? "Salvo localmente · " + formatDate(store.lastWrite.at || store.state.updatedAt || new Date().toISOString(), "time") : INDICATOR_TEXT[state];
    els.save.dataset.state = state;
    els.saveText.textContent = text;
    els.save.title = text;
  }

  /** Chamado depois de qualquer gravação local; avisa a camada de sync. */
  function notifyPersist() {
    for (const listener of persistListeners) listener();
  }

  function reportSave(ok) {
    indicator.local = ok ? "saved" : "error";
    if (!ok) toast("Não foi possível salvar no armazenamento local do navegador.", "error");
    renderUpdated();
    notifyPersist();
    renderIndicator();
  }

  function reportSaving() {
    indicator.local = "saving";
    renderIndicator();
  }

  // ====================================================== notas (autosave)

  const pendingNotes = new Map();

  function scheduleNote(id, value, card) {
    const pending = pendingNotes.get(id);
    if (pending) clearTimeout(pending.timer);
    pendingNotes.set(id, { value, card, timer: setTimeout(() => commitNote(id), NOTE_DEBOUNCE_MS) });
    reportSaving();
    const saved = card.querySelector(".ls-item__saved");
    saved.textContent = "Salvando…";
    saved.dataset.state = "saving";
    saved.classList.add("is-visible");
  }

  function commitNote(id) {
    const pending = pendingNotes.get(id);
    if (!pending) return;
    clearTimeout(pending.timer);
    pendingNotes.delete(id);
    const ok = store.setNotes(id, pending.value);
    reportSave(ok);
    const saved = pending.card.querySelector(".ls-item__saved");
    if (!saved) return;
    saved.textContent = ok ? "Salvo localmente" : "Falha ao salvar";
    saved.dataset.state = ok ? "saved" : "error";
    clearTimeout(saved._hide);
    saved._hide = setTimeout(() => saved.classList.remove("is-visible"), SAVED_FLASH_MS);
  }

  /** Grava imediatamente toda observação em espera (antes de re-renderizar ou sair). */
  function flushNotes() {
    for (const id of Array.from(pendingNotes.keys())) commitNote(id);
  }

  function autoGrow(textarea) {
    textarea.style.height = "auto";
    textarea.style.height = textarea.scrollHeight + 2 + "px";
  }

  // ============================================================== resumo

  function renderUpdated() {
    const at = store.state.updatedAt;
    els.updated.textContent = at ? "Última atualização em " + formatDate(at) : "Nenhuma alteração registrada ainda.";
  }

  function renderSummary() {
    const stats = store.stats();

    els.done.textContent = stats.done;
    els.total.textContent = stats.total;
    els.pct.textContent = stats.pct + "%";
    els.bar.setAttribute("aria-valuenow", String(stats.pct));
    els.bar.setAttribute("aria-valuetext", stats.done + " de " + stats.total + " itens concluídos");
    els.bar.classList.toggle("ls-bar--done", stats.total > 0 && stats.done === stats.total);
    setProgress(els.bar, stats.pct);

    els.statDone.textContent = stats.done;
    els.statPending.textContent = stats.pending;
    els.statCategories.textContent = stats.categories;
    els.statCategoriesMeta.textContent = stats.categoriesComplete ? stats.categoriesComplete + " completas" : "";
    els.statCost.textContent = costText(stats.cost.total);

    els.costTotal.textContent = costText(stats.cost.total);
    els.costDone.textContent = costText(stats.cost.done);
    els.costPending.textContent = costText(stats.cost.pending);
    const totalMid = stats.cost.total.min + stats.cost.total.max;
    setProgress(els.costSplit, totalMid ? ((stats.cost.done.min + stats.cost.done.max) / totalMid) * 100 : 0);
    const noteParts = [stats.cost.total.priced + " de " + stats.total + " itens com faixa de preço."];
    if (stats.cost.total.open) noteParts.push("“+” indica valor a partir de.");
    els.costNote.textContent = noteParts.join(" ");

    renderCategoryOverview(stats);
    renderFilterCounts(stats);
    renderSectionHeads(stats);
    renderUpdated();
    return stats;
  }

  function renderCategoryOverview(stats) {
    const tiles = stats.byCategory.map((c) => {
      const category = categoryById.get(c.id);
      const bar = h("div", { class: "ls-bar" + (c.total && c.done === c.total ? " ls-bar--done" : "") }, h("i", { class: "ls-bar__fill" }));
      setProgress(bar, c.pct);
      return h(
        "button",
        {
          class: "ls-cat",
          type: "button",
          "aria-pressed": String(view.category === c.id),
          "aria-label": category.title + ": " + c.done + " de " + c.total + " concluídos, " + c.pct + "%",
          dataset: { category: c.id },
        },
        h(
          "span",
          { class: "ls-cat__top" },
          h("span", { class: "ls-cat__code", text: category.code }),
          h("span", { class: "ls-cat__name", text: category.label }),
          h("span", { class: "ls-cat__pct", text: c.pct + "%" }),
        ),
        bar,
        h("span", { class: "ls-cat__count", text: c.done + " / " + c.total + " concluídos" }),
      );
    });
    els.cats.replaceChildren(...tiles);
  }

  // ============================================================== filtros

  function buildFilters() {
    els.status.replaceChildren(
      ...STATUS_OPTIONS.map((option) =>
        h(
          "button",
          { type: "button", dataset: { status: option.id }, "aria-pressed": "false" },
          option.label,
          h("span", { class: "ls-count", dataset: { count: option.id } }),
        ),
      ),
    );
    const chips = [{ id: "all", label: "Todas" }].concat(catalog.categories.map((c) => ({ id: c.id, label: c.label })));
    els.chips.replaceChildren(
      ...chips.map((chip) =>
        h(
          "button",
          { class: "ls-chip", type: "button", dataset: { category: chip.id }, "aria-pressed": "false" },
          chip.label,
          h("span", { class: "ls-count", dataset: { count: chip.id } }),
        ),
      ),
    );
    els.addCategory.replaceChildren(...catalog.categories.map((c) => h("option", { value: c.id, text: c.code + " — " + c.title })));
    els.addPriority.replaceChildren(...Object.entries(catalog.priorities).map(([id, p]) => h("option", { value: id, text: p.label })));
  }

  function renderFilterState() {
    for (const button of els.status.querySelectorAll("button")) {
      button.setAttribute("aria-pressed", String(button.dataset.status === view.status));
    }
    for (const button of els.chips.querySelectorAll("button")) {
      button.setAttribute("aria-pressed", String(button.dataset.category === view.category));
    }
    for (const tile of els.cats.querySelectorAll(".ls-cat")) {
      tile.setAttribute("aria-pressed", String(tile.dataset.category === view.category));
    }
  }

  function renderFilterCounts(stats) {
    const counts = { all: stats.total, pending: stats.pending, done: stats.done };
    for (const el of els.status.querySelectorAll("[data-count]")) el.textContent = counts[el.dataset.count];
    const byCategory = new Map(stats.byCategory.map((c) => [c.id, c.total]));
    for (const el of els.chips.querySelectorAll("[data-count]")) {
      el.textContent = el.dataset.count === "all" ? stats.total : byCategory.get(el.dataset.count);
    }
  }

  function setFilter(patch, options) {
    flushNotes();
    Object.assign(view, patch);
    store.setUi({ status: view.status, category: view.category });
    renderFilterState();
    renderList();
    if (options && options.scroll) scrollToList();
  }

  function clearFilters() {
    els.search.value = "";
    setFilter({ status: "all", category: "all", query: "" });
  }

  function scrollToList() {
    const top = els.toolbar.getBoundingClientRect().top + window.scrollY - parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--topbar-height"));
    if (window.scrollY > top + 1 || window.scrollY < top - 1) window.scrollTo({ top });
  }

  // ================================================================ lista

  function renderBadge(item) {
    const priority = catalog.priorities[item.priority] || catalog.priorities.media;
    return h(
      "span",
      { class: "ls-badge ls-badge--" + item.priority, title: "Prioridade " + priority.label.toLowerCase() },
      priority.label,
      item.priorityNote ? h("em", { text: item.priorityNote }) : null,
    );
  }

  function statusText(item) {
    return item.completed ? "Concluído" + (item.completedAt ? " em " + formatDate(item.completedAt, "short") : "") : "Pendente";
  }

  function renderCard(item) {
    const checkboxId = "ls-check-" + item.id;
    const notesId = "ls-notes-" + item.id;
    const price = priceText(item);

    const checkbox = h("input", { type: "checkbox", id: checkboxId, checked: item.completed, dataset: { role: "toggle" } });
    const notes = h("textarea", {
      class: "lkr-textarea",
      id: notesId,
      rows: "2",
      maxlength: String(window.LKR.labSetup.LIMITS.notes),
      placeholder: "Loja, modelo, preço encontrado…",
      value: item.notes,
      dataset: { role: "notes" },
    });

    const card = h(
      "article",
      { class: "ls-item" + (item.completed ? " is-done" : ""), dataset: { id: item.id } },
      h(
        "div",
        { class: "ls-item__head" },
        h(
          "label",
          { class: "ls-item__toggle", htmlFor: checkboxId },
          h("span", { class: "ls-check" }, checkbox, h("span", { class: "ls-check__box", "aria-hidden": "true" }, icon("check"))),
          h(
            "span",
            { class: "ls-item__text" },
            h("h3", { class: "ls-item__title", text: item.name }),
            item.description ? h("p", { class: "ls-item__desc", text: item.description }) : null,
          ),
        ),
        item.custom
          ? h(
              "button",
              {
                class: "lkr-btn lkr-btn--ghost lkr-btn--icon ls-item__delete",
                type: "button",
                title: "Excluir item personalizado",
                "aria-label": "Excluir " + item.name,
                dataset: { role: "delete" },
              },
              icon("trash"),
            )
          : null,
      ),
      h(
        "div",
        { class: "ls-item__meta" },
        h(
          "div",
          { class: "ls-item__badges" },
          renderBadge(item),
          item.future ? h("span", { class: "ls-tag ls-tag--dashed", text: "FUTURO" }) : null,
          item.quantity ? h("span", { class: "ls-tag", text: item.quantity }) : null,
          item.custom ? h("span", { class: "ls-tag ls-tag--accent", text: "PERSONALIZADO" }) : null,
        ),
        h("span", { class: "ls-item__price" + (price ? "" : " ls-item__price--none"), text: price || "Sem preço" }),
      ),
      item.hint
        ? h(
            "p",
            { class: "ls-hint" + (item.hintTone === "warning" ? " ls-hint--warning" : "") },
            icon(item.hintTone === "warning" ? "alert" : "info"),
            h("span", { text: item.hint }),
          )
        : null,
      h("div", { class: "ls-notes" }, h("label", { class: "ls-notes__label", htmlFor: notesId, text: "Observações" }), notes),
      h(
        "div",
        { class: "ls-item__foot" },
        h("span", { class: "ls-item__status" + (item.completed ? " is-done" : ""), text: statusText(item) }),
        h("span", { class: "ls-item__saved", "aria-live": "polite" }),
      ),
    );
    return card;
  }

  function renderSectionHead(category) {
    return h(
      "header",
      { class: "ls-section__head" },
      h(
        "div",
        null,
        h(
          "h2",
          { class: "ls-section__title", id: "ls-section-" + category.id },
          h("span", null, h("span", { class: "ls-section__code", text: category.code }), " — " + category.title.toUpperCase()),
          category.tag ? h("span", { class: "ls-tag ls-tag--dashed", text: category.tag }) : null,
        ),
        h("p", { class: "ls-section__summary", text: category.summary }),
      ),
      h(
        "div",
        { class: "ls-section__progress", dataset: { progress: category.id } },
        h("div", { class: "ls-section__nums" }, h("span", { dataset: { nums: "" } }), h("span", { class: "lkr-mono", dataset: { pct: "" } })),
        h("div", { class: "ls-bar" }, h("i", { class: "ls-bar__fill" })),
      ),
    );
  }

  function renderSectionHeads(stats) {
    for (const c of stats.byCategory) {
      const block = els.list.querySelector('[data-progress="' + c.id + '"]');
      if (!block) continue;
      const nums = block.querySelector("[data-nums]");
      nums.replaceChildren(h("span", { class: "lkr-mono", text: c.done + " / " + c.total }), " concluídos");
      block.querySelector("[data-pct]").textContent = c.pct + "%";
      const bar = block.querySelector(".ls-bar");
      bar.classList.toggle("ls-bar--done", c.total > 0 && c.done === c.total);
      setProgress(bar, c.pct);
    }
  }

  function renderResults(visible, total) {
    if (!hasActiveFilters()) {
      els.results.replaceChildren(document.createTextNode(total + " itens"));
      return;
    }
    els.results.replaceChildren(
      document.createTextNode("Mostrando " + visible + " de " + total + " itens"),
      h("button", { type: "button", dataset: { role: "clear-filters" } }, "Limpar filtros"),
    );
  }

  function renderList() {
    const all = store.items();
    const visible = all.filter(matchesFilters);
    const sections = [];

    for (const category of catalog.categories) {
      const list = visible.filter((item) => item.category === category.id);
      if (!list.length) continue;
      sections.push(
        h(
          "section",
          { class: "ls-section", "aria-labelledby": "ls-section-" + category.id },
          renderSectionHead(category),
          h("div", { class: "ls-grid" }, list.map(renderCard)),
        ),
      );
    }

    if (!sections.length) {
      sections.push(
        h(
          "div",
          { class: "ls-empty" },
          h("p", { class: "ls-empty__title", text: all.length ? "Nenhum item encontrado" : "Nenhum item cadastrado" }),
          h("p", { text: all.length ? "Ajuste a busca ou os filtros para ver outros itens." : "Adicione o primeiro item do laboratório." }),
          all.length
            ? h("button", { class: "lkr-btn", type: "button", dataset: { role: "clear-filters" } }, "Limpar filtros")
            : null,
        ),
      );
    }

    els.list.replaceChildren(...sections);
    for (const textarea of els.list.querySelectorAll("textarea")) autoGrow(textarea);
    renderResults(visible.length, all.length);
    renderSummary();
  }

  /** Atualiza um card no lugar, sem recriar a lista (preserva foco e texto). */
  function updateCard(id) {
    const card = els.list.querySelector('.ls-item[data-id="' + CSS.escape(id) + '"]');
    const item = store.getItem(id);
    if (!card || !item) return;
    card.classList.toggle("is-done", item.completed);
    const status = card.querySelector(".ls-item__status");
    status.textContent = statusText(item);
    status.classList.toggle("is-done", item.completed);
  }

  // =============================================================== ações

  function onToggle(checkbox) {
    const card = checkbox.closest(".ls-item");
    const ok = store.setCompleted(card.dataset.id, checkbox.checked);
    reportSave(ok);
    updateCard(card.dataset.id);
    renderSummary();
  }

  async function onDelete(card) {
    const item = store.getItem(card.dataset.id);
    if (!item || !item.custom) return;
    const confirmed = await confirmDialog({
      eyebrow: "ITEM PERSONALIZADO",
      title: "Excluir “" + item.name + "”?",
      message: "O item e as observações dele serão removidos deste navegador. Itens padrão do sistema não são afetados.",
      confirmLabel: "Excluir item",
      tone: "danger",
    });
    if (!confirmed) return;
    pendingNotes.delete(item.id);
    reportSave(store.removeCustom(item.id));
    renderList();
    toast("Item removido.");
    els.add.focus();
  }

  // ------------------------------------------------------ adicionar item

  function openAddDialog() {
    flushNotes();
    els.addForm.reset();
    els.nameError.textContent = "";
    els.priceError.textContent = "";
    els.addForm.elements.name.removeAttribute("aria-invalid");
    els.addCategory.value = view.category !== "all" ? view.category : catalog.categories[0].id;
    els.addPriority.value = "media";
    els.addDialog.showModal();
    els.addForm.elements.name.focus();
  }

  function readPrice(input) {
    const raw = input.value.trim().replace(",", ".");
    if (!raw) return { value: null };
    const value = Number(raw);
    return Number.isFinite(value) && value >= 0 ? { value } : { error: true };
  }

  function onAddSubmit(event) {
    event.preventDefault();
    const form = els.addForm.elements;
    const name = form.name.value.trim();
    const min = readPrice(form.priceMin);
    const max = readPrice(form.priceMax);

    let invalid = false;
    els.nameError.textContent = "";
    els.priceError.textContent = "";
    form.name.removeAttribute("aria-invalid");
    if (!name) {
      els.nameError.textContent = "Informe o nome do item.";
      form.name.setAttribute("aria-invalid", "true");
      invalid = true;
    }
    if (min.error || max.error) {
      els.priceError.textContent = "Use apenas números positivos nos preços.";
      invalid = true;
    } else if (min.value !== null && max.value !== null && max.value < min.value) {
      els.priceError.textContent = "O preço máximo deve ser maior ou igual ao mínimo.";
      invalid = true;
    }
    if (invalid) {
      (name ? form.priceMin : form.name).focus();
      return;
    }

    const result = store.addCustom({
      name,
      category: form.category.value,
      description: form.description.value,
      priority: form.priority.value,
      priceMin: min.value,
      priceMax: max.value,
      notes: form.notes.value,
    });
    if (!result.item) {
      els.nameError.textContent = result.error || "Não foi possível adicionar o item.";
      return;
    }
    els.addDialog.close();
    reportSave(result.ok);

    if (!matchesFilters(result.item)) {
      els.search.value = "";
      Object.assign(view, { status: "all", category: "all", query: "" });
      store.setUi({ status: view.status, category: view.category });
      renderFilterState();
    }
    renderList();
    const card = els.list.querySelector('.ls-item[data-id="' + CSS.escape(result.item.id) + '"]');
    if (card) {
      card.classList.add("is-new");
      card.scrollIntoView({ block: "center" });
      card.querySelector("input[type=checkbox]").focus({ preventScroll: true });
    }
    toast("Item adicionado a " + categoryById.get(result.item.category).title + ".", "success");
  }

  // ------------------------------------------------------------- backup

  function exportBackup() {
    flushNotes();
    const data = store.exportData();
    const stamp = data.exportedAt.slice(0, 10);
    downloadJson("lkr-lab-setup-backup-" + stamp + ".json", data);
    toast("Backup exportado.", "success");
  }

  async function importBackup(file) {
    let parsed;
    try {
      parsed = JSON.parse(await file.text());
    } catch {
      toast("Arquivo inválido: não é um JSON legível.", "error");
      return;
    }
    const preview = store.previewImport(parsed);
    if (!preview.ok) {
      toast(preview.error, "error");
      return;
    }
    const s = preview.summary;
    const confirmed = await confirmDialog({
      eyebrow: "IMPORTAR BACKUP",
      title: "Substituir os dados atuais?",
      message: [
        "Arquivo: " + file.name + (s.exportedAt ? " · exportado em " + formatDate(s.exportedAt) : "") + ".",
        "Contém " + plural(s.completed, "item concluído", "itens concluídos") + ", " + plural(s.notes, "observação", "observações") + " e " +
          plural(s.custom, "item personalizado", "itens personalizados") + "." +
          (s.skipped > 0 ? " " + plural(s.skipped, "registro inválido será ignorado", "registros inválidos serão ignorados") + "." : ""),
        "O estado atual deste navegador será substituído. Exporte um backup antes se quiser preservá-lo.",
      ],
      confirmLabel: "Importar e substituir",
      tone: "danger",
    });
    if (!confirmed) return;
    const result = replaceState(parsed, "import");
    if (!result.ok) {
      toast(result.error || "Falha ao salvar os dados importados.", "error");
      return;
    }
    toast("Backup importado.", "success");
  }

  /**
   * Substitui o estado inteiro (importação de arquivo ou restauração do GitHub).
   * O store guarda o estado anterior para permitir desfazer.
   */
  function replaceState(input, reason) {
    flushNotes();
    const result = reason === "undo" ? store.undoImport() : store.importData(input, { reason });
    if (!result.ok) return result;
    refreshAll();
    return result;
  }

  function refreshAll() {
    els.search.value = "";
    Object.assign(view, { status: store.state.ui.status, category: store.state.ui.category, query: "" });
    renderFilterState();
    renderList();
    reportSave(true);
  }

  async function resetAll() {
    const confirmed = await confirmDialog({
      eyebrow: "RESETAR CHECKLIST",
      title: "Resetar o Lab Setup?",
      message: [
        "Isso removerá todos os checklists, observações e itens personalizados armazenados neste navegador.",
        "Deseja continuar?",
      ],
      confirmLabel: "Resetar tudo",
      tone: "danger",
    });
    if (!confirmed) return;
    pendingNotes.forEach((p) => clearTimeout(p.timer));
    pendingNotes.clear();
    store.reset();
    els.search.value = "";
    Object.assign(view, { status: "all", category: "all", query: "" });
    renderFilterState();
    renderList();
    indicator.local = "reset";
    notifyPersist();
    renderIndicator();
    toast("Checklist resetado.");
  }

  // ============================================================== eventos

  function isTyping(target) {
    return target instanceof HTMLElement && (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName));
  }

  function bindEvents() {
    els.list.addEventListener("change", (event) => {
      if (event.target.dataset.role === "toggle") onToggle(event.target);
    });

    els.list.addEventListener("input", (event) => {
      const target = event.target;
      if (target.dataset.role !== "notes") return;
      autoGrow(target);
      const card = target.closest(".ls-item");
      scheduleNote(card.dataset.id, target.value, card);
    });

    els.list.addEventListener("focusout", (event) => {
      const target = event.target;
      if (target.dataset && target.dataset.role === "notes") commitNote(target.closest(".ls-item").dataset.id);
    });

    els.list.addEventListener("click", (event) => {
      const button = event.target.closest("button");
      if (!button) return;
      if (button.dataset.role === "delete") onDelete(button.closest(".ls-item"));
      if (button.dataset.role === "clear-filters") clearFilters();
    });

    els.results.addEventListener("click", (event) => {
      if (event.target.closest("[data-role=clear-filters]")) clearFilters();
    });

    els.status.addEventListener("click", (event) => {
      const button = event.target.closest("button[data-status]");
      if (button) setFilter({ status: button.dataset.status });
    });

    els.chips.addEventListener("click", (event) => {
      const button = event.target.closest("button[data-category]");
      if (button) setFilter({ category: button.dataset.category });
    });

    els.cats.addEventListener("click", (event) => {
      const tile = event.target.closest(".ls-cat");
      if (!tile) return;
      const next = view.category === tile.dataset.category ? "all" : tile.dataset.category;
      setFilter({ category: next }, { scroll: true });
    });

    const runSearch = debounce(() => setFilter({ query: els.search.value }), 90);
    els.search.addEventListener("input", runSearch);
    els.search.addEventListener("keydown", (event) => {
      if (event.key === "Escape" && els.search.value) {
        event.stopPropagation();
        els.search.value = "";
        runSearch.cancel();
        setFilter({ query: "" });
      } else if (event.key === "Enter") {
        runSearch.cancel();
        setFilter({ query: els.search.value });
      }
    });

    els.add.addEventListener("click", openAddDialog);
    els.addCancel.addEventListener("click", () => els.addDialog.close());
    els.addForm.addEventListener("submit", onAddSubmit);
    els.addDialog.addEventListener("click", (event) => {
      if (event.target === els.addDialog) els.addDialog.close();
    });
    els.addDialog.addEventListener("close", () => {
      if (document.activeElement === document.body) els.add.focus();
    });

    els.exportBtn.addEventListener("click", exportBackup);
    els.importBtn.addEventListener("click", () => els.importFile.click());
    els.importFile.addEventListener("change", () => {
      const file = els.importFile.files && els.importFile.files[0];
      els.importFile.value = "";
      if (file) importBackup(file);
    });
    els.resetBtn.addEventListener("click", resetAll);

    // Atalhos: "/" ou Ctrl/⌘+K busca, "N" novo item, Esc limpa a busca.
    document.addEventListener("keydown", (event) => {
      if (document.querySelector("dialog[open]")) return;
      const typing = isTyping(event.target);
      const key = event.key || "";
      const commandK = (event.ctrlKey || event.metaKey) && key.toLowerCase() === "k";
      if (commandK || (!typing && key === "/")) {
        event.preventDefault();
        els.search.focus();
        els.search.select();
        return;
      }
      if (typing || event.ctrlKey || event.metaKey || event.altKey) return;
      if (key === "n" || key === "N") {
        event.preventDefault();
        openAddDialog();
      } else if (key === "Escape" && hasActiveFilters()) {
        clearFilters();
      }
    });

    // Não perde a última digitação ao fechar a aba ou trocar de janela.
    window.addEventListener("pagehide", flushNotes);
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "hidden") flushNotes();
    });

    // Outra aba alterou os dados: recarrega, salvo se o usuário estiver escrevendo aqui.
    window.addEventListener("storage", (event) => {
      if (event.key !== null && event.key !== window.LKR.labSetup.STORAGE_KEY) return;
      if (pendingNotes.size || (document.activeElement && document.activeElement.dataset.role === "notes")) return;
      store.load();
      renderList();
      notifyPersist();
      renderIndicator();
    });

    // Borda da toolbar só quando ela está fixada no topo.
    if ("IntersectionObserver" in window) {
      const sentinel = h("div", { "aria-hidden": "true" });
      els.toolbar.before(sentinel);
      new IntersectionObserver(
        ([entry]) => els.toolbar.classList.toggle("is-stuck", !entry.isIntersecting),
        { rootMargin: "-" + (parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--topbar-height")) + 1) + "px 0px 0px 0px" },
      ).observe(sentinel);
    }

    let resizeFrame = 0;
    window.addEventListener("resize", () => {
      cancelAnimationFrame(resizeFrame);
      resizeFrame = requestAnimationFrame(() => els.list.querySelectorAll("textarea").forEach(autoGrow));
    });
  }

  // ================================================================= init

  function init() {
    hydrateIcons();
    buildFilters();
    renderFilterState();
    renderList();
    bindEvents();

    renderIndicator();
    if (!store.available) {
      els.storageWarning.hidden = false;
      els.storageWarning.replaceChildren(
        icon("alert"),
        "O armazenamento local está bloqueado neste navegador. As alterações valem só até fechar a página — exporte um backup para não perdê-las.",
      );
    }
    if (store.loadIssue === "corrupt") {
      toast("Os dados salvos estavam ilegíveis. Uma cópia foi preservada e o checklist recomeçou.", "error");
    }
  }

  init();

  // Superfície mínima para camadas extras do módulo (sync.js).
  window.LKR.labSetup.app = {
    store,
    flushNotes,
    replaceState,
    onPersist: (listener) => persistListeners.push(listener),
    setSyncIndicator(state) {
      indicator.sync = state;
      renderIndicator();
    },
  };
})();
