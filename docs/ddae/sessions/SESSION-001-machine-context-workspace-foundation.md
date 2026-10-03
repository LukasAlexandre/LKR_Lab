# SESSION-001 — Machine Context & Project Workspace Foundation

**PT:** Cadastro de Máquina e Fundação do Workspace
**Tipo:** Feature
**Status:** ACTIVE / ATIVA

## Objetivo

Transformar o LKR LAB em um assistente de desenvolvimento consciente da máquina e do projeto em que está operando.

A feature começa pela identidade da workstation e evolui para:

Machine Registry → Machine Health → Projects → Project Control Center → DDAE → Worktrees → Planning

Esta sessão combina **concepts e implementação**: 8 de 9 concepts estão aprovados e o Concept 09 — Planejamento está em refinamento. Implementados até agora: Concept 01 (D01) e Concept 02 (D02).

## Blocos

| # | Bloco | Status | Referência |
|---|-------|--------|------------|
| 01 | Product Architecture | CONCLUÍDO | abaixo |
| 02 | Concept 01 — Primeiro acesso / Computador não cadastrado | CONCLUÍDO (visual APROVADO) | [CONCEPT-01](../../concepts/machine-registry/CONCEPT-01.md), [imagem](../../concepts/machine-registry/concept-01-first-access.webp) |
| 03 | Concept 02 — Machine Health Dashboard | CONCLUÍDO (visual APROVADO; IMPLEMENTADO — ver D02) | [CONCEPT-02](../../concepts/machine-registry/CONCEPT-02.md), [imagem](../../concepts/machine-registry/concept-02-machine-health.webp) |
| 04 | Concept 03 — Projetos | CONCLUÍDO (visual APROVADO) | [CONCEPT-03](../../concepts/machine-registry/CONCEPT-03.md), [imagem](../../concepts/machine-registry/concept-03-projects.webp) |
| 05 | Concept 04 — Cadastro de Projeto | CONCLUÍDO (visual APROVADO) | [CONCEPT-04](../../concepts/machine-registry/CONCEPT-04.md), [imagem](../../concepts/machine-registry/concept-04-new-project.webp) |
| 06 | Concept 05 — Project Control Center | CONCLUÍDO (visual APROVADO) | [CONCEPT-05](../../concepts/machine-registry/CONCEPT-05.md), [imagem](../../concepts/machine-registry/concept-05-project-control-center.webp) |
| 07 | Concept 06 — DDAE / Sessões | CONCLUÍDO (visual APROVADO) | [CONCEPT-06](../../concepts/machine-registry/CONCEPT-06.md), [imagem](../../concepts/machine-registry/concept-06-ddae-sessions.webp) |
| 08 | Concept 07 — DDAE / Detalhe da Sessão | CONCLUÍDO (visual APROVADO) | [CONCEPT-07](../../concepts/machine-registry/CONCEPT-07.md), [imagem](../../concepts/machine-registry/concept-07-ddae-session-detail.webp) |
| 09 | Concept 08 — Worktrees | CONCLUÍDO (visual APROVADO) | [CONCEPT-08](../../concepts/machine-registry/CONCEPT-08.md), [imagem](../../concepts/machine-registry/concept-08-worktrees.webp) |
| 10 | Concept 09 — Planejamento | EM ANDAMENTO (visual EM REFINAMENTO) | [CONCEPT-09](../../concepts/machine-registry/CONCEPT-09.md), [imagem](../../concepts/machine-registry/concept-09-planning.webp) |

**Bloco atual:** 10 — Concept 09 — Planejamento, em refinamento (pendências em [CONCEPT-09](../../concepts/machine-registry/CONCEPT-09.md)). Progresso conceitual: 8 de 9 concepts aprovados.

### Bloco 01 — Product Architecture (CONCLUÍDO)

- Computador definido como raiz de contexto.
- Gate obrigatório antes de liberar a aplicação.
- Arquitetura Machine → Projects → Project → DDAE / Worktrees / Planning / Runtime.
- Definição inicial dos próximos concepts.

### Bloco 02 — Concept 01 (CONCLUÍDO)

Primeiro acesso / Computador não cadastrado. Visual APROVADO. Detalhes, regras canônicas, fronteira máquina × portátil e requisito de refresh de 6h em [CONCEPT-01](../../concepts/machine-registry/CONCEPT-01.md).

## Desenvolvimento

