/*
 * LKR LAB · Lab Setup — estado e persistência
 *
 * Um único objeto de estado, salvo inteiro em uma chave do localStorage:
 *
 *   {
 *     version: 1,
 *     createdAt, updatedAt,
 *     items:  { "<id>": { completed, completedAt, notes, updatedAt } },
 *     custom: [ { id: "custom-…", category, name, description, priority, priceMin, priceMax, createdAt } ],
 *     ui:     { status, category }
 *   }
 *
 * O estado guarda apenas o que o usuário produziu. As definições dos itens do
 * sistema vêm do catálogo e são mescladas por id a cada carregamento, então
 * novos itens do catálogo aparecem sem apagar marcações nem observações.
 * Entradas cujo item saiu do catálogo são mantidas (não exibidas) para que
 * voltem caso o item retorne.
 *
 * Sem dependência de DOM: `storage` e `now` são injetados, o que permite testes.
 */
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  root.LKR = root.LKR || {};
  root.LKR.labSetup = root.LKR.labSetup || {};
  Object.assign(root.LKR.labSetup, api);
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  "use strict";

  const STORAGE_KEY = "lkr-lab-setup-v1";
  const SNAPSHOT_KEY = "lkr-lab-setup-before-restore";
  const SCHEMA_VERSION = 1;
  const EXPORT_FORMAT = "lkr-lab.lab-setup";
  const REPO_SOURCE = "lkr-lab";
  const REPO_MODULE = "lab-setup";
  const STATUS_FILTERS = ["all", "pending", "done"];
  const LIMITS = { name: 120, description: 280, notes: 5000 };
  const ID_PATTERN = /^[a-z0-9][a-z0-9-]{0,79}$/;
  const CUSTOM_ID_PATTERN = /^custom-[a-z0-9-]{1,64}$/;

  // ------------------------------------------------------------- helpers

  const isPlainObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
  const text = (value, max) => (typeof value === "string" ? value.slice(0, max) : "");
  const isoOrNull = (value) => (typeof value === "string" && !Number.isNaN(Date.parse(value)) ? value : null);

  function price(value) {
    if (value === null || value === undefined || value === "") return null;
    const number = Number(value);
    return Number.isFinite(number) && number >= 0 && number < 1e7 ? Math.round(number * 100) / 100 : null;
  }

  function emptyState(now) {
    return {
      version: SCHEMA_VERSION,
      createdAt: now,
      updatedAt: null,
      items: {},
      custom: [],
      ui: { status: "all", category: "all" },
    };
  }

  function emptyEntry() {
    return { completed: false, completedAt: null, notes: "", updatedAt: null };
  }

  // ------------------------------------------------------ normalização

  function normalizeEntry(raw) {
    if (!isPlainObject(raw)) return null;
    const completed = raw.completed === true;
    return {
      completed,
      completedAt: completed ? isoOrNull(raw.completedAt) : null,
      notes: text(raw.notes, LIMITS.notes),
      updatedAt: isoOrNull(raw.updatedAt),
    };
  }

  function normalizeCustom(raw, catalog, now) {
    if (!isPlainObject(raw)) return null;
    const id = typeof raw.id === "string" && CUSTOM_ID_PATTERN.test(raw.id) ? raw.id : null;
    const name = text(raw.name, LIMITS.name).trim();
    if (!id || !name) return null;
    let priceMin = price(raw.priceMin);
    let priceMax = price(raw.priceMax);
    if (priceMin === null && priceMax !== null) priceMin = priceMax;
    if (priceMin !== null && priceMax !== null && priceMax < priceMin) [priceMin, priceMax] = [priceMax, priceMin];
    return {
      id,
      // Categoria desconhecida é preservada; o item só não é exibido.
      category: typeof raw.category === "string" ? raw.category.slice(0, 40) : "",
      name,
      description: text(raw.description, LIMITS.description).trim(),
      priority: Object.prototype.hasOwnProperty.call(catalog.priorities, raw.priority) ? raw.priority : "media",
      priceMin,
      priceMax,
      createdAt: isoOrNull(raw.createdAt) || now,
    };
  }

  /** Converte qualquer valor em um estado válido, descartando o que não reconhece. */
  function normalizeState(raw, catalog, now) {
    const state = emptyState(now);
    if (!isPlainObject(raw)) return state;
    state.createdAt = isoOrNull(raw.createdAt) || now;
    state.updatedAt = isoOrNull(raw.updatedAt);

    if (isPlainObject(raw.items)) {
      for (const [id, value] of Object.entries(raw.items)) {
        if (!ID_PATTERN.test(id)) continue;
        const entry = normalizeEntry(value);
        if (entry) state.items[id] = entry;
      }
    }

    if (Array.isArray(raw.custom)) {
      const seen = new Set();
      for (const value of raw.custom) {
        const custom = normalizeCustom(value, catalog, now);
        if (!custom || seen.has(custom.id)) continue;
        seen.add(custom.id);
        state.custom.push(custom);
      }
    }

    if (isPlainObject(raw.ui)) {
      if (STATUS_FILTERS.includes(raw.ui.status)) state.ui.status = raw.ui.status;
      if (raw.ui.category === "all" || catalog.categories.some((c) => c.id === raw.ui.category)) state.ui.category = raw.ui.category;
    }
    return state;
  }

  /** Ponto de migração entre versões do schema. Hoje só existe a v1. */
  function migrate(raw) {
    return raw;
  }

  // ---------------------------------------------------------- importação

  /**
   * Valida um backup antes de qualquer sobrescrita. Aceita:
   *  - o envelope exportado pela página ({ format, state });
   *  - o arquivo versionado no Git ({ source: "lkr-lab", module: "lab-setup", schemaVersion, state });
   *  - o estado puro.
   */
  function validateImport(input, catalog, now) {
    const fail = (error) => ({ ok: false, error });
    if (!isPlainObject(input)) return fail("O arquivo não contém um objeto JSON válido.");

    let raw = input;
    if ("format" in input) {
      if (input.format !== EXPORT_FORMAT) return fail("Este arquivo não é um backup do LKR LAB · Lab Setup.");
      raw = input.state;
    } else if ("source" in input || "module" in input) {
      if (input.source !== REPO_SOURCE || input.module !== REPO_MODULE) return fail("Este arquivo não é um backup do LKR LAB · Lab Setup.");
      if (!Number.isInteger(input.schemaVersion) || input.schemaVersion < 1) return fail("schemaVersion do backup ausente ou inválido.");
      if (input.schemaVersion > SCHEMA_VERSION) return fail("O backup foi criado por uma versão mais nova do Lab Setup (schema v" + input.schemaVersion + ").");
      raw = input.state;
    }
    if (!isPlainObject(raw)) return fail("O backup não contém o estado do checklist.");
    if (!("version" in raw) && !("items" in raw)) return fail("Este arquivo não é um backup do LKR LAB · Lab Setup.");
    if (!Number.isInteger(raw.version) || raw.version < 1) return fail("Versão do backup ausente ou inválida.");
    if (raw.version > SCHEMA_VERSION) return fail("O backup foi criado por uma versão mais nova do Lab Setup (schema v" + raw.version + ").");
    if (!isPlainObject(raw.items)) return fail("O backup não contém a lista de itens.");
    if ("custom" in raw && !Array.isArray(raw.custom)) return fail("Os itens personalizados do backup estão em formato inválido.");

    const state = normalizeState(migrate(raw), catalog, now);
    const entries = Object.values(state.items);
    return {
      ok: true,
      state,
      summary: {
        exportedAt: isoOrNull(input.exportedAt) || isoOrNull(input.updatedAt),
        completed: entries.filter((e) => e.completed).length,
        notes: entries.filter((e) => e.notes.trim()).length,
        custom: state.custom.length,
        skipped: Object.keys(raw.items).length - entries.length + ((raw.custom || []).length - state.custom.length),
      },
    };
  }

  // ------------------------------------------------ persistência / hash

  /**
   * Estado que vai para o arquivo versionado: sem `ui` (filtros são transitórios)
   * e com os itens em ordem estável, para diffs pequenos no Git.
   */
  function persistableState(state) {
    const items = {};
    for (const id of Object.keys(state.items || {}).sort()) items[id] = state.items[id];
    return { version: state.version, createdAt: state.createdAt, updatedAt: state.updatedAt, items, custom: state.custom || [] };
  }

  /** JSON com chaves ordenadas: mesma entrada, mesma string. */
  function stableStringify(value) {
    if (value === null || typeof value !== "object") return JSON.stringify(value === undefined ? null : value);
    if (Array.isArray(value)) return "[" + value.map(stableStringify).join(",") + "]";
    return (
      "{" +
      Object.keys(value)
        .filter((key) => value[key] !== undefined)
        .sort()
        .map((key) => JSON.stringify(key) + ":" + stableStringify(value[key]))
        .join(",") +
      "}"
    );
  }

  // cyrb53 em 64 bits: detecção de mudança, não criptografia.
  function hashString(str) {
    let h1 = 0xdeadbeef;
    let h2 = 0x41c6ce57;
    for (let i = 0; i < str.length; i++) {
      const ch = str.charCodeAt(i);
      h1 = Math.imul(h1 ^ ch, 2654435761);
      h2 = Math.imul(h2 ^ ch, 1597334677);
    }
    h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507);
    h1 ^= Math.imul(h2 ^ (h2 >>> 13), 3266489909);
    h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507);
    h2 ^= Math.imul(h1 ^ (h1 >>> 13), 3266489909);
    return (h2 >>> 0).toString(16).padStart(8, "0") + (h1 >>> 0).toString(16).padStart(8, "0");
  }

  /**
   * Hash do conteúdo que importa: marcações, datas de conclusão, observações e
   * itens personalizados. Ignora carimbos de "updatedAt" e entradas de volta ao
   * padrão, então marcar e desmarcar o mesmo item não conta como alteração.
   */
  function stateHash(state) {
    const items = {};
    for (const [id, e] of Object.entries(state.items || {})) {
      if (!e.completed && !e.notes) continue;
      items[id] = { completed: e.completed, completedAt: e.completedAt, notes: e.notes };
    }
    return "h" + hashString(stableStringify({ version: state.version, items, custom: state.custom || [] }));
  }

  // --------------------------------------------------------- estatísticas

  function costRange(list) {
    let min = 0;
    let max = 0;
    let open = false;
    let priced = 0;
    for (const item of list) {
      if (item.priceMin === null || item.priceMin === undefined) continue;
      priced += 1;
      min += item.priceMin;
      if (item.priceMax === null || item.priceMax === undefined) {
        max += item.priceMin;
        open = true;
      } else {
        max += item.priceMax;
      }
    }
    return { min, max, open, priced };
  }

  const percent = (done, total) => (total ? Math.round((done / total) * 100) : 0);

  function computeStats(items, catalog) {
    const done = items.filter((item) => item.completed);
    const pending = items.filter((item) => !item.completed);
    const byCategory = catalog.categories.map((category) => {
      const list = items.filter((item) => item.category === category.id);
      const doneCount = list.filter((item) => item.completed).length;
      return { id: category.id, total: list.length, done: doneCount, pct: percent(doneCount, list.length) };
    });
    const total = costRange(items);
    return {
      total: items.length,
      done: done.length,
      pending: pending.length,
      pct: percent(done.length, items.length),
      categories: catalog.categories.length,
      categoriesComplete: byCategory.filter((c) => c.total > 0 && c.done === c.total).length,
      byCategory,
      cost: {
        total,
        done: costRange(done),
        pending: costRange(pending),
        unpriced: items.length - total.priced,
      },
    };
  }

  // ---------------------------------------------------------------- store

  function createStore({ catalog, storage, now = () => new Date().toISOString(), key = STORAGE_KEY }) {
    const categoryIds = new Set(catalog.categories.map((c) => c.id));
    let state = emptyState(now());
    let loadIssue = null;
    let lastWrite = { ok: true, at: null };

    function load() {
      loadIssue = null;
      const raw = storage.get(key);
      if (raw === null || raw === undefined) {
        state = emptyState(now());
        return state;
      }
      try {
        state = normalizeState(migrate(JSON.parse(raw)), catalog, now());
      } catch {
        // Nunca descarta silenciosamente: guarda o conteúdo ilegível antes de recomeçar.
        storage.set(key + ":corrupt:" + Date.now(), raw);
        loadIssue = "corrupt";
        state = emptyState(now());
      }
      return state;
    }

    function persist(touch = true) {
      if (touch) state.updatedAt = now();
      const ok = storage.set(key, JSON.stringify(state));
      lastWrite = { ok, at: touch ? state.updatedAt : lastWrite.at };
      return ok;
    }

    function definitions() {
      const customs = state.custom.filter((c) => categoryIds.has(c.category)).map((c) => Object.assign({}, c, { custom: true }));
      return catalog.items.map((d) => Object.assign({}, d, { custom: false })).concat(customs);
    }

    const findDefinition = (id) => definitions().find((d) => d.id === id) || null;

    function entry(id) {
      if (!state.items[id]) state.items[id] = emptyEntry();
      return state.items[id];
    }

    /** Definições mescladas com o estado salvo: o que a interface exibe. */
    function items() {
      return definitions().map((d) => Object.assign(d, state.items[d.id] || emptyEntry()));
    }

    function getItem(id) {
      const d = findDefinition(id);
      return d ? Object.assign(d, state.items[id] || emptyEntry()) : null;
    }

    function setCompleted(id, completed) {
      if (!findDefinition(id)) return false;
      const e = entry(id);
      const at = now();
      e.completed = Boolean(completed);
      e.completedAt = e.completed ? at : null;
      e.updatedAt = at;
      return persist();
    }

    function setNotes(id, notes) {
      if (!findDefinition(id)) return false;
      const value = text(notes, LIMITS.notes);
      const e = entry(id);
      if (e.notes === value) return true;
      e.notes = value;
      e.updatedAt = now();
      return persist();
    }

    function addCustom(input) {
      const at = now();
      const id = "custom-" + Date.now().toString(36) + "-" + Math.random().toString(36).slice(2, 7);
      const custom = normalizeCustom(Object.assign({}, input, { id, createdAt: at }), catalog, at);
      if (!custom) return { ok: false, error: "Informe um nome para o item." };
      if (!categoryIds.has(custom.category)) return { ok: false, error: "Selecione uma categoria válida." };
      state.custom.push(custom);
      const notes = text(input.notes, LIMITS.notes);
      if (notes.trim()) Object.assign(entry(id), { notes, updatedAt: at });
      return { ok: persist(), item: getItem(id) };
    }

    function removeCustom(id) {
      const index = state.custom.findIndex((c) => c.id === id);
      if (index === -1) return false;
      state.custom.splice(index, 1);
      delete state.items[id];
      return persist();
    }

    function setUi(patch) {
      if (patch.status !== undefined && STATUS_FILTERS.includes(patch.status)) state.ui.status = patch.status;
      if (patch.category !== undefined && (patch.category === "all" || categoryIds.has(patch.category))) state.ui.category = patch.category;
      return persist(false);
    }

    function reset() {
      storage.remove(key);
      storage.remove(SNAPSHOT_KEY);
      state = emptyState(now());
      lastWrite = { ok: true, at: null };
      return true;
    }

    function exportData() {
      return {
        format: EXPORT_FORMAT,
        app: "LKR LAB",
        module: "Lab Setup",
        schemaVersion: SCHEMA_VERSION,
        exportedAt: now(),
        state: JSON.parse(JSON.stringify(state)),
      };
    }

    function previewImport(input) {
      return validateImport(input, catalog, now());
    }

    /**
     * Substitui o estado pelo backup. Antes guarda o estado atual em
     * SNAPSHOT_KEY, para que uma restauração acidental possa ser desfeita.
     */
    function importData(input, options) {
      const result = previewImport(input);
      if (!result.ok) return result;
      const snapshot = { takenAt: now(), reason: (options && options.reason) || "import", state: JSON.parse(JSON.stringify(state)) };
      if (!storage.set(SNAPSHOT_KEY, JSON.stringify(snapshot))) return { ok: false, error: "Não foi possível guardar o estado anterior; nada foi alterado." };
      state = result.state;
      return Object.assign(result, { ok: persist() });
    }

    function getSnapshot() {
      try {
        const snapshot = JSON.parse(storage.get(SNAPSHOT_KEY));
        return isPlainObject(snapshot) && isPlainObject(snapshot.state) ? snapshot : null;
      } catch {
        return null;
      }
    }

    /** Volta ao estado guardado antes da última restauração/importação. */
    function undoImport() {
      const snapshot = getSnapshot();
      if (!snapshot) return { ok: false, error: "Não há estado anterior guardado." };
      const result = previewImport(snapshot.state);
      if (!result.ok) return result;
      state = result.state;
      const ok = persist();
      if (ok) storage.remove(SNAPSHOT_KEY);
      return Object.assign(result, { ok });
    }

    load();

    return {
      get state() {
        return state;
      },
      get loadIssue() {
        return loadIssue;
      },
      get lastWrite() {
        return lastWrite;
      },
      available: storage.available !== false,
      load,
      items,
      getItem,
      stats: () => computeStats(items(), catalog),
      setCompleted,
      setNotes,
      addCustom,
      removeCustom,
      setUi,
      reset,
      exportData,
      previewImport,
      importData,
      getSnapshot,
      undoImport,
      persistable: () => persistableState(state),
      contentHash: () => stateHash(state),
    };
  }

  return {
    STORAGE_KEY,
    SNAPSHOT_KEY,
    SCHEMA_VERSION,
    EXPORT_FORMAT,
    REPO_SOURCE,
    REPO_MODULE,
    LIMITS,
    createStore,
    validateImport,
    computeStats,
    normalizeState,
    persistableState,
    stableStringify,
    stateHash,
  };
});
