import { describe, expect, it } from "vitest";
import "../core/lkr-portable.js";
import "./catalog.js";
import "./store.js";

const { catalog, createStore, STORAGE_KEY, EXPORT_FORMAT } = globalThis.LKR.labSetup;

function memoryStorage(initial) {
  const data = new Map(Object.entries(initial || {}));
  return {
    data,
    get: (key) => (data.has(key) ? data.get(key) : null),
    set: (key, value) => {
      data.set(key, value);
      return true;
    },
    remove: (key) => data.delete(key),
  };
}

const saved = (storage) => JSON.parse(storage.get(STORAGE_KEY));

describe("catálogo", () => {
  it("usa ids únicos, válidos e categorias/prioridades conhecidas", () => {
    const ids = catalog.items.map((i) => i.id);
    expect(new Set(ids).size).toBe(ids.length);
    const categories = new Set(catalog.categories.map((c) => c.id));
    for (const item of catalog.items) {
      expect(item.id).toMatch(/^[a-z0-9][a-z0-9-]*$/);
      expect(item.id.startsWith("custom-")).toBe(false);
      expect(categories.has(item.category)).toBe(true);
      expect(catalog.priorities[item.priority]).toBeDefined();
    }
  });
});

describe("store do Lab Setup", () => {
  it("carrega o catálogo padrão quando não há dados salvos", () => {
    const store = createStore({ catalog, storage: memoryStorage() });
    const stats = store.stats();
    expect(stats.total).toBe(catalog.items.length);
    expect(stats.done).toBe(0);
    expect(store.state.updatedAt).toBeNull();
  });

  it("persiste marcação e observação em uma única chave", () => {
    const storage = memoryStorage();
    const store = createStore({ catalog, storage });
    store.setCompleted("furadeira-12v", true);
    store.setNotes("furadeira-12v", "Achei no Mercado Livre por R$ 139.");
    expect([...storage.data.keys()]).toEqual([STORAGE_KEY]);

    const reopened = createStore({ catalog, storage });
    const item = reopened.getItem("furadeira-12v");
    expect(item.completed).toBe(true);
    expect(item.completedAt).toBeTruthy();
    expect(item.notes).toBe("Achei no Mercado Livre por R$ 139.");

    reopened.setCompleted("furadeira-12v", false);
    expect(createStore({ catalog, storage }).getItem("furadeira-12v").completedAt).toBeNull();
  });

  it("mescla o estado salvo com itens novos do catálogo e preserva entradas órfãs", () => {
    const storage = memoryStorage({
      [STORAGE_KEY]: JSON.stringify({
        version: 1,
        items: { trena: { completed: true, notes: "ok" }, "item-removido": { completed: true, notes: "guardar" } },
      }),
    });
    const extended = Object.assign({}, catalog, {
      items: catalog.items.concat({ id: "novo-item", category: "bancada", name: "Novo", description: "", priority: "alta", priceMin: 10, priceMax: 20 }),
    });
    const store = createStore({ catalog: extended, storage });
    expect(store.getItem("trena").completed).toBe(true);
    expect(store.getItem("novo-item").completed).toBe(false);
    expect(store.stats().total).toBe(catalog.items.length + 1);
    store.setCompleted("novo-item", true);
    expect(saved(storage).items["item-removido"].notes).toBe("guardar");
  });

  it("adiciona e remove itens personalizados", () => {
    const storage = memoryStorage();
    const store = createStore({ catalog, storage });
    const result = store.addCustom({ name: "  Morsa  ", category: "bancada", priority: "alta", priceMin: 80, priceMax: 150, notes: "Ver usada" });
    expect(result.ok).toBe(true);
    expect(result.item.id).toMatch(/^custom-/);
    expect(result.item.name).toBe("Morsa");
    expect(result.item.notes).toBe("Ver usada");
    expect(store.stats().total).toBe(catalog.items.length + 1);

    expect(store.addCustom({ name: "", category: "bancada" }).ok).toBe(false);
    expect(store.addCustom({ name: "X", category: "inexistente" }).ok).toBe(false);

    expect(store.removeCustom(result.item.id)).toBe(true);
    expect(saved(storage).custom).toEqual([]);
    expect(saved(storage).items[result.item.id]).toBeUndefined();
    expect(store.removeCustom("trena")).toBe(false);
  });

  it("calcula custos mínimo/máximo por status, com faixa aberta", () => {
    const store = createStore({ catalog, storage: memoryStorage() });
    const before = store.stats().cost.total;
    expect(before.open).toBe(true); // pegboard: R$ 50+
    store.setCompleted("furadeira-12v", true);
    const cost = store.stats().cost;
    expect(cost.done).toMatchObject({ min: 130, max: 200, priced: 1 });
    expect(cost.pending.min + cost.done.min).toBe(cost.total.min);
    expect(cost.pending.max + cost.done.max).toBe(cost.total.max);
  });

  it("exporta e importa com validação", () => {
    const source = createStore({ catalog, storage: memoryStorage() });
    source.setCompleted("multimetro", true);
    source.setNotes("multimetro", "Só baixa tensão");
    source.addCustom({ name: "Lupa", category: "eletronica" });
    const backup = JSON.parse(JSON.stringify(source.exportData()));
    expect(backup.format).toBe(EXPORT_FORMAT);

    const target = createStore({ catalog, storage: memoryStorage() });
    const preview = target.previewImport(backup);
    expect(preview.ok).toBe(true);
    expect(preview.summary).toMatchObject({ completed: 1, notes: 1, custom: 1 });
    expect(target.stats().done).toBe(0); // preview não altera nada

    expect(target.importData(backup).ok).toBe(true);
    expect(target.getItem("multimetro").notes).toBe("Só baixa tensão");
    expect(target.stats().total).toBe(catalog.items.length + 1);
  });

  it("rejeita backups inválidos sem alterar o estado", () => {
    const storage = memoryStorage();
    const store = createStore({ catalog, storage });
    store.setCompleted("trena", true);
    const before = storage.get(STORAGE_KEY);

    for (const bad of [null, [], "x", { format: "outro-app", state: {} }, { version: 2, items: {} }, { version: 1 }, { version: 1, items: {}, custom: {} }]) {
      expect(store.importData(bad).ok).toBe(false);
    }
    expect(storage.get(STORAGE_KEY)).toBe(before);
  });

  it("ignora chaves perigosas e campos malformados", () => {
    const raw = '{"version":1,"items":{"__proto__":{"completed":true},"trena":{"completed":"sim","notes":42}},"custom":[{"id":"custom-a","name":"<img src=x>","category":"bancada","priceMin":-5,"priceMax":"abc"},{"id":"../x","name":"y"}]}';
    const store = createStore({ catalog, storage: memoryStorage({ [STORAGE_KEY]: raw }) });
    expect(Object.getPrototypeOf(store.state.items)).toBe(Object.prototype);
    expect(store.getItem("trena")).toMatchObject({ completed: false, notes: "" });
    expect(store.state.custom).toHaveLength(1);
    expect(store.state.custom[0]).toMatchObject({ name: "<img src=x>", priceMin: null, priceMax: null });
  });

  it("preserva dados ilegíveis antes de recomeçar", () => {
    const storage = memoryStorage({ [STORAGE_KEY]: "{quebrado" });
    const store = createStore({ catalog, storage });
    expect(store.loadIssue).toBe("corrupt");
    const backupKey = [...storage.data.keys()].find((k) => k.startsWith(STORAGE_KEY + ":corrupt:"));
    expect(storage.get(backupKey)).toBe("{quebrado");
  });

  it("hash de conteúdo ignora filtros, carimbos e itens de volta ao padrão", () => {
    const store = createStore({ catalog, storage: memoryStorage() });
    const empty = store.contentHash();
    store.setUi({ status: "done", category: "bancada" });
    expect(store.contentHash()).toBe(empty);
    store.setCompleted("trena", true);
    const done = store.contentHash();
    expect(done).not.toBe(empty);
    store.setCompleted("trena", false);
    expect(store.contentHash()).toBe(empty);
    store.setNotes("multimetro", "Comprar depois do pagamento.");
    expect(store.contentHash()).not.toBe(empty);
    expect(store.persistable()).not.toHaveProperty("ui");
    expect(Object.keys(store.persistable().items)).toEqual([...Object.keys(store.persistable().items)].sort());
  });

  it("aceita o arquivo versionado do repositório e desfaz a restauração", () => {
    const storage = memoryStorage();
    const store = createStore({ catalog, storage });
    store.setNotes("trena", "estado local");
    const envelope = { schemaVersion: 1, source: "lkr-lab", module: "lab-setup", updatedAt: "2026-09-30T15:30:00-03:00", state: { version: 1, items: { esquadro: { completed: true, notes: "do GitHub" } }, custom: [] } };

    expect(store.previewImport({ ...envelope, schemaVersion: 2 }).ok).toBe(false);
    expect(store.previewImport({ ...envelope, module: "inventory" }).ok).toBe(false);
    const preview = store.previewImport(envelope);
    expect(preview.summary).toMatchObject({ completed: 1, notes: 1, exportedAt: "2026-09-30T15:30:00-03:00" });

    expect(store.importData(envelope, { reason: "restore" }).ok).toBe(true);
    expect(store.getItem("esquadro").notes).toBe("do GitHub");
    expect(store.getItem("trena").notes).toBe("");
    expect(store.getSnapshot()).toMatchObject({ reason: "restore" });

    expect(store.undoImport().ok).toBe(true);
    expect(store.getItem("trena").notes).toBe("estado local");
    expect(store.getItem("esquadro").completed).toBe(false);
    expect(store.getSnapshot()).toBeNull();
  });

  it("reset limpa tudo e filtros de UI não alteram a data de atualização", () => {
    const storage = memoryStorage();
    const store = createStore({ catalog, storage });
    store.setUi({ status: "done", category: "eletronica" });
    expect(store.state.updatedAt).toBeNull();
    expect(saved(storage).ui).toEqual({ status: "done", category: "eletronica" });
    store.addCustom({ name: "Temp", category: "bancada" });
    store.reset();
    expect(storage.get(STORAGE_KEY)).toBeNull();
    expect(store.stats().total).toBe(catalog.items.length);
  });
});
