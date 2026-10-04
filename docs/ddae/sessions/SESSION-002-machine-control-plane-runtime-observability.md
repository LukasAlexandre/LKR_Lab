# SESSION-002 — Machine Control Plane & Runtime Observability

**PT:** Plano de controle local da máquina e observabilidade de runtimes
**Tipo:** Feature
**Status:** ACTIVE / ATIVA
**Branch:** `feat/session-002-machine-control-plane` (checkout principal; nenhuma Managed Worktree)
**Planning Item:** Machine Control Plane & Runtime Observability (EM EXECUÇÃO)

## Objetivo

Construir a camada de observabilidade e controle local do LKR LAB para processos, portas, runtimes, consoles e saúde da máquina, relacionando o que roda no computador a Project, Worktree e Session **somente quando há evidência**.

## Blocos

| # | Bloco | Status |
|---|-------|--------|
| 01 | Control Plane Architecture | IMPLEMENTADO E VALIDADO NO DESKTOP |
| 02 | Process & Port Discovery | IMPLEMENTADO E VALIDADO NO DESKTOP |
| 03 | Runtime Attribution Engine | IMPLEMENTADO E VALIDADO NO DESKTOP |
| 04 | Managed Runtime Supervisor | IMPLEMENTADO E VALIDADO NO DESKTOP |
| 05 | Live Console Hub | IMPLEMENTADO E VALIDADO NO DESKTOP |
| 06 | Machine Telemetry Expansion | PENDENTE |
| 07 | Windows Health & Integrity | PENDENTE |
| 08 | Network & Security Visibility | PENDENTE |
| 09 | Alerts & Diagnostics | PENDENTE |
| 10 | Validation & Hardening | PENDENTE |

Progresso no DDAE: **5 / 10**, sem bloco em andamento; próximo: **06 — Machine Telemetry Expansion** (não iniciado). A SESSION-002 **continua ATIVA** e o Planning Item **continua EM EXECUÇÃO**.

## Checkpoint — CONTROL PLANE MVP (Blocks 01–05)

### Arquitetura

O Control Plane é uma camada de contrato sobre a infraestrutura que já existia; não há supervisor, inventário ou buffer paralelos.

- **Observação (somente leitura):** `system::inventory` (processos: PID, pai, executável, linha de comando, cwd, início, CPU, memória, E/S) e `netstat2` (TCP em escuta, IPv4/IPv6, PID dono). Nada é encerrado, fechado ou alterado.
- **Atribuição:** PROCESSO → PROJECT → WORKTREE → SESSION → BLOCK, sempre com `Confidence` (`exact`, `high`, `medium`, `unknown`) e a lista de evidências. Sem evidência o resultado é `unknown` e a interface mostra "—". O Project ativo nunca é um palpite. Session e Block só vêm pela Worktree vinculada (sem FK redundante).
- **Runtimes:** MANAGED (iniciado pelo LKR LAB: Job Object do `Supervisor`, stdout/stderr capturados) e DISCOVERED (externo: sem pipe, logo sem console retroativo).
- **Console:** ring buffer por execução já existente, agora limitado a 10 000 linhas **e** 8 MB, com timestamp local por linha. Estado de máquina: nunca vai ao workspace portátil, ao Git nem ao sync. Linhas de comando são redigidas (token, senha, credenciais em URL) antes de sair do módulo.
- **Streaming:** evento `output` do supervisor avisa e o frontend busca só as linhas novas (`runtime_logs(since)`); sem polling do buffer inteiro. Snapshot do Control Plane a cada ~4 s com a janela visível.
- **UI:** painel "Control Plane" na área `#project/<id>/runtime` (sem rota nova): Managed Runtimes, Detected Services, outros serviços da máquina (recolhido), console com ALL/STDOUT/STDERR, busca, pausa/retomada, limpar vista (apenas visual), copiar e autoscroll.

## VALIDAÇÃO DESKTOP REAL

Executada no app Tauri em execução (`tauri dev`), sem reiniciá-lo, sobre o Project LKR_Lab no checkout principal. Os números abaixo são **observações daquele snapshot**, não requisitos.

- O Control Plane exibiu cerca de 500 processos e 51 portas em escuta.
- **Vite** (PID 25904, `:1420`) apareceu como Detected, com atribuição **Alta** ao LKR_Lab pela evidência real "pasta de trabalho dentro de LKR_Lab" (linha de comando e cwd visíveis na árvore).
- Serviços sem evidência (Docker, TeamViewer_Service, Node.js, Spotify, Weixin, bun, entre outros) ficaram **Desconhecida**, com Project/Worktree/Session/Block "—".
- Todo processo externo mostrou "Console não disponível — processo iniciado fora do LKR LAB.", mantendo PID, portas, CPU, memória, uptime e árvore.
- **Managed Runtime:** `npm run lab` (script real do Project, bridge local em `127.0.0.1:4317`) foi iniciado pelo LKR LAB. Apareceu MANAGED / RUNNING, atribuição **Exata**; o listener `:4317` foi associado à árvore real (`cmd.exe → node.exe (npm) → cmd.exe → node.exe`).
- **Console:** stdout real (banner do bridge) com timestamps; STDERR exibiu corretamente o estado vazio (o processo não escreveu em stderr); ALL, STDOUT e STDERR, busca, Pausar/Retomar (o processo e a porta seguiram ativos durante a pausa), Copiar, Limpar vista (processo intacto) e Autoscroll funcionaram.
- **Stop:** a execução passou a STOPPED; a árvore inteira do `lab` encerrou e `:4317` foi liberada; o console continuou legível. A transição intermediária STOPPING foi rápida demais para ser vista na tela (coberta por teste automatizado).
- **Restart:** nova execução RUNNING com stream próprio; a anterior ficou STOPPED como histórico; havia **uma única** instância viva (um `npm run lab`, um dono de `:4317`).
- Ao final o `lab` foi parado. **Vite, o app LKR LAB e a porta 1420 permaneceram ativos.**
- Durante o Stop, dois PIDs da linha de base de processos externos desapareceram. Um controle sem intervenção mostrou **churn natural** de processos efêmeros (principalmente `conhost.exe`) na máquina; a identidade desses dois PIDs não pôde ser comprovada porque a linha de base guardou só PIDs. O isolamento da árvore gerenciada é garantido e testado pelo Job Object (ver cobertura automatizada), não por esta contagem.
- A validação encontrou uma falha real: o console não mostrava o timestamp de cada linha. Corrigido (`fbf4fc0`) e reconfirmado na janela real via HMR.