| # | Bloco | Status | Branch |
|---|-------|--------|--------|
| D01 | Machine Registry Foundation (Concept 01) | CONCLUÍDO | `feat/session-001-machine-registry` |
| D02 | Machine Health (Concept 02) | CONCLUÍDO | `feat/session-001-machine-registry` |
| D03 | Projects Foundation (Concepts 03–05) | A INICIAR (nenhuma implementação feita) | `feat/session-001-projects-foundation` |

### Checkpoint — MACHINE FOUNDATION COMPLETE

A camada **MACHINE** está concluída e integrada na `main` por fast-forward (sem commit de merge). A branch `feat/session-001-machine-registry` foi preservada como marco histórico.

- **Concept 01 — Machine Registry:** IMPLEMENTADO (Machine ID persistente, cadastro e edição de metadata, gate frontend/backend, SQLite local, inventário, refresh de 6 h e manual, web preview segura).
- **Concept 02 — Machine Health:** IMPLEMENTADO (inventário separado da telemetria, capabilities, sampler único, saúde e alertas).
- **Validação cross-machine:** concluída em uma segunda workstation, que revelou e corrigiu displays virtuais contados como GPUs físicas (ver D02). Refresh operacional do inventário confirmado com 1 GPU física.
- **Próximo domínio:** PROJECTS. **Próximo concept de implementação:** Concept 03 — Projetos (depois 04 — Cadastro de Projeto e 05 — Project Control Center), na branch `feat/session-001-projects-foundation`.

A SESSION-001 **continua ATIVA**: ela representa a feature maior (Machine Context & Project Workspace Foundation), da qual a camada MACHINE é a primeira etapa. Os status visuais dos concepts não mudaram.

### Bloco D01 — Machine Registry Foundation (CONCLUÍDO)

Implementação do Concept 01. (Na época do bloco, o Dashboard seguia como estava; o Concept 02 foi implementado depois, no D02.)

- **Identidade:** `machine_id` UUID v4 gerado pelo LKR LAB no cadastro e persistido no `hub.db` (migration aditiva `005_machine.sql`, tabela de linha única `machine`). Hostname, IP, interfaces e hardware são atributos do snapshot; mudar qualquer um deles atualiza o snapshot e nunca cria outra máquina.
- **Fonte de verdade:** SQLite local (estado desta máquina, classe C de [STATE.md](../../STATE.md)); nada entra em `data/workspace.json`.
- **Dados do usuário:** nome deste computador, uso/local (Casa, Trabalho, Outro) e descrição opcional (até 120 caracteres). O hostname é só sugestão.
- **Detecção passiva** (`hub-core::machine`): sysinfo (sistema, CPU, RAM, discos, interfaces/IPv4, uptime), registro do Windows (DisplayVersion, build, VRAM) e adaptadores de vídeo presentes (`EnumDisplayDevicesW`); IPv4/interface ativa pela tabela de rotas (UDP `connect`, sem enviar pacotes). Nenhum programa é executado; sem MAC, serial, chaves ou variáveis de ambiente. Atributo indisponível fica ausente e não bloqueia o cadastro.
- **Validade de 6h:** ao abrir o app (leitura instantânea do cadastro e, em seguida, detecção só se expirada), ao voltar o foco/visibilidade da janela (inclui retorno de suspensão), consulta leve a cada 15 min com o app aberto (só detecta se expirado) e "Atualizar detecção"/"Atualizar agora" (força).
- **Gate global:** no backend, todo comando do app que não seja `machine_status`, `machine_refresh` ou `machine_register` é recusado com `MACHINE_NOT_REGISTERED` antes do cadastro; no frontend, o App (rotas, atalhos, paleta, carregamentos) não é montado até o cadastro, então hash/deep-link não alcançam módulos.
- **Web:** prévia da tela de cadastro, sem detecção nem dados simulados; cadastro só no desktop.
- **Depois do cadastro:** redireciona ao Dashboard; nome da máquina na barra superior; painel "Este computador" em Configurações com "Atualizar agora".
- **Testes:** `crates/hub-core/tests/machine.rs` (identidade, persistência, 6h, refresh manual, mudança de IP e de hardware, dados indisponíveis, gate, validação, isolamento do workspace portátil, migration 005 contra banco v4 legado) e `src/shared/machine.test.ts`.
- **Edição da metadata (fechamento do bloco):** em Configurações › Este computador, "Editar" abre edição inline de nome, uso/local e descrição ("Cancelar" / "Salvar alterações"). Comando `machine_update` (só `MachineInput`, mesmas regras do cadastro, bloqueado pelo gate antes do cadastro) altera apenas `name`, `usage`, `description` e `updated_at`; `machine_id`, `created_at`, snapshot e `last_detected_at` são preservados e nenhuma detecção é disparada. "Atualizar agora" continua separado e não toca a metadata. O nome na barra superior muda ao salvar; uma leitura iniciada antes da edição não traz o nome antigo de volta. Sem migration.
- **Limitações:** GPU só no Windows; sem histórico de snapshots.

