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
| `machine` (`machine_id`, nome, uso, descrição, snapshot detectado) | `hub_core::machine` (cadastro e detecção) | gate global, Configurações | `hub.db`, migration 005 | C — identidade desta instalação; nunca entra no workspace portátil |

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

Desktop: o `hub.db` fica no app data e não viaja. **Identidade do projeto != caminho local**: o cadastro (`projects`) guarda só dados portáteis; a pasta é um vínculo desta máquina (`project_bindings`, migration 003). Fonte de verdade: dados portáteis → `data/workspace.json` (formato em `lkr-lab/core/lkr-workspace.js`, validado de novo em `hub-core::portable`); vínculos → SQLite local; processos/portas/Git → detecção em tempo real; caches reconstruíveis. Cada projeto está `available`, `missing` (vínculo sem pasta) ou `unbound` (nunca localizado aqui); nesses dois últimos nenhuma operação local roda e a interface oferece "Localizar" (`bind_project`, que exige o mesmo remote — HTTPS e SSH equivalem — ou confirmação explícita quando o projeto não tem repositório). `apply_portable` atualiza o SQLite sem tocar nos vínculos. Ainda não há relocação automática (o rebind é manual por “Localizar”).

### Sync do workspace (desktop → bridge → Git)

Fluxo explícito (botão “Sincronizar” no topo; nada em segundo plano, nunca push automático): `export_portable` (forma canônica) → hash → `hub-core::sync` → bridge local (`GET /api/remote/workspace`, `POST /api/sync/workspace`, `POST /api/update/workspace?strict=1`) → Git → `apply_portable`. O Git roda só no bridge (`git-backup.mjs`): commit só de `data/workspace.json` (`--only`, verificado depois), recusa merge/rebase/cherry-pick em andamento e HEAD destacado, nunca força push. O cliente Rust (`bridge.rs`) só fala HTTP com 127.0.0.1 e confere que quem responde é o bridge.

Três hashes (SHA-256 do JSON canônico, **sem** carimbos de data, listas por id): LOCAL (SQLite agora), REMOTO (arquivo em `origin/<branch>`, lido sem tocar na árvore de trabalho) e BASE (último acordo; `sync_state` no SQLite, metadado desta máquina, migration 004 — nunca entra no workspace portátil).

| | remoto = base | remoto ≠ base |
|---|---|---|
| local = base | limpo | remoto mudou → aplica |
| local ≠ base | local mudou → publica | **diverge** → nada é sobrescrito |

Divergência só se resolve por escolha explícita (usar o remoto / manter o local, ambas confirmadas na interface). Máquina nova (sem projetos, só prompts de fábrica) adota o remoto. A base e `last_applied_hash` só avançam depois do sucesso (apply + base na mesma transação; push só após confirmação do bridge). Se o usuário edita durante o sync, o apply é abortado. Bridge fora do ar ou GitHub sem conexão → “Offline”/“Sync indisponível”, o app local segue; a abertura nunca espera a rede (estado local instantâneo; remoto consultado em segundo plano, no máximo a cada 5 min ao voltar à janela).

Preferências portáteis (`sidebarCompact`, `density`, `promptFavorites`, conforme `PREFERENCE_SCOPE`) são enviadas pela interface ao SQLite (`portable_preferences`), exportadas e, depois de um apply, devolvidas à interface. A forma canônica Rust e a de `lkr-workspace.js` são verificadas contra o mesmo arquivo (`crates/hub-core/tests/fixtures/workspace-golden.json`).

Limites: o repositório de sync é o próprio checkout do LKR LAB; se o remoto andou por código e a árvore tem alterações de desenvolvimento, o sync para (`LOCAL_CHANGES`) e pede `git pull` manual — nunca move a árvore de quem desenvolve. Não há merge estrutural.

## Migrações

