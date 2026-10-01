# Estado: onde vive, para onde vai

Princípio: **abrir o LKR LAB em outra máquina e continuar de onde parou**, sem levar junto o que só faz sentido nesta máquina. Decisão registrada em [ADR-004](adr/ADR-004-portable-state.md).

## Classes de estado

| Classe | Vai para o Git? | Exemplos |
| --- | --- | --- |
| **A. Versionado** | sim, como código | código, catálogo do Lab Setup, docs, migrations, configuração do projeto |
| **B. Portátil do usuário** | sim, por sync explícito | checklist do Lab Setup (marcações, observações, itens personalizados); preferências marcadas `portable` |
| **C. Desta máquina** | nunca | caminhos absolutos, ids do SQLite local, PIDs, portas, filtros, metadados de sync, "desfazer", rascunhos |
| **D. Segredos** | nunca | tokens, credenciais (não são lidos nem armazenados pelo LKR LAB; `.env` ignorado) |
| **E. Efêmero** | não persiste | modal aberto, busca digitada, loading, hover |

Regra de ouro: o que é **intenção/configuração desejada** (B) fica separado do que é **observação da máquina** (C). "Quero este projeto no workspace" é B; "a pasta dele existe em `D:\Dev\...`" é C e é recalculado a cada leitura.

## Mapa atual

| Fonte | Quem grava | Quem lê | Onde vive | Classe |
| --- | --- | --- | --- | --- |
| `lkr-lab-setup-v1` (`items`, `custom`) | `lab-setup/store.js` | store, app.js | localStorage (cache local) | B — publicado em `data/lab-setup.json` |
| `lkr-lab-setup-v1` (`ui`) | store.js (`setUi`) | app.js | localStorage | C — nunca entra no arquivo portátil nem é trocado por restauração |
| `lkr-lab-setup-v1:sync` (schema 2) | `sync.js` via `LKR.portable.createMetaStore` | sync.js | localStorage | C — `baseHash`, último sync, última tentativa |
| `lkr-lab-setup-v1:sync:v1` | migração | ninguém (cópia de segurança) | localStorage | C |
| `lkr-lab-setup-before-restore` | store.js (`importData`) | "Desfazer" | localStorage | C |
| `lkr-lab-setup-v1:corrupt:<ts>` | store.js (`load`) | manual | localStorage | C — conteúdo ilegível preservado |
| `data/lab-setup.json` | bridge (`POST /api/lab-sync`) | bridge (`GET /api/lab-state`) | repositório Git | B |
| `lk.preferences` (schema 1) | `src/shared/preferences.ts` | App, Ports, Prompts, Knowledge, workspace | localStorage do WebView | campo a campo: `PREFERENCE_SCOPE` |
| `hub.db` (projetos, prompts, knowledge, atividades) | `hub-core` (Rust) | renderer via IPC | `%APPDATA%/br.com.lktechnologies.devhub` | C hoje (inclui `local_path`); projetos/prompts/knowledge são candidatos a B |
| `pathAvailable` | `hub_core::projects::entry` | Repositórios, detalhe do projeto | calculado em `list_projects` | C — nunca persistido |

Preferências do desktop (`PREFERENCE_SCOPE`):

| Campo | Escopo |
| --- | --- |
| `sidebarCompact`, `density`, `promptFavorites` | portable |
| `activeProjectId`, `portsFilter`, `portsProtocol`, `knowledgeDraft` | machine |

## Fluxo do Lab Setup

```
UI (app.js) ──▶ store.js ──▶ cache local (autosave instantâneo, também em file://)
                     │
sync.js ── reconcile(local, base, arquivo) ──▶ GET /api/lab-state ──▶ data/lab-setup.json
   │
   └── "Sincronizar agora" ──▶ POST /api/lab-sync ──▶ escreve, commita SÓ esse arquivo, push
```

A **base** é o hash do conteúdo portátil com o qual este navegador se reconciliou por último. Ao abrir a página, ao voltar à aba e depois de "Verificar atualização":

| Situação | Resultado |
| --- | --- |
| local = arquivo | em dia; registra a base |
| cache vazio, sem base (máquina nova) | adota o arquivo — "Estado restaurado do repositório" |
| só o arquivo mudou (git pull) | adota, com "Desfazer" |
| só o local mudou | "Alterações não sincronizadas" até o sync |
| os dois mudaram, ou dados antigos do navegador diferentes do arquivo | **conflito**: nada é sobrescrito; o usuário escolhe "Usar a do repositório" ou "Manter a deste navegador" (e sincroniza) |
| arquivo ausente | cache vale sozinho; primeiro sync cria o arquivo |
| arquivo inválido | nada é adotado nem sobrescrito; sync bloqueado com aviso |

Nenhuma reconciliação escreve no repositório. Commits só acontecem no clique de sync, levando apenas `data/lab-setup.json`, e nunca com force.

## Nova máquina

```
PC casa: marca itens → Sincronizar agora → commit + push de data/lab-setup.json
PC trabalho: git clone / git pull → npm run lab → abre http://127.0.0.1:4317/lab-setup/
           → cache vazio → adota data/lab-setup.json → filtros/metadados começam do zero nesta máquina
```

O bridge redireciona `localhost` para `127.0.0.1`: o cache do navegador é separado por endereço e assim existe um só.

Desktop: o `hub.db` fica no app data e não viaja. Os caminhos de uma máquina nunca são aplicados em outra; quando um projeto aponta para uma pasta que não existe aqui, a interface mostra "Pasta não encontrada nesta máquina". A exportação portátil do workspace (projetos sem `local_path`, prompts, knowledge e preferências `portable`) é o próximo bloco: o caminho será religado por máquina (localizar, clonar ou remover a referência).

## Migrações

- **Metadados de sync v1 → v2** (`lkr-portable.js`): `lastSyncedHash` vira `baseHash`; o JSON original fica em `lkr-lab-setup-v1:sync:v1` antes da conversão; campos desconhecidos são descartados. Rodar de novo não muda nada. Schema mais novo é lido com padrões e nunca sobrescrito.
- **Dados legados do Lab Setup** (só no navegador): não precisam de conversão de formato. Na primeira reconciliação viram "não sincronizado" (sem arquivo), "em dia" (iguais) ou conflito (diferentes); `migratedAt` registra quando aconteceu.
- **Preferências do desktop** (`lk.sidebarCompact`, `lk.density`, `lk.activeProject`, `lk.ports.*`, `lk.prompt.favorites`, `lk.knowledge.draft` → `lk.preferences`): cada valor é validado; as chaves antigas só são removidas depois que a nova é gravada e relida igual. Idempotente.

## Segurança

- O arquivo portátil passa pela mesma validação de importação (`validateImport`): ids com padrão restrito, textos com limite, campos desconhecidos descartados, schema futuro recusado.
- O bridge só lê/escreve caminhos fixos definidos no servidor (`MODULES`), nunca vindos da requisição; não há rota de comando; Git roda com `execFile`, argumentos fixos e sem prompts; mensagens de erro passam por `redact`.
- Metadados e preferências guardam só strings curtas e validadas; nada de tokens.
- `data/*.tmp-*` (escrita atômica) está no `.gitignore`.
