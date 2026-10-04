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
| 06 | Machine Telemetry Expansion | IMPLEMENTADO E VALIDADO NO DESKTOP |
| 07 | Windows Health & Integrity | IMPLEMENTADO E VALIDADO NO DESKTOP |
| 08 | Network & Security Visibility | IMPLEMENTADO E VALIDADO NO DESKTOP |
| 09 | Alerts & Diagnostics | PENDENTE |
| 10 | Validation & Hardening | PENDENTE |

Progresso no DDAE: **8 / 10**, sem bloco em andamento; próximo: **09 — Alerts & Diagnostics** (não iniciado). A SESSION-002 **continua ATIVA** e o Planning Item **continua EM EXECUÇÃO**.

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
7. Na data deste checkpoint, os Blocks 06–10 ainda não estavam implementados (o 06 foi entregue no checkpoint seguinte).
8. STDERR com conteúdo real não foi observado no desktop (o processo usado não escreve em stderr); está coberto por teste automatizado com subprocesso real.
9. O caminho Worktree → Session → Block foi validado apenas por teste automatizado: o desktop está no checkout principal, sem Managed Worktree.

## Checkpoint — MACHINE TELEMETRY (Block 06)

Block 06 — Machine Telemetry Expansion: **IMPLEMENTADO E VALIDADO NO DESKTOP**. SESSION-002: **ATIVA, 6 / 10**, sem bloco atual; próximo: **07 — Windows Health & Integrity** (não iniciado). Nada do 07 foi implementado.

### Arquitetura

O Block 06 evolui o sampler de telemetria que já existia (Concept 02), sem criar inventário ou comando paralelos: o contrato `Telemetry` ganhou campos e o comando/evento existentes (`machine_telemetry`, `machine://telemetry`) continuam sendo a única via.

- **Estático × vivo:** o hardware estático (modelo da CPU, núcleos, RAM total, identidade das GPUs) continua no inventário; a telemetria só carrega o que muda. O clock base da CPU é lido uma vez; tipo/estado/velocidade das interfaces de rede são relidos a cada 30 s e os discos físicos a cada 5 min.
- **Fontes nativas, em processo:** sysinfo, PDH, D3DKMT, `GetSystemPowerStatus`, `GetPerformanceInfo`, `GetAdaptersAddresses`, registro e consultas de propriedade de disco. **Sem PowerShell, sem WMI e sem subprocesso periódico**; há um teste que falha se o coletor passar a usar `Command::new` ou PowerShell.
- **Domínios independentes:** cada domínio (CPU, memória, GPU, disco, rede, bateria, temperaturas) é medido separadamente e reporta `available`, `partial` ou `unavailable`. A falha ou ausência de um sensor nunca derruba os outros nem aparece como erro.
- **Primeira amostra:** CPU, disco e rede trazem `ready`; antes do primeiro intervalo a interface mostra "Calibrando…", nunca um zero inventado.
- **Local:** nada é persistido e nada entra no Portable Workspace, no Git, no sync, no Planejamento ou no DDAE (há teste). O histórico continua sendo o buffer curto em memória (2 min).
- **Refresh (Dashboard aberto):** CPU/memória a cada 1 s, E/S, rede e GPU a cada 2 s, temperaturas a cada 4 s (antes 10 s); em segundo plano 5 s, 10 s e 60 s. O Dashboard renova um lease de 15 s; ao sair dele o sampler reduz o ritmo.

### O que o contrato passou a expor