### Bloco D02 — Machine Health (CONCLUÍDO)

**IMPLEMENTAÇÃO DO CONCEPT 02 CONCLUÍDA.** O visual já havia sido aprovado; este bloco registra a implementação, validada no desktop real. A SESSION-001 continua ATIVA.

- **Inventário × Telemetria separados:** o inventário (identidade, metadata, hardware relativamente estático, refresh de 6h do D01) não mudou de papel. A telemetria dinâmica vive em `hub-core::telemetry`, `health` e `sensors`.
- **Sampler único:** um `Service` por app (estado gerenciado pelo Tauri), com thread própria. O Dashboard renova um lease curto (`machine_telemetry_watch`); sem renovação, ou com a janela oculta/minimizada, o sampler desacelera e coleta menos. Navegar entre telas não cria sampler novo. Evento `machine://telemetry`; `machine_telemetry` devolve o estado atual e o buffer das sparklines; `machine_telemetry_refresh` ("Atualizar agora") força amostra completa. Os comandos seguem atrás do gate de máquina cadastrada.
- **Capabilities explícitas:** cada métrica carrega disponibilidade. Indisponível nunca vira zero: a UI mostra "—".
- **Métricas:** CPU (uso e frequência), RAM, discos (uso por volume; Disk I/O pelo **disco físico mais ativo**, nunca média dos discos), rede, GPU no Windows (utilização, memória **dedicada** e **compartilhada** como dimensões separadas, sem campo único de "VRAM total"), temperaturas dos sensores suportados e processos.
- **Temperaturas:** GPU pela API do Windows; NVMe pelo próprio dispositivo, classificados só quando o dispositivo informa limites (sem thresholds arbitrários); `0 °C` não é leitura válida (GPU integrada sem sensor = indisponível). Temperatura de CPU Package **não está disponível** na máquina validada; o sensor ACPI TZ00 aparece como "Sensor térmico do sistema (ACPI TZ00)", sem limite, **não participa da saúde** (sem semântica de CPU comprovada) e nunca é exibido como CPU.
- **Processos:** top por CPU, RAM, GPU e Disk I/O. GPU muito pequena aparece como `<0,1%`; memória de GPU indisponível aparece como "—".
- **Machine Health determinístico:** Saudável / Atenção / Crítico. Limiares com janela sustentada: CPU 90%/95% (60 s/180 s), RAM 90%/95% (30 s/60 s), espaço livre 10%/5%, temperatura de GPU 85/95 °C (30 s). Métrica indisponível, ACPI sem semântica e NVMe sem limite do dispositivo são **neutros** e não influenciam a saúde. Alertas derivam da mesma avaliação.
- **Web:** continua na prévia da tela de cadastro, sem telemetria nem valores simulados.
- **Validação:** testes Rust em `crates/hub-core/tests/telemetry.rs` e testes de apresentação em `src/shared/telemetry.test.ts`; validação funcional no desktop real e cargas controladas para CPU e Disk I/O por processo. Overhead do processo Rust baixo (≈0,15% de CPU com o Dashboard ativo; ≈0,08% em outra tela). Contagem de threads estável após navegação repetida.
- **Validação cross-machine (segunda workstation):** achou e corrigiu **adaptadores de vídeo virtuais (Microsoft Indirect Display, "MS Idd Device") sendo contados como GPUs físicas** — uma GPU Intel aparecia como quatro no Dashboard e no inventário. Causa: o Windows registra um adaptador DirectX, com LUID próprio e a mesma descrição/IDs da GPU real, para cada display virtual, e a enumeração só deduplicava por LUID. Solução arquitetural (em `hub-core::sensors`, não na UI): a GPU é um **adaptador que renderiza ou computa**, decidido pelos bits de `D3DKMT_ADAPTERTYPE` (consulta D3DKMT em tempo de execução; `AdapterType` do registro só como reserva; nome só como último recurso). `IndirectDisplayDevice`, `SoftwareDevice` e adaptadores só de vídeo não são GPU. A **identidade física é o endereço PCI** (`KMTQAITYPE_ADAPTERADDRESS`), nunca nome, VendorId ou DeviceId: duas placas idênticas continuam duas GPUs, e sem endereço confiável nada é fundido. Cada GPU carrega `id` e todos os seus LUIDs (fontes PDH); a telemetria agrega por GPU, e a temperatura casa por `id` em vez de por nome. Verificado que os LUIDs virtuais não têm instâncias nos contadores PDH de GPU, então nenhuma utilização se perde. Testes de topologia em `crates/hub-core/tests/gpu_topology.rs` (fixtures genéricas).
- **Limitações:** telemetria de GPU, discos e sensores só no Windows; sem CPU Package; sem histórico persistido (apenas buffer em memória para as sparklines). Placas idênticas aparecem com o mesmo nome (distintas internamente pelo `id`).
- **Dívidas técnicas pequenas:** o CSS legado do antigo Dashboard (`src/features/Dashboard.tsx` foi removido) segue sem uso em `src/styles.css`; remover em tarefa isolada para evitar regressão visual. Intermitência pré-existente em `crates/hub-core/tests/stacks_exec.rs` (testes `tauri_*`), não relacionada a esta feature.

