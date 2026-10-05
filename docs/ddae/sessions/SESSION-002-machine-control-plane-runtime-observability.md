# SESSION-002 — Machine Control Plane & Runtime Observability

**PT:** Plano de controle local da máquina e observabilidade de runtimes
**Tipo:** Feature
**Status:** COMPLETED / FINALIZADA
**Branch:** `feat/session-002-machine-control-plane` (checkout principal; nenhuma Managed Worktree)
**Planning Item:** Machine Control Plane & Runtime Observability (CONCLUÍDO, derivado da Session finalizada)

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
| 09 | Alerts & Diagnostics | IMPLEMENTADO E VALIDADO NO DESKTOP |
| 10 | Validation & Hardening | CONCLUÍDO E VALIDADO NO DESKTOP |

Progresso no DDAE: **10 / 10 blocos** e **10 / 10 critérios**, sem bloco em andamento e sem próximo bloco. A SESSION-002 foi **FINALIZADA** (`completed`, terminal) e o Planning Item aparece **CONCLUÍDO** por derivação; ver o fechamento formal ao final. A branch foi integrada na `main` por fast-forward.

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


## Checkpoint — ALERTS & DIAGNOSTICS (Block 09)

Block 09 — Alerts & Diagnostics: **IMPLEMENTADO E VALIDADO NO DESKTOP**. SESSION-002: **ATIVA, 9 / 10**, sem bloco atual; próximo: **10 — Validation & Hardening** (não iniciado). Nada do 10 foi implementado.

### Arquitetura

**Deterministic Diagnostic Engine** (`diagnostics.rs`): `OBSERVATIONS → FACTS → RULES → FINDINGS → ALERTS → DIAGNOSTICS`. Não há IA, pontuação, remediation nem heurística opaca decidindo estado; cada Finding sai de uma regra pura com evidência.

- **Consome, não duplica:** Machine Telemetry (incluindo as regras sustentadas de `health.rs`), Windows Health, Network & Security, Control Plane e Runtime Supervisor, sempre pelos snapshots que os collectors já têm (cada um com o próprio TTL). Abrir a tela não relê nada além do que expirou.
- **Distinções:** *signal* (fato bruto), *finding* (interpretação determinística), *alert* (finding persistido com ciclo de vida), *diagnostic* (ação explícita para obter mais evidência) e *remediation* (não existe).
- **Severidade** `info | attention | critical`; `unknown` não é severidade e nunca vira alerta. **Confiança** `high | medium | low`; todo crítico exige confiança alta.
- **Explicabilidade:** id, rule_id, título, resumo, severidade, confiança, domínio, fonte, recurso, evidência (rótulo, valor e fonte), motivo, próximo passo, diagnóstico opcional e CTA de navegação.
- **Fingerprint estável:** `rule_id@recurso` (ex.: `machine.disk.low_space@C:`). Portas de um listener não entram no fingerprint (sobem e descem).
- **Fonte avaliada, velha ou sem dado:** só uma fonte **avaliada agora** pode resolver um alerta; fonte velha (mais de 2× o TTL), indisponível ou que exige administrador não gera alerta novo, não resolve o existente e não escala.
- **Execução:** avaliação barata sob demanda (`alerts_snapshot`) e uma avaliação periódica de 60 s numa thread própria (sem busy loop, só depois do cadastro da máquina, sem serviço do Windows).

### Ciclo de vida local (hub.db, migração 011)

Tabelas `machine_alerts` e `machine_diagnostic_runs`: estado **da máquina**, fora do workspace portátil, do sync, do Git, do Planejamento e do DDAE (há testes de varredura de código).

- **Ativo → Reconhecido → Resolvido.** Reconhecer só marca o alerta como visto (não altera a máquina nem resolve). Reconhecido que **piora** volta a Ativo.
- **Resolver:** só depois de 90 s sem ser visto por uma fonte avaliada (atraso contra leituras que piscam). **Reabrir:** nova ocorrência (`occurrence_count` + 1), com a anterior preservada; no máximo uma ocorrência aberta por fingerprint (índice único).
- Persistidos: first_seen, last_seen, acknowledged_at, resolved_at, occurrence_count e observations. Histórico resolvido visível por 7 dias e podado depois de 30 dias (ou 500 linhas).

### Regras (IDs estáveis)