- **CPU:** uso por processador lógico, clock base (registro) além do clock efetivo, e `ready`.
- **Memória:** RAM física separada de **commit** (RAM + pagefile) e de **pagefile em uso**.
- **GPU:** fabricante (VendorId PCI) e versão do driver (registro DirectX), por adaptador.
- **Disco:** atividade, taxas de leitura/escrita e operações por segundo **por disco físico** (modelo, NVMe, letras dos volumes), separadas da capacidade dos volumes.
- **Rede:** todas as interfaces (sem loopback) com tipo, estado, velocidade de enlace, IPv4, IPv6 (sem link-local), taxas por interface e a interface ativa.
- **Bateria:** presença, carga, tomada, carregando e autonomia; computador sem bateria é "não aplicável", nunca 0%.
- **Disponibilidade:** `availability` por domínio e novas `capabilities` (núcleos, clock base, commit, bateria, saúde da bateria, disco por dispositivo, interfaces).

### VALIDAÇÃO DESKTOP REAL

Executada no app Tauri em execução, em um notebook (PC Casa). Valores são **observações daquele momento**, não especificações nem requisitos.

- **CPU:** Intel Core i7-11370H, 8 processadores lógicos, clock efetivo observado ~4,19 GHz com base ~3,3 GHz (o clock efetivo varia a cada amostra).
- **Memória:** commit ~35 GB de ~48 GB; pagefile em uso ~3,1–3,4 GB de ~24 GB, batendo com o uso informado pelo Windows (~3,3 GB).
- **GPUs:** Intel Iris Xe (driver observado 31.0.101.4502) e NVIDIA GeForce GTX 1650 (driver observado 32.0.16.1047), com temperatura de ~60 °C na GTX 1650; a Iris Xe não informa temperatura.
- **Temperaturas indisponíveis nesta máquina (não é erro):** CPU (pacote), placa-mãe e GPU integrada. A interface mostra "Não disponível" numa linha neutra e marca o domínio como **Parcial**. Os SSDs NVMe informam temperatura; o sensor ACPI é exibido sem avaliação, como já era.
- **Discos:** Samsung NVMe e Kingston NVMe, com capacidade, espaço livre, taxa de leitura/escrita, operações por segundo e % ativo **por disco físico**, e a capacidade dos volumes C: e D: separada. **Saúde SMART: Não disponível**; nenhuma saúde de disco é afirmada sem dado real.
- **Rede:** 8 interfaces naquele snapshot (3 desconectadas, recolhidas na tela). Wi-Fi ativo com velocidade de enlace ~574 Mbps no Dashboard (a negociação do Wi-Fi varia: outra leitura marcou ~542 Mbps). Taxas calculadas por delta real por interface; a primeira amostra aparece como calibrando.
- **Bateria:** presente, 100%, tomada conectada. **Saúde da bateria e capacidade de projeto: Não disponível** (o Windows não as entrega sem driver ou elevação); nada foi calculado.
- **Uptime:** da máquina (inicialização do Windows), não do app.
- **Ao vivo:** o horário da última amostra avançou segundo a segundo e "Atualizar agora" força uma amostra imediata.

### Desempenho do coletor (observação desta máquina, não SLA)

Custo médio medido por rodada: carga ~2 ms, E/S + rede + GPU ~16 ms, temperaturas ~14 ms, rodada completa ~108 ms (inclui o ranking de processos, que só roda a cada 2 s com o Dashboard aberto).

### COBERTURA AUTOMATIZADA

- **Rust:** 19 testes em `machine_telemetry` (instâncias e modelo de disco, taxas ausentes viram `None`, interfaces por nome/descrição/IP, primeira amostra sem taxa, tipos de interface, desktop sem bateria, bateria de notebook, vendor/driver de GPU, iGPU + dGPU, disponibilidade por domínio, domínio que falha sem degradar os outros, amostragem real em duas rodadas com núcleos/commit/pagefile/taxas, planos parciais, ausência de PowerShell/subprocessos, nada no workspace portátil) mais o teste de política de refresh atualizado.
- **Frontend (vitest):** 36 testes novos (helpers puros e renderização do Dashboard completo, parcial, sem bateria, sem GPU, primeira amostra e estado de espera); total do projeto: 479.
- **Gates:** `npm test` (479), `npm run lint`, `npm run typecheck`, `npm run build`, `cargo fmt --check`, `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` e `cargo test --workspace` (na repetição completa) passaram.
- **Intermitência registrada:** em uma rodada do workspace, `runtime_snapshot_for_a_ready_project_has_identity_git_and_no_secrets` e `tauri_build_is_a_task_with_an_exit_code_and_never_a_service` falharam por inspeção de processo. Passaram 5/5 isoladas, 3/3 nos binários completos e na repetição do workspace; é a mesma classe do `stacks_exec`, já reproduzida no commit `0336d93`, anterior a esta Session. Esses testes não foram alterados.