## COBERTURA AUTOMATIZADA

- **Rust (`hub-core`):** 22 testes de `control_plane` (atribuição por cwd/executável/ancestral/porta, "desconhecido continua desconhecido", fronteira de pasta, PID reutilizado, redação, descoberta e árvore, runtime gerenciado Exato com Stop que não toca no externo, passividade, estado fora do workspace portátil, eventos de saída, limite por bytes, processo muito verboso sem ouvinte, servidor externo que não herda atribuição Exata) e testes de supervisor (stop duplo, STOPPING→STOPPED, `ended_at`, FAILED com exit code, restart sem duplicar, cauda do log por índice, buffer limitado). Subprocessos temporários controlados, com guard que os encerra mesmo se a asserção falhar.
- **Frontend (vitest):** lógica pura do console (filtros, busca, pausa/retomada, limpar vista, teto de renderização, autoscroll, cópia), agrupamento Managed/Detected/outros, "—" na atribuição, e renderização do hub (Managed, Discovered, sem atribuição, STOPPED/FAILED, portas, console disponível e indisponível, stdout/stderr, timestamps, estados vazios).
- **Gates finais:** `npm test` (443), `npm run lint`, `npm run typecheck`, `npm run build`, `cargo fmt --check`, `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` e `cargo test --workspace` passaram.
- **`stacks_exec`:** intermitente histórico (`cargo_run_build_test_check_and_clippy_complete_with_exit_codes_and_logs`). Reproduzido também no commit `0336d93`, anterior ao Control Plane (4 falhas em 8 rodadas), portanto **não foi introduzido por esta Session**. Passou em outras rodadas, inclusive na final. O teste não foi alterado.

## LIMITAÇÕES

1. Processo externo não oferece stdout/stderr retroativo (não há pipe).
2. O Start continua baseado nos recursos/scripts locais conhecidos do Project (não aceita comando livre).
3. A Managed Execution ainda não recebe Worktree opcional explicitamente no Start; a Worktree é inferida pelo `cwd` quando possível.
4. O inventário de processos usa cache curto (1 s); um processo recém-criado pode demorar até 1 s para aparecer.
5. `stacks_exec` tem flakiness histórica.
6. Não existe Windows Service / Agent privilegiado: processos protegidos não expõem caminho nem linha de comando sem elevação e não podem ser associados a um Project.
7. Os Blocks 06–10 não foram implementados.
8. STDERR com conteúdo real não foi observado no desktop (o processo usado não escreve em stderr); está coberto por teste automatizado com subprocesso real.
9. O caminho Worktree → Session → Block foi validado apenas por teste automatizado: o desktop está no checkout principal, sem Managed Worktree.

## Critérios de conclusão

Os 10 critérios continuam **não marcados** neste checkpoint, de propósito. Evidência reunida até aqui, para a revisão no fechamento da Session:

- Inventário de processos e mapeamento de portas: demonstrados no desktop e por teste.
- Diferenciar runtime conhecido de processo não associado; UI sem associação falsa com confiança insuficiente: demonstrados (Unknown) e por teste.
- Relação com Project/Worktree por evidência: Project demonstrado no desktop; Worktree só por teste.
- stdout/stderr capturados e Console Hub com logs ao vivo: stdout no desktop; stderr por teste.
- Coleta local e páginas de observação sem ação mutante: cobertos por teste (passividade e workspace portátil).
- "Gates e validação desktop passam": depende da Session inteira (Blocks 06–10).

## Bugs corrigidos nesta Session

- Falsa atribuição de serviço externo a árvore Managed (a árvore engolia irmãos de um hospedeiro comum e herdava confiança Exata).
- Stop duplo emitindo `Stopping` repetido.
- Parser legado da SESSION-001 aceitando um título final incorreto (rodapé `# SESSION-001 FINALIZADA`).
- `useRunLogs` sem `getServerSnapshot`.
- Vazamento de processos Node nos testes quando uma asserção falhava.
- Timestamps ausentes na visualização do console.

## Próximo bloco

**06 — Machine Telemetry Expansion** (não iniciado). Um agente privilegiado só será avaliado se uma informação concreta o exigir (Blocks 07–08).