- **Máquina:** `machine.disk.low_space`, `machine.cpu.sustained_pressure`, `machine.memory.sustained_pressure`, `machine.thermal.over_limit`.
- **Windows:** `windows.reboot.pending`, `windows.service.not_running`, `windows.device.problem`, `windows.event.bugcheck`, `windows.event.unexpected_shutdown`, `windows.event.storage_error`, `windows.event.filesystem_error`, `windows.event.service_failures`, `windows.update.repeated_failure`, `windows.volume.problem`.
- **Segurança:** `security.firewall.active_profile_disabled`, `security.no_active_antivirus`, `security.threat.active`, `security.defender.signatures_stale`.
- **Rede:** `network.listener.all_interfaces` (**INFO**, nunca atenção).
- **Runtime:** `runtime.managed.failed`, `runtime.managed.repeated_failure`, `runtime.port.collision`.

Valores adotados (documentados e testados): **disco** atenção com menos de 10% **e** menos de 20 GiB livres, crítico com menos de 5% **e** menos de 5 GiB (os dois limites precisam ser cruzados: 2 TB com 9% tem 180 GiB e não alerta; um volume de 8 GiB nunca teria 20 GiB), volumes com menos de 1 GiB não são avaliados e a histerese segura o alerta até 1% e 1 GiB de folga. **CPU** 90% por 60 s (atenção) e 95% por 180 s (crítico), por amostras ininterruptas: pico isolado nunca alerta. **Memória** 90% por 120 s e 95% por 180 s, e **crítico só com o commit (RAM + pagefile) também em 90% ou mais** (RAM inclui cache); sem a medida de commit, nunca crítico. **Temperatura** só com limite declarado pelo próprio dispositivo (ou o limite conhecido da GPU): sem limite, nenhum crítico. Falhas repetidas de runtime: 3 ou mais da mesma ação em 15 minutos.

Semântica que **não** gera alerta: BitLocker ou categoria da rede desconhecidos por exigirem administrador; Defender passivo com antivírus de terceiros; firewall desativado quando o Security Center informa outro firewall saudável; `0.0.0.0`/`::` (é INFO, e o texto diz que isso não significa exposição à internet); SMART e bateria (sem fonte confiável); Session congelada, Worktree parada e Planning pendente (workflow não é problema); qualquer erro comum do Event Log.

**Correlação determinística:** falha de runtime gerenciado + colisão de porta declarada → o Finding de falha mostra a porta, o processo dono e o PID e diz que a falha "provavelmente foi causada por porta ocupada"; a colisão só existe se o Project tem execução ativa (ou que falhou há pouco) e o dono da porta está fora da árvore gerenciada dele. Nunca se mata o processo.

### Diagnostic Runner

- **Allowlist estrita:** `sfc_verifyonly`, `dism_checkhealth`, `dism_scanhealth`, `chkdsk_scan`. Cada id é um comando e argumentos **fixos** (executável do System32 por caminho absoluto), sem shell, sem argumentos livres; o único parâmetro é a letra do volume, validada (`C:`). `sfc /scannow`, `DISM /RestoreHealth`, `chkdsk /f`, `/r` e qualquer reparo **não existem**.
- **Elevação:** todos exigem administrador. O LKR LAB **não pede UAC nem cria processo privilegiado**: sem elevação o diagnóstico fica "Requer administrador" e não é executado.
- **Execução:** só por ação do usuário, um por vez, com PID, stdout, stderr (decodificados de UTF-16 ou da página OEM), início, fim, código de saída e cancelamento (encerra só o processo do diagnóstico). Não é um Project Runtime.
- **Resultado:** parsers próprios, em inglês e português, por ferramenta (`clean`, `problems_found`, `inconclusive`, `failed`, `cancelled`); o código de saída sozinho nunca decide sucesso.
- **Histórico local:** resumo, código e uma cauda curta da saída (30 linhas, 4 KB), com no máximo 50 execuções; a saída é local da máquina, não vai ao workspace portátil, ao sync nem ao contexto de IA.

### VALIDAÇÃO DESKTOP REAL

Executada no app Tauri em execução, em um notebook (PC Casa). Valores são **observações daquele momento**.