### LIMITAÇÕES

1. Sem temperatura de CPU e de placa-mãe: o Windows não as expõe sem driver de terceiros ou elevação, e nada é pedido ao usuário nem elevado automaticamente.
2. Sem saúde SMART dos discos (fica para a fase de saúde do Windows).
3. Sem saúde, capacidade de projeto ou de carga cheia da bateria; só carga, tomada, estado e autonomia estimada.
4. Uso e temperatura de GPU dependem do driver: a GPU integrada desta máquina não informa temperatura.
5. Sem histórico persistido de telemetria; só o buffer curto em memória.
6. IP público não é coletado.
7. Contadores PDH dependem de existirem na máquina; se não existirem, o campo fica `None` e o domínio vira parcial.
8. A bateria é a leitura agregada do sistema (`GetSystemPowerStatus`), não por bateria física.

### Bug corrigido

O campo `swapUsed` herdado do sysinfo **não representava o uso real do pagefile** (ele mede commit além da RAM: ~12,5 GB contra ~3,4 GB reais). A interface passou a usar a medição real via contador PDH do Windows (`Paging File % Usage`), mantendo o commit como memória virtual separada da RAM física.

## Checkpoint — WINDOWS HEALTH & INTEGRITY (Block 07)

Block 07 — Windows Health & Integrity: **IMPLEMENTADO E VALIDADO NO DESKTOP**. SESSION-002: **ATIVA, 7 / 10**, sem bloco atual; próximo: **08 — Network & Security Visibility** (não iniciado). Nada do 08 foi implementado.

### Arquitetura

Coletor **passivo e somente leitura** (`windows_health.rs` + `windows_native.rs`): nada é reparado, reiniciado, instalado, iniciado, parado ou alterado, e SFC, DISM e CHKDSK **não** são executados. O estado fica só na máquina (nada vai ao workspace portátil, ao Git, ao sync, ao Planejamento ou ao DDAE; há teste).

- **Fontes nativas em processo:** registro (leitura), Service Control Manager (só consulta), Configuration Manager/SetupAPI, Event Log API (consultas filtradas) e volumes. **Sem PowerShell, sem WMI e sem subprocessos**; há um teste que falha se o código passar a usar APIs de reparo, instalação, reinício ou escrita.
- **Domínios independentes, cada um com saúde, motivo, fontes e instante próprios:** reinício pendente, Windows Update, serviços essenciais, dispositivos, eventos, volumes. Informativos (fora do estado geral): sistema, confiabilidade e integridade passiva. Falha de um domínio nunca derruba os outros.
- **Semântica:** `healthy`, `attention`, `critical` e `unknown`, só com regra objetiva. **Ausência de informação é `unknown`, nunca `healthy`.** O estado geral é o pior entre os domínios avaliados, com os motivos visíveis e a contagem "X de Y domínios avaliados".
- **Cache por domínio (TTL próprio):** reinício e serviços 45 s, volumes 60 s, eventos 3 min, Windows Update e dispositivos 10 min. "Atualizar agora" força a releitura. A interface consulta de 30 em 30 s com a janela visível; só o que expirou é relido. A leitura marca como "desatualizada" o domínio que passar do dobro do TTL.
- **Permissões:** nada eleva no startup e não há UAC automático. Fonte que exige administrador vira `requires_elevation`, e a interface diz "Requer privilégio administrativo".
- **API:** um único comando de leitura, `windows_health_snapshot(force)`, atrás do gate de máquina cadastrada.

