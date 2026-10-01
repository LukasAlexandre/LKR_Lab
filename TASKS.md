# LKR LAB — Tasks

## NOW

- [x] Produto, arquitetura, threat model, ADRs e documentação.
- [x] Monorepo Tauri / React / hub-core, TS strict, CSS navy, sidebar e 13 rotas.
- [x] SQLite migration transacional, CRUD e persistência confirmada em teste de reabertura.
- [x] Descoberta de pasta, stack e remote seguro sem executar scripts.
- [x] Git real: status, HEAD, upstream, ahead/behind e commits.
- [x] gh provider: auth status, PRs, checks, review, mergeability e issues (validação real autenticada pendente).
- [x] Portas/processos nativos implementados; PID/start time + confirmação para kill.
- [x] Associação manual de porta esperada; CWD canonicalizado para inferência, priorizando projeto mais específico.
- [x] Launchers de pasta/Windows Terminal/Code.exe/Claude nativo.
- [x] Templates globais/por projeto, cópia e variáveis com NOT VERIFIED quando faltar fonte.
- [x] Snapshot local visualizável, copiável e exportável por diálogo.
- [x] Provider Claude com instruções/skills; presença MCP, sem credenciais.
- [x] Worktrees em leitura e paleta Ctrl+K.
- [x] Typecheck, lint, build web, 14 testes frontend, 8 testes Rust, Clippy.
- [x] Inspeção visual de 13 rotas, modal/paleta e ausência de overflow horizontal a 1100px.
- [x] Estado portátil × estado da máquina ([docs/STATE.md](docs/STATE.md), ADR-004): Lab Setup reconcilia com `data/lab-setup.json` (adoção automática segura, pendência, conflito), metadados de sync v2 com migração; preferências do desktop tipadas com escopo e migração das chaves antigas; `pathAvailable` por máquina.

## NEXT

- [ ] Workspace portátil do desktop: exportar projetos (sem `local_path`), prompts, knowledge e preferências `portable` para `data/workspace.json`; religar caminho por máquina (localizar, clonar, remover referência).
- [ ] Rodar `scripts/validate-windows.ps1` e checklist manual de docs/DEVELOPMENT.md.
- [ ] Confirmar startup Tauri, IPC, seletor de pasta e persistência via UI Windows.
- [ ] Validar portas TCP/UDP IPv4/IPv6, PID e kill somente em processo descartável.
- [ ] Validar gh autenticado e launchers em paths com espaços/Unicode.
- [ ] Gerar e testar instalador NSIS; assinatura ainda não configurada.
- [ ] Endurecer runner: cancelamento de árvore, prazos por fonte e confirmação no host.
- [ ] Serviços: comandos visíveis, execução controlada, stop/restart confirmados.
- [ ] Expandir snapshot com GitHub e documentação opt-in.

## LATER

Environment-key validation, worktree create/remove, skills/MCP management, sessões IA, knowledge/Obsidian, terminal integrado, scan limitado de pasta raiz, cloud/marketplace somente após produto local estável.

## BLOCKED / NOT VERIFIED

- Push da branch: falta autenticação de escrita no GitHub nesta sessão; código entregue também em ZIP.

- Janela/build desktop não verificados: Work é Linux sem pkg-config/GTK/WebKit2GTK; instalação via apt falhou por restrições do ambiente.
- Enumeração live de sockets falhou no Work (`Failed to call ffi`). Teste separado exige OS host/Windows e é executado explicitamente no workflow Windows.
- Caminhos, processos e CLIs do computador Windows do usuário não são acessíveis nesta sessão.
- GitHub Actions/instalador: status ainda não observado; não presumir CI verde.

Não há merge automático, force push, deploy ou instalação no computador do usuário.