- **Faixa e painel** no Dashboard: faixa "Alertas · 0 críticos · 0 atenção · 3 informações · Ver diagnósticos" e painel "Alertas e diagnósticos" com **15 de 15 fontes avaliadas**.
- **Nenhum problema foi inventado.** Os 3 achados reais foram **Informação factual**: Spotify.exe, SpotifyLauncher.exe e vmms.exe escutando em todas as interfaces (com as portas na evidência, o texto "não significa exposição à internet" e o CTA "Abrir Network & Security").
- **Ciclo de vida real:** o alerta de memória (Atenção) abriu, **resolveu e reabriu como ocorrência 2**, e voltou a resolver; no painel aparecia como "1 resolvido recentemente". "Reconhecer" foi usado num Info (vmms.exe): o estado passou a "Reconhecido" e o botão sumiu, sem alterar a máquina. As 3 observações persistem no `hub.db` (69 avaliações no mesmo alerta, sem duplicar).
- **Diagnósticos:** os 4 do catálogo aparecem como **"Requer administrador"**, com a explicação de que o app não solicita elevação; histórico vazio. **Nenhum diagnóstico real foi executado** (o app não está elevado e não houve autorização para executar nesta etapa).
- **Não validado no desktop (cobertos só por teste automatizado):** falha de runtime gerenciado, repetição de falhas, colisão de porta, estados Atenção/Crítico de disco, Windows e segurança, e a execução e o cancelamento de um diagnóstico. Nenhuma falha foi provocada na máquina real para demonstrar a interface.

### COBERTURA AUTOMATIZADA

- **Rust:** 64 testes do motor (máquina saudável sem findings, Unknown não vira alerta, disco atenção e crítico e volumes pequenos e histerese, pico de CPU sem alerta, CPU e memória sustentadas, memória transitória sem alerta, temperatura, reinício pendente, serviço crítico parado, dispositivo com problema, bugcheck, desligamento inesperado, ruído do Event Log, falhas de update, volume com problema e diagnóstico sugerido, firewall do perfil ativo, nenhum antivírus, antivírus de terceiros válido, ameaça, assinaturas, listener em todas as interfaces como INFO, runtime falhou, loop de falhas, colisão de porta, correlação, dado velho, dedup, ocorrências, resolver, reabrir, reconhecer, escalada, ordenação, privacidade, passividade e nada no workspace portátil) e 28 do runner e do armazenamento (allowlist, id desconhecido rejeitado, sem argumentos livres, sem injeção, elevação, execução real de um comando inofensivo injetado com stdout e stderr e código de saída, um por vez, cancelamento, parsers em inglês e português, decodificação UTF-16 e OEM, histórico limitado e ciclo de vida completo em SQLite real).
- **Frontend (vitest):** 54 testes novos (estado vazio, crítico, atenção, info, reconhecido, resolvido, filtros por estado e severidade e domínio, ordenação, evidência, próximo passo, CTA de navegação, diagnóstico indisponível, requer administrador, em execução, concluído, problemas encontrados, falha, cancelado e histórico, fontes não avaliadas e a faixa do Dashboard); total do projeto: 631.
- **Gates:** `npm test` (631), `npm run lint`, `npm run typecheck`, `npm run build`, `cargo fmt --check`, `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` e `cargo test --workspace` (sem nenhuma falha) passaram, inclusive `stacks_exec` (7/7) e `runtime` (25).

### LIMITAÇÕES

1. Nenhum diagnóstico real foi executado: exigem administrador e o app não eleva sozinho (decisão de segurança); a execução, o cancelamento e os parsers estão cobertos por testes com comandos inofensivos injetados e textos de exemplo.
2. Não há regra para "execução gerenciada que desapareceu" nem para "processo associado a Project com condição inconsistente": o supervisor registra a saída como Failed, Stopped ou Completed, sem um sinal separado de desaparecimento.
3. Ameaças ativas do Defender são "não consultadas" na leitura passiva; a regra existe, mas só dispara quando houver fonte para o dado.
4. Temperatura só alerta com limite conhecido; sem SMART, sem saúde de bateria e sem pontuação, nada disso gera alerta.
5. A avaliação periódica só ocorre com o app aberto (sem serviço do Windows); enquanto aberto, os collectors releem o que expirou (por exemplo o Event Log a cada 3 minutos) mesmo sem a tela de alertas aberta.
6. A revisão formal dos 10 critérios de conclusão fica para o Block 10.

### Bugs encontrados e corrigidos durante o Block 09

- A regra de disco era só porcentagem (2 TB com 9% alertava); agora cruza porcentagem e valor absoluto, com volumes pequenos tratados à parte e histerese.
- A janela de memória de 30 s e 60 s fazia o alerta abrir e fechar em minutos numa máquina que gira em torno de 90% (visto no uso real, ocorrência 2); agora 120 s e 180 s.
- Conflito de CSS com a classe `.mh-alert` do card de saúde do Block 06 (selos e botões esticados); os cartões ganharam classe própria.
- Testes de migração antigos assumiam `user_version` 10 e re-aplicavam a migração sobre tabelas já criadas; a migração 011 ficou idempotente e as expectativas foram atualizadas para 11.

## VALIDATION & HARDENING (Block 10)

