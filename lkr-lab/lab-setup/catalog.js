/*
 * LKR LAB · Lab Setup — catálogo padrão
 *
 * Fonte dos itens do sistema. Para evoluir o checklist, adicione itens aqui:
 * o estado salvo do usuário é mesclado por `id`, então marcações e observações
 * existentes são preservadas.
 *
 * Regras:
 *  - `id` é permanente: nunca renomeie o id de um item já publicado.
 *  - ids usam [a-z0-9-] e não podem começar com "custom-" (reservado ao usuário).
 *  - preços em reais; `priceMax: null` com `priceMin` definido significa "a partir de".
 */
(function (root, factory) {
  const catalog = factory();
  if (typeof module === "object" && module.exports) module.exports = catalog;
  root.LKR = root.LKR || {};
  root.LKR.labSetup = root.LKR.labSetup || {};
  root.LKR.labSetup.catalog = catalog;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  "use strict";

  const priorities = {
    altissima: { label: "ALTÍSSIMA", rank: 0 },
    alta: { label: "ALTA", rank: 1 },
    media: { label: "MÉDIA", rank: 2 },
    baixa: { label: "BAIXA", rank: 3 },
    futuro: { label: "FUTURO", rank: 4 },
  };

  const categories = [
    { id: "bancada", code: "01", label: "Bancada", title: "Bancada", summary: "Estrutura, acabamento, medição e segurança da bancada." },
    { id: "organizacao", code: "02", label: "Organização", title: "Organização", summary: "Onde cada ferramenta, peça e componente vai morar." },
    { id: "ferramentas", code: "03", label: "Ferramentas", title: "Ferramentas", summary: "Base para marcenaria, montagem e manutenção." },
    { id: "eletronica", code: "04", label: "Eletrônica", title: "Eletrônica", summary: "Medição, solda e prototipagem com Arduino e ESP32." },
    { id: "computadores", code: "05", label: "Computadores", title: "Computadores / Servidor", summary: "Planejamento de manutenção, rede e estrutura do servidor.", tag: "PLANEJAMENTO" },
    { id: "fabricacao", code: "06", label: "Fabricação", title: "Fabricação", summary: "Próxima fase: impressão 3D e ferramentas de fabricação.", tag: "FUTURO" },
  ];

  // Atalho para manter a tabela legível.
  const item = (category, id, name, description, priority, priceMin, priceMax, extra) =>
    Object.assign({ id, category, name, description, priority, priceMin, priceMax }, extra || {});

  const items = [
    // 01 — Bancada
    item("bancada", "lixas-80-120-220", "Lixas 80 / 120 / 220", "Preparar e recuperar a madeira.", "alta", 15, 30),
    item("bancada", "verniz-selador", "Verniz ou selador para madeira", "Proteger a bancada e melhorar a durabilidade.", "alta", 40, 60),
    item("bancada", "pincel-trincha", "Pincel / trincha", "Aplicação do acabamento.", "alta", 10, 20),
    item("bancada", "parafusos-madeira", "Parafusos para madeira", "Montagem da estrutura.", "alta", 20, 40),
    item("bancada", "cola-madeira", "Cola para madeira", "Reforço da estrutura.", "alta", 15, 25),
    item("bancada", "oculos-protecao", "Óculos de proteção", "Proteção durante corte, lixamento e manutenção.", "alta", 10, 20),
    item("bancada", "mascaras-pff2", "Máscaras PFF2", "Proteção contra pó da madeira.", "alta", 5, 15),
    item("bancada", "sargentos-grampos", "Sargentos / grampos", "Fixar peças durante montagem, colagem e corte.", "media", 30, 80, { quantity: "2–4 unidades" }),
    item("bancada", "trena", "Trena", "Medição.", "alta", 10, 20),
    item("bancada", "esquadro", "Esquadro", "Garantir montagens e cortes em ângulo correto.", "alta", 15, 30),
    item("bancada", "estilete", "Estilete", "Uso geral.", "media", 5, 15),

    // 02 — Organização
    item("organizacao", "caixa-ferramentas", "Caixa de ferramentas", "Guardar e transportar as ferramentas principais.", "alta", 30, 60),
    item("organizacao", "gaveteiro-componentes", "Gaveteiro para componentes", "Parafusos, componentes eletrônicos e peças pequenas.", "media", 30, 50),
    item("organizacao", "pegboard", "Painel de ferramentas / pegboard", "Utilizar o espaço vertical da bancada.", "baixa", 50, null, {
      priorityNote: "inicialmente",
      hint: "Antes de comprar, avaliar construir utilizando a madeira disponível.",
    }),
    item("organizacao", "caixas-organizadoras", "Caixas organizadoras", "Cabos, projetos, peças e componentes.", "media", null, null, {
      hint: "Reaproveitar caixas e recipientes sempre que possível.",
    }),

    // 03 — Ferramentas
    item("ferramentas", "furadeira-12v", "Furadeira / parafusadeira 12 V", "Marcenaria, montagem, manutenção, servidor e projetos.", "altissima", 130, 200),
    item("ferramentas", "jogo-bits", "Jogo de bits", "Pontas para a parafusadeira: Philips, fenda, Torx e Allen.", "alta", null, null),
    item("ferramentas", "alicate-universal", "Alicate universal", "Uso geral: segurar, dobrar e cortar.", "alta", null, null),
    item("ferramentas", "alicate-corte", "Alicate de corte", "Corte de fios, terminais e abraçadeiras.", "alta", null, null),
    item("ferramentas", "alicate-bico", "Alicate de bico", "Trabalho fino em eletrônica e espaços apertados.", "media", null, null),
    item("ferramentas", "martelo", "Martelo", "Montagem e ajustes na marcenaria.", "media", null, null),
    item("ferramentas", "chaves-philips-fenda", "Jogo de chaves Philips e fenda", "Montagem e manutenção geral.", "alta", null, null),
    item("ferramentas", "chaves-allen", "Chaves Allen", "Móveis, perfis e equipamentos.", "media", null, null),
    item("ferramentas", "kit-chave-precisao", "Kit de chave de precisão", "Computadores, notebooks, eletrônicos e servidor.", "alta", 30, 50),
    item("ferramentas", "lixadeira-orbital", "Lixadeira orbital", "Agilizar o tratamento da madeira.", "baixa", 130, 200, {
      priorityNote: "inicialmente",
      hint: "Utilizar lixamento manual primeiro se for necessário economizar.",
    }),

    // 04 — Eletrônica
    item("eletronica", "multimetro", "Multímetro", "Medição, Arduino, eletrônica, continuidade e diagnóstico.", "altissima", null, null, {
      hint: "Multímetro muito barato deve ser utilizado inicialmente apenas em baixa tensão. Para trabalhos em 127/220 V, comprar equipamento adequado e certificado futuramente.",
      hintTone: "warning",
    }),
    item("eletronica", "ferro-solda", "Ferro de solda", "Solda de componentes e reparos.", "alta", 60, 100),
    item("eletronica", "suporte-ferro-solda", "Suporte para ferro de solda", "Apoio seguro para o ferro durante a solda.", "alta", null, null),
    item("eletronica", "estanho", "Estanho", "Solda para eletrônica.", "alta", null, null),
    item("eletronica", "sugador-solda", "Sugador de solda", "Remover solda e corrigir ligações.", "media", null, null),
    item("eletronica", "protoboard", "Protoboard", "Montar circuitos sem solda.", "alta", null, null),
    item("eletronica", "jumpers", "Jumpers", "Ligações na protoboard, Arduino e ESP32.", "alta", null, null),
    item("eletronica", "esp32", "ESP32", "Wi-Fi e Bluetooth para projetos IoT.", "media", null, null),
    item("eletronica", "arduino", "Arduino", "Prototipagem e aprendizado em eletrônica.", "media", null, null),

    // 05 — Computadores / Servidor (planejamento)
    item("computadores", "srv-chaves-precisao", "Kit de chaves de precisão", "Manutenção de computadores, notebooks e servidor.", "media", null, null, {
      hint: "Também listado em Ferramentas: um único kit pode atender os dois usos.",
    }),
    item("computadores", "organizador-parafusos", "Organizador de parafusos", "Separar parafusos durante desmontagens.", "media", null, null),
    item("computadores", "pasta-termica", "Pasta térmica", "Manutenção de CPUs e GPUs.", "media", null, null),
    item("computadores", "abracadeiras", "Abraçadeiras", "Organização de cabos.", "media", null, null),
    item("computadores", "cabos-rede", "Cabos de rede", "Ligação do servidor e dos equipamentos de rede.", "media", null, null),
    item("computadores", "testador-cabo-rede", "Testador de cabo de rede", "Diagnóstico de cabos e conectores RJ45.", "baixa", null, null),
    item("computadores", "switch-rede", "Switch de rede", "Rede local do laboratório e do servidor.", "baixa", null, null),
    item("computadores", "nobreak-servidor", "Nobreak para servidor", "Proteção contra quedas e oscilações de energia.", "baixa", null, null),
    item("computadores", "rack-servidor", "Rack / estrutura do servidor", "Suporte físico e ventilação do servidor.", "baixa", null, null),

    // 06 — Fabricação (futuro)
    item("fabricacao", "impressora-3d", "Impressora 3D", "Peças, suportes e protótipos.", "baixa", null, null, { future: true }),
    item("fabricacao", "filamentos", "Filamentos", "Material para impressão 3D.", "baixa", null, null, { future: true }),
    item("fabricacao", "paquimetro-digital", "Paquímetro digital", "Medição precisa de peças.", "baixa", null, null, { future: true }),
    item("fabricacao", "micro-retifica", "Micro retífica", "Corte, desbaste e acabamento de precisão.", "baixa", null, null, { future: true }),
    item("fabricacao", "pistola-ar-quente", "Pistola de ar quente", "Termo-retrátil, retrabalho e moldagem.", "baixa", null, null, { future: true }),
    item("fabricacao", "serra-ferramentas-corte", "Serra / ferramentas de corte", "Cortes em madeira e outros materiais.", "baixa", null, null, { future: true }),
  ];

  return { priorities, categories, items };
});