### Regras (objetivas e explicáveis)

- **Reinício pendente:** duas fontes fortes (Component Based Servicing e Windows Update). Uma presente basta para "pendente". Fonte negada ou indisponível **nunca** vira "sem reinício": sem fonte presente e com alguma não consultada, o resultado é desconhecido. Renomeações pendentes de arquivo são informativas e nunca alertam sozinhas.
- **Serviços (lista curta):** Log de Eventos, RPC, WMI, Agendador de Tarefas, Serviços de Criptografia, BITS e Windows Update. Parado só é problema quando o início é automático (crítico para Log de Eventos e RPC). Serviços sob demanda parados são normais; desabilitado é atenção. Início automático atrasado logo após o boot não é falha.
- **Dispositivos:** só os que reportam código de problema no Gerenciador de Dispositivos. Desabilitados de propósito (códigos 22 e 29) não contam. Nenhum dispositivo enumerado é desconhecido.
- **Eventos:** janelas de 24 h e 7 dias, consultas filtradas e com teto, guardando só provedor, ID, nível e instante (a mensagem nunca é lida). Erros e avisos comuns **não** mudam o estado. Crítico: tela azul (bugcheck) nas últimas 24 h. Atenção: bugcheck mais antigo na semana, desligamento inesperado (Kernel-Power 41 e EventLog 6008 do mesmo desligamento contam uma vez), erro de dispositivo de armazenamento, erro de NTFS, avisos de E/S repetidos (5 ou mais) e 3 ou mais falhas de serviço em 24 h. Nunca diagnostica a causa.
- **Windows Update:** estado do serviço, falhas de instalação/download em 7 dias (Event Log do cliente) e as datas de última instalação/verificação quando existirem. Atenção para falha recente ou serviço desabilitado. Não instala nem busca atualizações.
- **Volumes:** sistema de arquivos, somente leitura, bit "sujo" (quando acessível) e verificação de disco agendada para a próxima inicialização (`BootExecute`). Sujo, somente leitura ou verificação agendada são atenção.
- **Integridade passiva:** reúne só sinais que as outras leituras já têm. Sem sinais fica `unknown` e explica que a integridade completa só é comprovada por verificação sob demanda. O modelo de SFC verify, DISM ScanHealth e CHKDSK scan existe, marcado como não disponível, exigindo elevação e reservado ao Block 09.

### VALIDAÇÃO DESKTOP REAL

Executada no app Tauri em execução, em um notebook (PC Casa). Valores são **observações daquele momento**, não requisitos.

- **Windows:** Windows 11 Pro (Professional), versão 25H2, build 26200.9457, x64, ligado há ~1 dia e 13 h (uptime da máquina). O `ProductName` do registro ainda diz "Windows 10"; a build decide "Windows 11".
- **Estado geral: Saudável, 5 de 6 domínios avaliados** (o sexto, Volumes, ficou Desconhecido por exigir administrador).
- **Reinício pendente:** não. Há renomeações de arquivo pendentes, mostradas como informativas, sem alerta.
- **Windows Update:** serviço em execução, início manual/sob demanda; 0 falhas em 7 dias. Última instalação, última verificação e atualizações pendentes: **Desconhecido** (o Windows não mantém mais esse registro nesta versão, o log do cliente não tem eventos de instalação, e contar pendentes exigiria uma busca que o app não dispara).
- **Serviços essenciais:** 7 de 7 saudáveis (Log de Eventos, RPC, WMI, Agendador, Criptografia e BITS em início automático e rodando; Windows Update manual e rodando).
- **Dispositivos:** 192 presentes, nenhum com problema, nenhum desabilitado de propósito.
- **Eventos:** 0 críticos e 5 erros nas últimas 24 h (22 avisos, só informativos); 0 críticos e 25 erros em 7 dias; um único sinal: 1 falha de aplicativo. Nenhum bugcheck, desligamento inesperado, erro de disco/NTFS ou falha de serviço.
- **Volumes C: e D:** NTFS, leitura e escrita. O bit "sujo" **requer privilégio administrativo**; a tela diz isso e o domínio fica Desconhecido, nunca "limpo". Não há verificação de disco agendada.
- **Confiabilidade:** 1 falha de aplicativo e 0 travamentos em 7 dias; sem pontuação (o Monitor de Confiabilidade depende de WMI e não é consultado).
- **Integridade passiva:** sem sinais; estado Desconhecido com a explicação.
- **Fontes não disponíveis (4):** última instalação e última verificação do Windows Update, volume sujo (administrador) e Monitor de Confiabilidade (WMI).
- **Custo (desta máquina, não SLA):** leitura completa forçada de todos os domínios em ~280–340 ms; nas chamadas seguintes só o que expirou é relido.
- **A máquina estava saudável:** nenhum estado Atenção/Crítico foi inventado para provar a interface. Esses estados estão cobertos por testes.