Block 10 — Validation & Hardening: **CONCLUÍDO**. SESSION-002: **ATIVA, 10 / 10 blocos, 10 / 10 critérios**, sem bloco atual e sem próximo bloco. A Session **não foi finalizada** (decisão do usuário, depois da revisão) e `main` não foi mesclada.

O Block 10 não adiciona feature: cada item abaixo foi provado por teste, reprodução, auditoria ou medição antes de qualquer mudança de código. Valores são **observações daquele momento**, não requisitos.

### Banco de dados e migrações

- **Schema:** `user_version = 11`. Migrações 001 a 011 em transação, cada uma aplicada por inteiro ou nada.
- **Provado por teste (banco isolado):** banco novo chega à versão 11 com todas as tabelas; **v10 → v11** cria as tabelas de alerta e **preserva** Project, SESSION-001, Planning e Worktrees; reabrir um banco já em v11 é idempotente e preserva os alertas; uma migração que falha (tabela incompatível pré-existente) faz **rollback atômico**: a versão continua em 10, a segunda tabela da 011 não é criada e o erro é explícito; banco criado por um build **mais recente** é recusado ("mais recente") sem alterar o schema. Um build antigo (que só conhece até a sua versão) recusa o banco v11 da mesma forma, sem corromper nada.
- **Concorrência SQLite:** WAL, `busy_timeout` de 5 s e chaves estrangeiras ligadas. 4 conexões simultâneas × 40 rodadas (gravação de alertas, histórico de diagnósticos e leituras) terminaram sem nenhum "database is locked".

### Instância única (problema provado, corrigido)

- **Problema provado:** numa etapa anterior uma ferramenta chegou a abrir uma segunda instância sobre o mesmo `hub.db` (PID 42108).
- **Correção:** `tauri-plugin-single-instance` como primeiro plugin, antes do `setup`. A segunda execução **não chega a abrir o banco**: encerra com código 0 e a janela existente é restaurada, exibida e focada.
- **Validado no app real:** segunda execução saiu com código 0, continuou **uma instância**, e uma janela **minimizada** foi restaurada e ficou em primeiro plano.

### Restart, recuperação e reconciliação

- **Alertas:** o ciclo de vida sobrevive a restart (ativo, reconhecido com o instante, resolvido e o histórico), a deduplicação continua, o fingerprint é estável e a contagem de ocorrências **não zera** (reabrir depois de reiniciar é a ocorrência 2). Testado com reabertura real do arquivo e confirmado no uso real: o app foi relançado várias vezes durante os Blocks 08 a 10 e Projects, Sessions, Planning, Worktrees e alertas permaneceram.
- **Diagnósticos:** só o **término** é gravado (resumo, código e cauda curta); a tabela não tem coluna de estado "em execução" nem de PID. Um app que cai no meio de um diagnóstico **não deixa nada eternamente RUNNING**.
- **Managed Runtime:** o estado do supervisor fica só na memória e a árvore gerenciada vive num Job Object `KILL_ON_JOB_CLOSE`: ao fechar o app (ou ele cair) o SO encerra a árvore, e um teste confirma que descartar o supervisor não deixa órfãos. Por isso, depois de um restart, **não existe execução RUNNING persistida** para reconciliar; um processo que continue vivo por outro caminho aparece como **Descoberto** (externo), nunca como gerenciado. Nenhum processo externo é encerrado.

### Vazamentos, limpeza e buffers

- **Processos e handles:** 12 ciclos de iniciar, observar (Control Plane) e parar uma execução gerenciada deixaram **zero processos órfãos**. O crescimento de handles foi investigado: o `sysinfo` mantém **um handle por processo vivo da máquina** (um platô do tamanho da lista de processos, ~380 neste PC, que aparece na primeira leitura do inventário e não cresce por ciclo) e cada execução finalizada retida (no máximo 10) segura **1 handle** do Job Object; ao descartar o supervisor os handles voltam ao nível anterior. O teste mede o crescimento por ciclo depois do aquecimento e passou 3 de 3.
- **Listeners e timers (frontend):** o listener de eventos de execução é registrado **uma única vez** mesmo com 25 montagens; todo `setInterval` de estado e de tela tem o `clearInterval` correspondente; os polls de fundo só rodam com a janela visível; os listeners de documento e janela são removidos na limpeza; o console remonta por execução (sem herdar estado).
- **Ring buffer do console:** limites de 10 000 linhas e 8 MB, mantendo as mais novas (testes existentes passando; processo ruidoso não bloqueia observação nem parada).
- **Históricos:** alertas resolvidos limitados a 500 linhas e a 30 dias (visíveis por 7 dias) e **alertas abertos nunca são podados**; diagnósticos limitados a 50 execuções, com cauda de 30 linhas e 4 KB (nunca a saída inteira).