- **Metadados de sync v1 → v2** (`lkr-portable.js`): `lastSyncedHash` vira `baseHash`; o JSON original fica em `lkr-lab-setup-v1:sync:v1` antes da conversão; campos desconhecidos são descartados. Rodar de novo não muda nada. Schema mais novo é lido com padrões e nunca sobrescrito.
- **Dados legados do Lab Setup** (só no navegador): não precisam de conversão de formato. Na primeira reconciliação viram "não sincronizado" (sem arquivo), "em dia" (iguais) ou conflito (diferentes); `migratedAt` registra quando aconteceu.
- **Preferências do desktop** (`lk.sidebarCompact`, `lk.density`, `lk.activeProject`, `lk.ports.*`, `lk.prompt.favorites`, `lk.knowledge.draft` → `lk.preferences`): cada valor é validado; as chaves antigas só são removidas depois que a nova é gravada e relida igual. Idempotente.

## Segurança

- O arquivo portátil passa pela mesma validação de importação (`validateImport`): ids com padrão restrito, textos com limite, campos desconhecidos descartados, schema futuro recusado.
- O bridge só lê/escreve caminhos fixos definidos no servidor (`MODULES`), nunca vindos da requisição; não há rota de comando; Git roda com `execFile`, argumentos fixos e sem prompts; mensagens de erro passam por `redact`.
- Metadados e preferências guardam só strings curtas e validadas; nada de tokens.
- `data/*.tmp-*` (escrita atômica) está no `.gitignore`.

## Project Runtime Manager (estado desta máquina)

O runtime de um projeto é **derivado da máquina real a cada leitura** e nunca vai para o workspace portátil nem para o SQLite: stack e gerenciador (por arquivos/lockfiles da raiz, com evidência), scripts do `package.json`, resumo do Git DO PROJETO (`git::summary`, só leitura — não tem relação com o repositório de sync do bridge), processos e portas com dono verificado. Estados: `unbound`, `missing`, `ready`, `running`, `partial`, `error` (estado de runtime ≠ estado do workspace).

- **Dono de processo** (`system::processes_managed`), só com evidência: `managed` (PID na árvore iniciada pelo LKR LAB), `cwd` (dentro da pasta vinculada; comparação por componentes, sem o prefixo verbatim do Windows) ou `descendant` de processo gerenciado. Projeto sem pasta nunca é dono de nada. Porta só é do projeto se o PID dono for do projeto.
- **Managed × externo**: só o que o supervisor iniciou pode ser parado daqui. Processo externo aparece como “Em execução externamente” e não tem Parar (adoção fica para depois).
- **Rodar** (`supervisor.rs`): `<gerenciador> run <script>` no diretório vinculado, sem shell nosso; o script precisa existir no package.json LOCAL e ter nome `[A-Za-z0-9][A-Za-z0-9:_.-]*`. **Fronteira de confiança**: nada vindo do workspace.json, do Git remoto do LKR LAB ou de `Project.commands` vira comando. Sem lockfile (ou com lockfiles conflitantes) o gerenciador não é inventado e Rodar fica indisponível com o motivo.
- **Árvore de processos**: cada execução vive num Job Object do Windows (`KILL_ON_JOB_CLOSE`); Parar/Reiniciar encerram npm → node → vite por handle (nunca por nome, imune a reuso de PID) e Reiniciar espera a árvore sumir antes de iniciar. Ao sair do app nada fica órfão.
- **Estado e logs**: eventos `runtime://event` (starting, running, stopping, stopped, failed, completed) sem polling; stdout/stderr em buffer de 2000 linhas (ANSI removido), buscado só para a execução aberta. Externos: consulta leve a cada 5 s com a janela visível.
- **Web**: o controle é exclusivo do desktop; a web mostra o painel desabilitado (“Disponível no aplicativo desktop”).
- **Contexto IA** (`snapshot::generate`) inclui um resumo do runtime com nomes e estados — nunca o texto dos scripts (podem conter segredos).
