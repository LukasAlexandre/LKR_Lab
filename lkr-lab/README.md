# LKR LAB

**Build. Learn. Create.** — hub pessoal para desenvolvimento, laboratório, eletrônica, computadores, servidor, Arduino/ESP32, marcenaria, impressão 3D e projetos físicos/digitais.

Módulos estáticos em HTML, CSS e JavaScript puro. Abrem direto no navegador (`file://`), sem build, backend ou dependências, e não fazem nenhuma requisição de rede (CSP com `connect-src 'none'`).

## Módulos

| Módulo | Estado | Abrir |
| --- | --- | --- |
| Laboratório · Lab Setup | funcional | [`lab-setup/index.html`](lab-setup/index.html) |
| Desenvolvimento, Projetos, Eletrônica, Computadores, Servidor, Inventário | previstos (navegação visual apenas) | — |

## Estrutura

```
lkr-lab/
├── core/
│   ├── lkr-core.css    tokens (cores, tipografia, espaçamento, movimento), shell, botões, campos, modal, toast
│   └── lkr-core.js     LKR.core: DOM seguro (h), ícones, formatação, localStorage, toast, confirmDialog
└── lab-setup/
    ├── index.html      página do módulo
    ├── lab-setup.css   estilos do módulo
    ├── catalog.js      itens e categorias padrão (edite aqui para evoluir o checklist)
    ├── store.js        estado, mesclagem, persistência, importação/exportação, estatísticas
    ├── app.js          interface: renderização, eventos, autosave, filtros, modais
    └── store.test.js   testes do store (rodam no `npm test` do repositório)
```

Scripts clássicos no namespace `window.LKR`, não ES modules: navegadores bloqueiam módulos em `file://`. Um novo módulo reaproveita `core/` e segue o mesmo formato de `lab-setup/`.

## Lab Setup: dados

- Chave única no localStorage: `lkr-lab-setup-v1`. Estado centralizado:

  ```json
  {
    "version": 1,
    "createdAt": "…", "updatedAt": "…",
    "items":  { "furadeira-12v": { "completed": true, "completedAt": "…", "notes": "…", "updatedAt": "…" } },
    "custom": [ { "id": "custom-…", "category": "bancada", "name": "…", "priority": "media", "priceMin": 80, "priceMax": 150 } ],
    "ui":     { "status": "all", "category": "all" }
  }
  ```

- O estado guarda só o que você produziu; definições vêm de `catalog.js` e são mescladas por `id` a cada carregamento. Novos itens no catálogo aparecem sem apagar marcações ou observações. Nunca renomeie o `id` de um item existente.
- Dados ilegíveis no localStorage não são descartados: uma cópia é guardada em `lkr-lab-setup-v1:corrupt:<timestamp>` antes de recomeçar.
- **Backup:** *Exportar* gera `lkr-lab-setup-backup-AAAA-MM-DD.json`; *Importar* valida o arquivo, mostra um resumo e pede confirmação antes de substituir.
- O localStorage é por navegador e por perfil. Limpar os dados do navegador apaga o checklist: exporte backups periodicamente.

## Atalhos

`/` ou `Ctrl+K` buscar · `N` novo item · `Esc` limpar busca/filtros ou fechar modal.

## Testes

```powershell
npx vitest run lkr-lab
```