### Desempenho (6 minutos, máquina real)

Soma da árvore do app (processo principal + WebView2), 8 processadores lógicos:

| Fase | CPU | RAM | Handles | Threads |
|---|---|---|---|---|
| Dashboard visível (amostragem ativa) | ~4,0% (máx. 5,5%) | ~610 MB | 900 → 907 | 20–23 |
| Janela minimizada | ~1,0% (máx. 1,7%) | ~608 MB | 907 → 890 | 20–21 |
| Janela restaurada | ~1,0% | ~606 MB | 884 → 892 | 19–20 |

Sem crescimento de RAM, handles ou threads; minimizar reduz o custo (a política de visibilidade funciona). Não é um benchmark científico: serve para detectar regressão grosseira.

### Isolamento de falhas e ausência de panic

- Uma fonte que falha não derruba as outras: coletores com fontes falsas (Blocks 06 a 09), avaliação com fatos parciais, falha de gravação de alerta (devolve erro, sem panic, e o resto do banco continua funcional).
- **Auditoria de panic:** `0` ocorrências de `unwrap()`/`expect()` nos módulos novos de runtime (`diagnostics`, `diagnostic_runner`, `alert_store`, `security_native`, `network_security`, `windows_health`, `windows_native`, `control_plane`, `supervisor`, `telemetry`, `sensors`, `machine`) e em `main.rs`.

### Segurança e privilégio

- **Diagnostic Runner:** allowlist fechada (4 ids), sem shell, sem comando ou argumento livre, volume validado como uma letra (`C:`); injeção por texto de shell, caminho ou flags é rejeitada; um teste de varredura impede assinaturas públicas com superfície livre.
- **Bug provado e corrigido (redaction):** uma linha de comando como `curl -H "Authorization: Bearer <token>"` **vazava o token**. Agora cabeçalhos HTTP sensíveis (Authorization, X-Api-Key, Cookie), inclusive com padding base64 e com o valor no argumento seguinte, são mascarados; cabeçalhos comuns e caminhos do Windows não são alterados.
- **Segredos:** varredura dos módulos da Session não encontra campos sugerindo senha, token, chave de recuperação, segredo de Wi-Fi ou credenciais; o console pode conter segredos gerados pelo processo, mas é local da máquina (fora do workspace portátil, do Git, do sync e do contexto de IA).
- **Privilégio:** varredura prova que nenhum módulo (nem o app Tauri nem o manifesto) pede elevação por conta própria. Bloco 06: sensor indisponível fica indisponível; Block 07: volume sujo "requer privilégio administrativo"; Block 08: BitLocker e categoria da rede ficam Desconhecidos; Block 09: diagnósticos "requerem administrador". Nenhum dispara UAC.

### Auditoria de falsos positivos

Limites do disco testados no valor exato (estritamente menor; só um dos dois limites nunca alerta; histerese), listener em `0.0.0.0` é **INFO** e nunca "exposto à internet", Unknown nunca vira alerta, Defender passivo com antivírus de terceiros não é problema, ruído do Event Log não é crítico, estados de workflow (Session congelada, Worktree parada, Planning pendente) não são alerta, e uma execução **parada** pelo usuário não é falha. Nenhum limite foi alterado sem evidência.

### Atribuição adversarial e Worktree real

- **Cenário real e isolado:** repositório git temporário com `git worktree add` real, quatro processos reais (cwd no Worktree, no checkout principal, numa pasta com o mesmo prefixo e numa pasta fora), e depois limpeza total (processos encerrados, Worktree removido). Resultado: cwd no Worktree atribui **Project + Worktree + Session + bloco** com confiança Alta; o checkout principal atribui só o **Project** (sem Worktree nem Session); a pasta com prefixo parecido e a pasta de fora ficam **Unknown**, sem associação nenhuma.
- A regressão do Block 03/05 segue coberta: um serviço externo **não herda Exata** só por dividir um ancestral com um runtime gerenciado.

### Captura real de stdout e stderr

Subprocesso controlado (Node) que escreve `OUT-1`, `ERR-1`, `OUT-2` e `ERR-2`: os fluxos chegam **separados**, **na ordem** em que foram escritos, com sequência estritamente crescente, sem repetir linhas ao reler e sem processo restante; com saída 7, a execução fica Failed com código 7 e o último stderr preservado.

### Console, UX e rotas