### COBERTURA AUTOMATIZADA

- **Rust (`windows_health`):** 44 testes com fontes falsas (nenhum depende do Event Log, dos serviços ou dos drivers reais): versão do Windows, reinício verdadeiro/falso/parcial/desconhecido, regras de serviço (esperado rodando, parado mas válido, desabilitado, atraso pós-boot, consulta falha), dispositivos (com e sem problema, desabilitado de propósito), filtro de eventos (ruído, bugcheck crítico e antigo, desligamento inesperado deduplicado, erros de armazenamento brandos e duros, NTFS, falhas de serviço, janelas e relógio), Windows Update, interpretação de datas do registro, volumes (limpo, sujo, somente leitura, agendamento, ilegível/negado), integridade passiva, estado geral, isolamento de falhas, cache e staleness por domínio, serialização, passividade (varredura do código-fonte) e nada no workspace portátil. Testes de leitura real, só leitura, validam as fontes nativas.
- **Frontend (vitest):** 40 testes novos (helpers e renderização do painel: saudável, atenção, crítico, desconhecido, reinício, update, eventos, dispositivos, serviços, volumes, dado parcial, requer elevação, estados vazios, última leitura, desatualizado, recolhido por padrão e ausência de qualquer ação de reparo); total do projeto: 519.
- **Gates:** `npm test` (519), `npm run lint`, `npm run typecheck`, `npm run build`, `cargo fmt --check`, `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` e `cargo test --workspace` passaram, inclusive `stacks_exec` (7/7) e `runtime` (25).

### LIMITAÇÕES

1. O bit "sujo" do volume exige privilégio administrativo (o mesmo que o `fsutil dirty query`); sem ele o domínio Volumes fica Desconhecido.
2. Última instalação/verificação do Windows Update dependem de registro ou de eventos que o Windows atual pode não manter; a contagem de atualizações pendentes não é consultada.
3. Sem SMART nem saúde física de disco, e sem pontuação de confiabilidade (WMI).
4. Sem driver/versão por dispositivo: só nome, classe, fabricante e o código de problema.
5. A integridade completa (SFC, DISM, CHKDSK) só existe sob demanda, fora deste Block (Block 09).
6. Firewall, Defender, BitLocker, Secure Boot, TPM e exposição de rede ficam para o Block 08; alertas e recomendações, para o Block 09.
7. Event Log por janela limitada e com teto por consulta; um log muito ruidoso é truncado (a interface avisa).

### Bugs encontrados e corrigidos durante o Block 07

- Avisos de 7 dias apareciam como "0" mesmo sem serem medidos; agora são "não medido" (traço).
- A leitura do bit "sujo" via handle de dispositivo falhava com erro opaco; o caminho correto (volume aberto só para leitura) devolve "requer privilégio administrativo", dito explicitamente.
- Uma edição minha duplicou um trecho do módulo nativo e foi reconstruída antes de qualquer commit.

