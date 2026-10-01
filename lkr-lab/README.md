# LKR LAB

**Build. Learn. Create.** — hub pessoal para desenvolvimento, laboratório, eletrônica, computadores, servidor, Arduino/ESP32, marcenaria, impressão 3D e projetos físicos/digitais.

Módulos estáticos em HTML, CSS e JavaScript puro, sem build nem dependências. Abrem direto no navegador (`file://`, só cache local) ou pelo bridge local `npm run lab` (estado portátil e GitHub Sync). A única rede usada é a própria origem (CSP `connect-src 'self'`).

## Módulos

| Módulo | Estado | Abrir |
| --- | --- | --- |
| Laboratório · Lab Setup | funcional | [`lab-setup/index.html`](lab-setup/index.html) |
| Desenvolvimento, Projetos, Eletrônica, Computadores, Servidor, Inventário | previstos (navegação visual apenas) | — |

## Estrutura

```
lkr-lab/
├── bridge/
│   ├── server.mjs        npm run lab: serve os módulos em 127.0.0.1 e expõe a API fixa de estado/Git
│   └── git-backup.mjs    lê/escreve data/<módulo>.json e publica SÓ esse arquivo (sem force)
├── core/
│   ├── lkr-core.css      tokens (cores, tipografia, espaçamento, movimento), shell, botões, campos, modal, toast
│   ├── lkr-core.js       LKR.core: DOM seguro (h), ícones, formatação, adaptador de armazenamento, bridge, toast
│   └── lkr-portable.js   LKR.portable: reconciliação cache local × arquivo portátil, metadados da máquina
└── lab-setup/
    ├── index.html        página do módulo
    ├── lab-setup.css     estilos do módulo
    ├── catalog.js        itens e categorias padrão (edite aqui para evoluir o checklist)
    ├── store.js          estado, mesclagem, cache local, importação/exportação, estatísticas
    ├── app.js            interface: renderização, eventos, autosave, filtros, modais
    └── sync.js           estado portátil: reconciliação automática, conflito, GitHub Sync
```

Scripts clássicos no namespace `window.LKR`, não ES modules: navegadores bloqueiam módulos em `file://`. Um novo módulo reaproveita `core/` e segue o mesmo formato de `lab-setup/`.

## Lab Setup: dados

Mapa completo e classes de estado em [docs/STATE.md](../docs/STATE.md).

- **Portátil:** `data/lab-setup.json` no repositório (marcações, observações, itens personalizados). Ao abrir pelo bridge (`npm run lab`), a página se reconcilia com ele: máquina nova ou `git pull` sem edições locais → adota automaticamente (com "Desfazer"); edições locais → "não sincronizado" até o *Sincronizar agora*; os dois mudaram → conflito, nada é sobrescrito e você escolhe a versão.
- **Desta máquina:** filtros (`ui`), metadados de sync (`lkr-lab-setup-v1:sync`) e o estado guardado para "Desfazer". Nunca vão para o Git.
- Cache local (localStorage), chave `lkr-lab-setup-v1`. Estado centralizado:

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
- O localStorage é por navegador, perfil e endereço (o bridge redireciona `localhost` para `127.0.0.1` para manter um só). Limpar os dados do navegador apaga só o que ainda não foi sincronizado.

## Atalhos

`/` ou `Ctrl+K` buscar · `N` novo item · `Esc` limpar busca/filtros ou fechar modal.

## Testes

```powershell
npx vitest run lkr-lab
```