- **Desktop real:** execução gerenciada `lab` (porta 4317) iniciada e parada na tela: atribuição **Exata**, console ao vivo com horário por linha, filtros ALL/STDOUT/STDERR (STDERR vazio mostra "Nenhuma linha corresponde ao filtro"), busca ("bridge" encontrou as 2 linhas), pausa e retomada, aviso "Console local". Ao parar, a porta foi liberada, nenhum processo ficou e o Vite externo (PID 25904) **não foi tocado**; o console do Vite continua "não disponível — processo iniciado fora do LKR LAB".
- **Walkthrough:** Dashboard (Telemetry, Windows Health, Network & Security, Alerts & Diagnostics), Projetos, Visão geral do Project, DDAE, Detalhe da Session, Worktrees (2 Worktrees), Planejamento e Runtime com Control Plane e Console. **Rotas:** recarga direta (F5) na rota do Runtime preservou a tela, e voltar e avançar do histórico funcionaram.
- Estados vazio, carregando, parcial, requer elevação, desatualizado e erro têm texto próprio nos painéis (testes de renderização); nenhuma tela ficou em branco.

### Revisão formal dos 10 critérios

| # | Critério | Teste automatizado | Desktop | Status |
|---|---|---|---|---|
| 1 | Processos relevantes podem ser inventariados | `live_inventory_maps_own_pid_parent_and_listening_port`, `own_process_is_flagged_and_inventory_is_relevance_filtered` | Control Plane: 532 processos, serviços detectados | Concluído |
| 2 | Portas listening mapeadas para processos | `live_inventory_maps_own_pid_parent_and_listening_port`, exposição com PID (Block 08) | 52 portas, porta 1420 → Vite PID 25904, 4317 → `lab` | Concluído |
| 3 | Runtime conhecido ≠ processo não associado | `unassociated_listener_is_unknown_and_system_is_categorized`, `bare_node_exe_stays_unknown_even_with_an_active_project` | "Descoberto" vs "Gerenciado"; 17 serviços sem relação; "157 processos protegidos não podem ser associados" | Concluído |
| 4 | Runtime relacionado a Project/Worktree com evidência confiável | `cwd_inside_a_worktree_attributes_worktree_and_session`, Worktree git real (hardening) | Project: Vite → LKR_Lab e `lab` Exata; **Worktree só por teste automatizado** (nenhum processo em Worktree gerenciado) | Concluído |
| 5 | stdout/stderr dos processos iniciados pelo LKR LAB capturados | `stdout_and_stderr_are_captured_separately_in_order...`, `a_failing_controlled_process_keeps_its_exit_code_and_last_stderr`, `service_stays_alive_streams_both_outputs_and_stops_cleanly` | stdout do `lab`; **stderr por teste controlado** (o `lab` não escreve em stderr) | Concluído |
| 6 | Console Hub exibe logs ao vivo | testes de eventos de saída e do console (filtros, pausa, limpeza, teto de 1000 linhas) | Console ao vivo do `lab` com filtros, busca e pausa | Concluído |
| 7 | UI sem associação falsa com confiança insuficiente | `external_service_never_inherits_exact_from_managed_siblings_of_a_shared_host`, `project_without_a_local_folder_never_owns_anything`, prefixos parecidos ficam Unknown | Vite externo "Alta" (nunca Exata), sem console; "Processo não identificado" | Concluído |
| 8 | Coleta local por padrão | `control_plane_state_never_enters_the_portable_workspace`, `console_is_local_only...`, testes de "nada no workspace portátil" dos Blocks 06 a 09 | Painéis rotulados "Somente local" e "Console local — não é sincronizado" | Concluído |
| 9 | Abrir páginas de observabilidade não executa ação mutante | `observing_never_mutates_anything`, varreduras de passividade (Windows Health, Network & Security, diagnósticos), `nothing_in_the_app_can_request_elevation_by_itself` | Todas as páginas abertas no walkthrough sem nenhuma ação executada; diagnósticos só por clique e só com administrador | Concluído |
| 10 | Gates e validação desktop passam | npm 636 testes, `cargo test --workspace` 604 passando e 0 falhas, lint, typecheck, build, fmt, check e clippy | Walkthrough completo, instância única e desempenho | Concluído |

Os 10 critérios foram marcados pela UI real do DDAE (10 / 10). A contagem só foi alterada depois da evidência acima.

### Gates e estabilidade