## Checkpoint — NETWORK & SECURITY VISIBILITY (Block 08)

Block 08 — Network & Security Visibility: **IMPLEMENTADO E VALIDADO NO DESKTOP**. SESSION-002: **ATIVA, 8 / 10**, sem bloco atual; próximo: **09 — Alerts & Diagnostics** (não iniciado). Nada do 09 foi implementado.

### Arquitetura

Coletor **passivo e somente leitura** (`network_security.rs` + `security_native.rs`): nada é ativado, bloqueado, encerrado, escaneado ou alterado (firewall, regras, portas, processos, Defender, BitLocker, DNS, rotas, adaptadores e perfis de rede ficam como estão). O estado fica só na máquina: conexões e endpoints remotos não vão ao workspace portátil, ao Git, ao sync, ao Planejamento, ao DDAE nem ao contexto de IA (há teste).

- **Fontes nativas em processo:** registro (leitura), IP Helper (interfaces, gateways, DNS), Windows Security Center (saúde agregada de antivírus e firewall), TPM Base Services (presença e versão), firmware (UEFI/BIOS) e `netstat` (portas e conexões). **Sem PowerShell, sem WMI, sem subprocessos, sem requisição externa, sem consulta de IP público, sem resolução reversa de DNS e sem varredura de portas**; há um teste que falha se o código passar a usar APIs de alteração, rede externa ou subprocessos.
- **Reuso do Control Plane:** a atribuição Porta → PID → Project usa `control_plane::attribute`; a interface de saída usa a rota que o próprio SO escolhe (sem enviar pacote).
- **Domínios independentes**, cada um com estado, motivo, fontes e instante próprios: firewall, antivírus (com Defender), criptografia (BitLocker), Secure Boot e TPM entram no estado geral; rede, exposição (portas em escuta) e conexões são **informativos**. Falha de um domínio nunca derruba os outros.
- **Semântica:** `healthy`, `attention`, `critical` e `unknown`. **Ausência de informação é `unknown`, nunca `healthy`.** Sem pontuação de segurança.
- **Cache por domínio:** rede e portas 15 s, conexões 10 s, firewall e antivírus 60 s, BitLocker 5 min, Secure Boot e TPM 10 min. "Atualizar agora" força a releitura; a interface consulta de 15 em 15 s com a janela visível.
- **Permissões:** nada eleva no startup e não há UAC automático. Fonte que exige administrador vira `requires_elevation`, e a interface diz "Requer privilégio administrativo".
- **API:** um único comando de leitura, `network_security_snapshot(force)`, atrás do gate de máquina cadastrada.

### Regras (objetivas e sem inventar insegurança)

- **Escuta em `0.0.0.0` ou `::` não é vulnerabilidade nem "exposto à internet":** é descrita como "todas as interfaces", e a interface diz que a alcançabilidade externa depende do firewall e do roteador, que o app não testa. A exposição nunca altera o estado geral.
- **Firewall:** avalia o perfil da rede ativa quando conhecido. Desativado no perfil ativo é crítico, a menos que o Security Center informe outro firewall saudável. Perfis inativos desativados não alertam. Sem a categoria da rede, o estado reflete todos os perfis lidos e a tela diz isso.
- **Antivírus:** Defender passivo ou parado por causa de antivírus de terceiros **não é problema**. Crítico só quando o Defender está inativo, não há antivírus de terceiros e o Security Center não informa antivírus saudável. Assinaturas com mais de 7 dias são atenção (ignoradas com o Defender passivo). Ameaças ativas: **"Não consultado"** na leitura passiva, nunca "zero".
- **BitLocker:** indisponível é `unknown`; suspenso ou desligado no volume do sistema é atenção; volume de dados sem BitLocker é só informação. Chaves de recuperação nunca são lidas.
- **Secure Boot:** desativado é um fato (atenção); BIOS legado e indisponível são `unknown`. **TPM:** presente é saudável; ausente é atenção; indisponível é `unknown`.

