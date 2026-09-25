# LK DEV HUB — DEVELOPMENT CHECKPOINT

Data: 2026-09-19. Branch: `feat/dev-hub-foundation`. Produto provisório: LK Dev Hub; repository: LK-Tools.

## Implementado

Fundação desktop Tauri 2/React/TypeScript strict/Rust/SQLite. UI operacional navy baseada no mockup. Cadastro de projetos, consultas Git, camadas OS e GitHub, launchers, prompts, contexto, worktrees em leitura e provider Claude. Não há conexões fictícias.

## Arquivos principais

- `src/App.tsx`: layout, rotas, dashboard, projeto ativo e ações.
- `src/features/`: Projects, Ports/Processes, Prompts.
- `src/shared/`: DTOs, IPC, templates, validação e componentes.
- `src-tauri/src/main.rs`: comandos IPC enumerados e diálogos.
- `crates/hub-core/src/`: banco, projetos, Git, GitHub, sistema, portas, agentes, launchers e snapshot.
- `crates/hub-core/migrations/001_initial.sql`: schema e templates iniciais.
- `scripts/validate-windows.ps1` e `.github/workflows/ci.yml`: validação Windows.
- `docs/images/current-preview.png`: captura real da UI em prévia web; não é captura desktop nativa.

## Arquitetura atual

React apresenta DTOs; Rust controla filesystem, SQLite e subprocessos. Hub-core independente permite testes sem GUI. CLI usa argumentos separados e fsmonitor desabilitado nas consultas Git. Providers proporcionais para Git hosting e agentes. Não há shell ou SQL genérico exposto ao renderer.

## Banco

SQLite local com WAL e FK, migration transacional versionada em user_version=1. Tabelas: projects, prompt_templates, activities. CRUD parametrizado, caminho único, timestamps UTC, delete não remove arquivos. Teste confirmou reabertura e persistência.

## Features funcionais

Implementadas em código: cadastro/edição/delete confirmado; descoberta; Git local; system metrics e CLI presence; sockets/processos; associação de expectativa de porta; kill com confirmação/identidade; launchers; gh PRs/issues/checks; templates e contexto local.

Verificadas em execução aqui: banco/core, Git fixture, descoberta, proteção de kill (sem matar processo), renderização de templates e prévia UI. Integrações nativas Windows ainda precisam de execução local.

## Testes executados

| Gate | Resultado |
|---|---|
| npm run typecheck | PASS |
| npm run lint | PASS |
| npm test | PASS — 14 testes |
| npm run build | PASS — bundle JS ~274 KB / gzip ~85 KB |
| cargo fmt --all -- --check | PASS |
| cargo test -p hub-core | PASS — 8 testes, 1 live explicitamente ignorado |
| cargo clippy -p hub-core --all-targets -- -D warnings | PASS |
| SQLite migration/CRUD/reopen | PASS, dentro da suíte Rust |
| Git fixture com commit e alteração local | PASS, dentro da suíte Rust |
| Snapshot sem conteúdo de .env | PASS com sentinela de teste |
| UI Chromium automatizada | PASS — 13 rotas, modal, Ctrl+K, CRUD nativo desabilitado em web |
| UI overflow horizontal a 1100px | Ausente |
| UI JavaScript page errors | 0 |
| Live sockets no Work | FAIL por restrição OS/FFI; não considerado validado |
| cargo check -p lk-dev-hub | BLOCKED em dependências gráficas/pkg-config |
| Desktop Windows/startup/IPC/NSIS | NOT VERIFIED |
| GitHub Actions | NOT VERIFIED nesta sessão |

O teste live de sockets não desapareceu: está marcado `ignore` com motivo e tem comando explícito tanto no script quanto no CI Windows. Nenhum teste encerrou processos reais.

A captura visual foi inspecionada. Chromium temporário de teste veio de pacote npm após falha no CDN do Playwright; não foi acrescentado às dependências do aplicativo.

## Entrega e sincronização

Três commits de implementação/documentação foram criados localmente. O push da branch foi tentado e falhou por ausência de autenticação de escrita do GitHub (`could not read Username`). Não houve merge. O ZIP de entrega contém o código versionado completo, sem node_modules, target, bancos ou secrets.

## Build status

Frontend pronto para servir como prévia. Núcleo Rust compila e passa no Clippy. **Não existe instalador Windows validado nesta entrega.** O shell Tauri está implementado, mas a compilação completa parou antes da checagem do código da aplicação por falta de dependências gráficas Linux.

## Limitações

- Work não acessa `C:\Users\LARos\Documents\Dev\LK-tools`.
- Ports/PIDs/launchers Windows não foram exercitados.
- CLI detection é presença, não saúde do daemon; conta Claude não consultada.
- VS Code requer Code.exe no PATH; code.cmd não é executado.
- Serviços/comandos customizados apenas cadastrados, sem runner livre.
- Snapshot não inclui conteúdo de documentação, TODOs, PRs ou issues automaticamente; marca ausência de consulta.
- Knowledge, env keys, sessões IA e worktree mutation são roadmap.
- CRUD completo pela ponte IPC desktop precisa de smoke test Windows; teste de banco não substitui isso.
- Confirm dialog no renderer não é autorização independente se o renderer for comprometido.

## Validações que precisam ser feitas no Windows

Instalar pré-requisitos Tauri; executar script de gates; abrir app; cadastrar pasta com espaços; reiniciar; confirmar persistência; validar Git/gh, CLIs e launchers; abrir listener descartável; verificar sockets e cancelamento de kill; validar export/copy; remover cadastro e conferir arquivos; testar NSIS.

## Pendências

Veja TASKS.md. A próxima fase é estabilizar o fluxo nativo Windows antes de adicionar módulos futuros.

## Como executar localmente

No checkout da branch:

```powershell
npm ci
npm run tauri dev
```

Gates automatizados:

```powershell
.\scripts\validate-windows.ps1
```

Não altere políticas do PowerShell para executar o script se estiver bloqueado: os mesmos comandos estão no README e podem ser executados manualmente.

## Árvore resumida

```text
LK-Tools/
  src/
    App.tsx
    features/
    shared/
    styles.css
  src-tauri/
    src/main.rs
    tauri.conf.json
  crates/hub-core/
    src/
    migrations/
    tests/
  docs/
    adr/
    images/
    PRODUCT_SPEC.md
    ARCHITECTURE.md
    SECURITY.md
    DATABASE.md
    DEVELOPMENT.md
    ROADMAP.md
    DEPENDENCIES.md
    CHECKPOINT.md
  scripts/validate-windows.ps1
  .github/workflows/ci.yml
  README.md
  TASKS.md
```