- `npm test` (49 arquivos, 636 testes), `npm run lint`, `npm run typecheck`, `npm run build`, `cargo fmt --check`, `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` e `cargo test --workspace --no-fail-fast` (**604 passando, 0 falhas, 6 ignorados**) passaram.
- **Repetições:** control_plane ×5, runtime ×5, diagnostics ×5, diagnostic_runner ×5, windows_health ×3, network_security ×3, migrações (hardening, planning, ddae, machine, worktrees, portable) ×3, hardening_leaks ×3 e hardening_streams ×3: **todas passaram em todas as repetições**.
- **stacks_exec (flake histórico):** falhou intermitentemente em passagens anteriores (no Block 09 e no início deste), sempre alternando entre testes do mesmo arquivo com `runtime::inspect` vendo `Running`; rodado isolado passou 5 de 5 na janela final. Registrado, sem alterar o teste (sem reprodução determinística).

### Eligible for finalize e ReadyForAI

- **eligible_for_finalize: sim.** `canComplete = true` e nenhum impedimento: 10 / 10 blocos concluídos e 10 / 10 critérios concluídos. **Não foi executado** (o botão "Finalizar" não foi usado).
- **ReadyForAI: não** (`incomplete`, falta `actionable_block`): é a regra real e independente da finalização; sem bloco pendente ou em andamento, não há bloco acionável para um agente continuar.

### LIMITAÇÕES REMANESCENTES

1. A avaliação de alertas e o monitoramento só ocorrem **enquanto o app está aberto**; não há Windows Service nem agente privilegiado.
2. Os diagnósticos privilegiados (SFC, DISM e CHKDSK) **nunca foram executados de verdade** (exigem administrador e o app não eleva sozinho); só a capability, os parsers e a execução com comandos inofensivos injetados são testados.
3. Fontes do Windows indisponíveis sem administrador continuam Desconhecidas: categoria da rede, BitLocker por volume, bit "sujo" do volume, última instalação e verificação do Windows Update e temperatura do pacote da CPU.
4. Sem SMART (saúde física dos discos) e sem saúde de bateria; sem pontuação.
5. Ameaças ativas do Defender e o nome do antivírus de terceiros não são consultados (exigiriam WMI ou a API do Defender).
6. Worktree tem evidência por teste automatizado (cenário git real); não há processo em Worktree gerenciado no desktop para demonstrar ao vivo.
7. Não há regra para "execução gerenciada que desapareceu" nem para "processo com condição inconsistente" (o supervisor registra a saída como Failed, Stopped ou Completed).
8. Só TCP é listado; alcançabilidade externa e IP público não são testados (nenhuma consulta externa).
9. `stacks_exec` segue com instabilidade intermitente histórica (disputa de processos `cargo` no mesmo arquivo de testes).
10. Uma queda abrupta do app encerra a árvore gerenciada (Job Object): execuções gerenciadas não sobrevivem ao app, por decisão de segurança.

### Bugs encontrados e corrigidos durante o Block 10

- **Redaction vazava `Authorization: Bearer …`** em linhas de comando (provado por teste; corrigido).
- **Instância duplicada sobre o mesmo `hub.db`** (observada numa etapa anterior; corrigida com o guard de instância única e validada).
- O teste de vazamento acusou crescimento de handles que se mostrou ser o platô do inventário do `sysinfo` e a retenção limitada de 10 execuções, não um vazamento por ciclo (investigado com sonda temporária, removida); o teste ficou com baseline aquecido.
- Um erro de tipagem no teste de listeners do frontend (tipos do Node) foi pego pelo `typecheck` e corrigido com imports `?raw`.

## Critérios de conclusão

Os 10 critérios foram **formalmente revisados no Block 10** e marcados como concluídos (**10 / 10**) pela UI real do DDAE, cada um com evidência automatizada e/ou de desktop (tabela em "VALIDATION & HARDENING"). Eles ficaram em 0 / 10 até o Block 09 de propósito: nenhum critério era marcado só porque um bloco terminou.

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
- Regra de disco só por porcentagem, alerta de memória que piscava (janelas curtas), conflito de CSS e migração não idempotente (Block 09).
- Redaction de linha de comando vazava `Authorization: Bearer …` (Block 10).
- Segunda instância do app podia abrir o mesmo `hub.db` (Block 10; guard de instância única).

## Próximo bloco

Nenhum: os 10 blocos estão concluídos e a Session está finalizada. Nenhuma Session nova nem Planning Item novo foi criado por este fechamento.

## Checkpoint — SESSION-002 FINALIZADA

# SESSION-002 FINALIZADA