### VALIDAÇÃO DESKTOP REAL

Executada no app Tauri em execução, em um notebook (PC Casa). Valores são **observações daquele momento**, não requisitos.

- **Estado geral: Saudável, 4 de 5 verificações avaliadas** (a quinta, BitLocker, ficou Desconhecida por exigir administrador).
- **Rede:** interface ativa Wi-Fi (rota padrão, ~574 Mbps, DHCP), IPv4 privado /24, gateway IPv4, DNS (IPv4 antes de IPv6, sem repetição), IP público **"Não consultado"**. Outras interfaces listadas (VPN, adaptadores virtuais do Hyper-V/WSL, interfaces desconectadas). Categoria da rede: **requer privilégio administrativo** (a chave `NetworkList\Profiles` é negada ao usuário comum), mostrada como Desconhecido, não adivinhada.
- **Exposição:** ~51 portas TCP em escuta (≈25 em todas as interfaces, 5 em interface específica, 21 só na máquina), com processo, PID e executável. A busca por porta atribuiu o Vite do desktop ao Project LKR_Lab. A tela explica que "todas as interfaces" não é exposição à internet.
- **Conexões:** ~86 estabelecidas (≈64 remotas), em visão compacta filtrada por padrão para remotas, com processo e PID, sem DNS reverso.
- **Firewall:** Domínio, Privado e Público ativados; saúde do Security Center "Bom".
- **Antivírus:** Microsoft Defender ativo, nenhum antivírus de terceiros, assinaturas e mecanismo atuais, Security Center "Bom"; proteção em tempo real **Desconhecido** (o valor não existe no registro) e ameaças ativas **Não consultado**.
- **Secure Boot:** ativado (UEFI). **TPM:** presente, versão 2.0. **BitLocker:** Desconhecido, requer administrador.
- **Conferência cruzada (uma vez, só leitura, fora do app):** firewall nos três perfis, Defender com as mesmas versões de assinatura e mecanismo, único produto de antivírus o Defender, rota padrão pelo Wi-Fi com o mesmo gateway, contagens de portas e conexões compatíveis. O Windows classifica o Wi-Fi como Público; o app mostra "Desconhecido" por não conseguir ler essa categoria sem administrador.
- **Fontes não disponíveis (2):** categoria da rede e BitLocker por volume (administrador).
- **Custo (desta máquina, não SLA):** leitura completa forçada em ~520–580 ms.
- **A máquina estava saudável:** nenhum estado Atenção/Crítico foi inventado para provar a interface. Esses estados estão cobertos por testes.

### COBERTURA AUTOMATIZADA

- **Rust (`network_security`):** 61 testes com fontes falsas (nenhum depende de firewall, Defender, TPM ou rede reais): interface ativa pela rota do SO e por métrica, IPv4/IPv6, gateway e DNS (ordem e repetição), categoria da rede, escopos de escuta, atribuição porta → PID → Project, dono não identificado, conexões e filtragem, truncamento com contagem exata, firewall (perfis, perfil ativo, desativado, outro firewall saudável, política de entrada, ilegível, requer elevação), antivírus (Defender ativo, passivo com antivírus de terceiros, serviço parado com terceiros, desativado sem antivírus, tempo real desligado, ameaça, assinaturas antigas, saúde do Security Center), BitLocker (protegido, suspenso, desligado, indisponível), Secure Boot, TPM, estado geral e isolamento de falhas, cache por domínio e relógio, serialização, ausência de segredos, nada no workspace portátil e passividade (varredura do código-fonte). Há um teste ignorado de leitura real (só estados agregados).
- **Frontend (vitest):** 58 testes novos (saudável, atenção, crítico, desconhecido, interface ativa, IP local, gateway, DNS, IP público não consultado, listeners em loopback/interface/todas as interfaces, detalhes de exposição, perfis do firewall, provedor de antivírus, Defender ativo e passivo, BitLocker, Secure Boot, TPM, dado parcial, requer elevação, conexões vazias, recolhido por padrão, última leitura e desatualizado); total do projeto: 577.
- **Gates:** `npm test` (577), `npm run lint`, `npm run typecheck`, `npm run build`, `cargo fmt --check`, `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` e `cargo test --workspace` passaram, inclusive `stacks_exec` (7/7) e `runtime` (25).