## Roadmap de concepts

| # | Concept | Status |
|---|---------|--------|
| 01 | Primeiro acesso / Computador não cadastrado | APROVADO / IMPLEMENTADO |
| 02 | Dashboard da Máquina / Machine Health | APROVADO / IMPLEMENTADO |
| 03 | Projetos | APROVADO |
| 04 | Cadastro de Projeto | APROVADO |
| 05 | Project Control Center | APROVADO |
| 06 | DDAE / Sessões | APROVADO |
| 07 | DDAE / Detalhe da Sessão | APROVADO |
| 08 | Worktrees | APROVADO |
| 09 | Planejamento | EM REFINAMENTO |

## Decisões de arquitetura registradas

- **Hierarquia:** MACHINE → PROJECTS → PROJECT → DDAE / WORKTREES / PLANNING / RUNTIME.
- **Machine ID:** estável e persistente por máquina. IP, hostname e hardware são atributos, nunca identidade (nem isoladamente nem como identidade primária).
- **Nome amigável:** definido/confirmado pelo usuário, não inferido do hardware.
- **Fronteira de estado:** estado da máquina nunca é sincronizado; nome amigável, identidade lógica, descrição e organização são portáveis. Ver [STATE.md](../../STATE.md) e [ADR-004](../../adr/ADR-004-portable-state.md). Nenhum sync novo nesta sessão.
- **Refresh do snapshot da máquina:** na inicialização; a cada 6h com o app aberto; "Atualizar agora" (futuro); novo refresh ao retornar de suspensão/hibernação com snapshot expirado; sem polling pesado.

## Requisitos de produto para fases futuras (não implementar agora)

### Estados das sessões DDAE

| Estado | Direção visual inicial |
|--------|------------------------|
| ACTIVE / ATIVA | mais viva; glow/sombra; pulsação discreta |
| FROZEN / CONGELADA | fria; estática; sem pulsação |
| STOPPED / PARADA | neutra; pouco destaque |
| FINISHED / FINALIZADA | resolvida; estática; sem sensação de atividade |

### Estados de Worktrees

ATIVO, CONGELADO, PARADO, FINALIZADO. São **metadata operacional do LKR LAB**; o Git não possui esses conceitos nativamente e eles não devem ser apresentados como se possuísse.

### Planejamento

Área do projeto para organizar o que está planejado para o sistema. Inicialmente: lista, backlog, tarefas, ideias e próximos passos. Futuramente: item planejado → criar Session DDAE → Session ativa → conclusão → item concluído.

## Fora de escopo desta sessão (por ora)

Além dos blocos D01 e D02: alterações no restante do Dashboard/sidebar, páginas React, migrations, tabelas, SQLite, APIs, bridge, DDAE visual, Worktrees, Planning, geração de imagens e implementação dos concepts já aprovados.

## Convenção

Não havia estrutura DDAE no repositório. Esta é a primeira sessão e define a convenção mínima: `docs/ddae/sessions/SESSION-NNN-<slug>.md`. Concepts ficam em `docs/concepts/<tema>/`. Convenção sujeita a validação.