- **Status:** FINALIZADA (`completed`, terminal) em 05/10/2026 (`completed_at` = `2026-10-05T06:32:48.180Z`).
- **Blocks:** 10 / 10 · **Completion criteria:** 10 / 10 · **Roadmap 01–10: COMPLETO.** Sem bloco atual e sem próximo.
- **Integração:** `feat/session-002-machine-control-plane` (`5fbf7bd`) foi integrada na `main` por **fast-forward** (`0336d93` → `5fbf7bd`), sem merge commit e sem force push. `main` == `origin/main` (0 / 0) no momento da integração. Smoke gates na `main`: `npm run typecheck`, `npm run build` e `cargo check --workspace` passaram. A branch da feature e as demais branches históricas foram **preservadas**.
- **Ação de finalização:** lifecycle canônico do DDAE (`ddae_complete`), explícita e autorizada pelo usuário, executada pela API real do backend (`hub-core`) sobre o `hub.db` real; sem SQL direto, sem alterar Blocks, critérios, objetivo ou resultado desejado. `result` **vazio** (a API permite; nenhum texto foi inventado). Backup do banco feito antes da ação.
- **Terminal confirmado:** retomar, congelar, parar e finalizar de novo foram recusados ("Sessão finalizada é terminal e não pode ser alterada.").
- **Eventos:** o histórico passou a ter `SESSION_COMPLETED` (1), além dos eventos anteriores, preservados (53 no total).
- **Planning:** `planning_item_id` da SESSION-002 permanece o mesmo; nenhum item foi criado, duplicado, cancelado ou restaurado e o `stored_status` continua `open`. A fase **CONCLUÍDO** é derivada (Planning Item → Session vinculada finalizada → fase concluída). Contagens do Planning: 1 concluído, 0 planejados, 0 em execução.
- **DDAE / Project Control Center (pelo modelo):** SESSION-002 FINALIZADA, 10 / 10, sem atual nem próximo; 2 Sessions, 2 finalizadas, nenhuma ativa.
- **Próxima ação (regras existentes):** sem Session ativa e sem item planejado, nenhuma regra de trabalho dispara; permanece apenas a regra de contexto (`context_generated = false` neste projeto no momento da finalização, portanto "Gerar contexto"). Nada novo foi inventado.
- **Ready for AI no momento da finalização:**
  - **antes:** `false` (`incomplete`, falta `actionable_block`: não há bloco pendente ou em andamento após 10 / 10);
  - **depois:** `ready = false`, estado de contexto `available` (Session finalizada permite gerar contexto), nada faltando.
  - Isso **não** é erro da finalização: finalizar depende do lifecycle (`canComplete = true`), não de Ready for AI; são regras independentes. Nenhum campo foi alterado para forçar `true`.

### Entregue

- **Machine Control Plane** (Blocks 01–04): arquitetura, descoberta de processos e portas, atribuição de runtime com evidência e Managed Runtime Supervisor.
- **Runtime observability e Console Hub** (Block 05): consoles ao vivo com captura de stdout/stderr.
- **Machine Telemetry** (Block 06) e **Windows Health** (Block 07).
- **Network & Security** (Block 08).
- **Alerts & Diagnostics** (Block 09): alertas determinísticos com ciclo de vida persistido e Diagnostic Runner.
- **Validation & Hardening** (Block 10): migrações, instância única, vazamentos, streams, privilégio, listeners e revisão formal dos 10 critérios.

### Limitações preservadas (nada foi escondido)

1. O monitoramento e a avaliação de alertas só ocorrem **enquanto o app está aberto**; não há Windows Service nem Agent privilegiado.
2. Diagnósticos privilegiados (SFC, DISM, CHKDSK) **nunca foram executados de verdade** (exigem administrador).
3. Algumas fontes do Windows exigem administrador e ficam Desconhecidas: categoria da rede, BitLocker por volume, bit "sujo" do volume, última instalação e verificação do Windows Update e temperatura do pacote da CPU.
4. Sem SMART e sem saúde avançada da bateria; sem pontuação.
5. Ameaças ativas do Defender e o nome do antivírus de terceiros **não** são consultados.
6. Worktree tem evidência por teste automatizado; não há processo em Worktree gerenciado no desktop para demonstrar ao vivo.
7. Sem regra para "execução gerenciada que desapareceu" nem para "processo com condição inconsistente".
8. Apenas TCP é listado; sem teste de alcançabilidade externa e **sem IP público**.
9. `stacks_exec` possui flake histórico intermitente (disputa de processos `cargo` no mesmo arquivo de testes), sem reprodução determinística.
10. O Managed Runtime **morre com o app** (Job Object): execuções gerenciadas não sobrevivem ao app, por decisão de segurança.