### LIMITAÇÕES

1. A categoria da rede (Público/Privado/Domínio) e o estado do BitLocker por volume exigem privilégio administrativo; sem ele ficam Desconhecidos.
2. Proteção em tempo real do Defender depende de um valor de registro que pode não existir; ameaças ativas não são consultadas (exigiriam a API do Defender).
3. O nome de antivírus de terceiros não é exposto (só a contagem de provedores e a saúde agregada), pois exigiria WMI.
4. Somente TCP: UDP não é listado. Alcançabilidade externa não é testada.
5. Sem IP público (nenhuma requisição externa) e sem DNS reverso.
6. Sem remediação, alertas ou recomendações: ficam para o Block 09.

### Bugs encontrados e corrigidos durante o Block 08

- A interface "ativa" escolhida só por métrica/gateway era um adaptador de VPN; agora vale a rota que o SO realmente usa.
- Gateway apresentado como endereço IPv6 link-local e DNS com repetições; agora IPv4 primeiro e sem repetição.
- "Conexões" aparecia como desatualizada entre consultas (TTL de 10 s, consulta de 30 s); consulta de 15 s.
- Endereços IPv6 remotos sem colchetes na lista de conexões.
- Contagem de antivírus de terceiros falhava por um caminho de registro com barras perdidas numa edição minha (detectado na leitura real).


## Critérios de conclusão

Os 10 critérios continuam **não marcados** (0 / 10), de propósito, inclusive depois dos Blocks 06, 07 e 08: nenhum critério é marcado só porque um bloco terminou. Evidência reunida até aqui, para a revisão no fechamento da Session:

- Inventário de processos e mapeamento de portas: demonstrados no desktop e por teste.
- Diferenciar runtime conhecido de processo não associado; UI sem associação falsa com confiança insuficiente: demonstrados (Unknown) e por teste.
- Relação com Project/Worktree por evidência: Project demonstrado no desktop; Worktree só por teste.
- stdout/stderr capturados e Console Hub com logs ao vivo: stdout no desktop; stderr por teste.
- Coleta local e páginas de observação sem ação mutante: cobertos por teste (passividade e workspace portátil).
- "Gates e validação desktop passam": depende da Session inteira (Blocks 09–10).

## Bugs corrigidos nesta Session

- Falsa atribuição de serviço externo a árvore Managed (a árvore engolia irmãos de um hospedeiro comum e herdava confiança Exata).
- Stop duplo emitindo `Stopping` repetido.
- Parser legado da SESSION-001 aceitando um título final incorreto (rodapé `# SESSION-001 FINALIZADA`).
- `useRunLogs` sem `getServerSnapshot`.
- Vazamento de processos Node nos testes quando uma asserção falhava.
- Timestamps ausentes na visualização do console.
- `swapUsed` do sysinfo apresentado como uso de pagefile (era commit além da RAM); substituído pelo contador real do Windows (Block 06).
- Avisos de 7 dias exibidos como "0" sem serem medidos (Block 07); agora aparecem como "não medido".
- Interface ativa escolhida só por métrica (adaptador de VPN), gateway IPv6 e DNS repetido, "Conexões" desatualizada entre consultas e IPv6 sem colchetes (Block 08).

## Próximo bloco

**09 — Alerts & Diagnostics** (não iniciado). Um agente privilegiado só será avaliado se uma informação concreta o exigir (Blocks 07–08).
