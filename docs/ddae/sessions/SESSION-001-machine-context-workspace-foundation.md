# SESSION-001 — Machine Context & Project Workspace Foundation

**PT:** Cadastro de Máquina e Fundação do Workspace
**Tipo:** Feature
**Status:** ACTIVE / ATIVA

## Objetivo

Transformar o LKR LAB em um assistente de desenvolvimento consciente da máquina e do projeto em que está operando.

A feature começa pela identidade da workstation e evolui para:

Machine Registry → Machine Health → Projects → Project Control Center → DDAE → Worktrees → Planning

Esta sessão está na fase de **concepts e registro**: 8 de 9 concepts estão aprovados e o Concept 09 — Planejamento está em refinamento. Nenhuma implementação foi iniciada.

## Blocos

| # | Bloco | Status | Referência |
|---|-------|--------|------------|
| 01 | Product Architecture | CONCLUÍDO | abaixo |
| 02 | Concept 01 — Primeiro acesso / Computador não cadastrado | CONCLUÍDO (visual APROVADO) | [CONCEPT-01](../../concepts/machine-registry/CONCEPT-01.md), [imagem](../../concepts/machine-registry/concept-01-first-access.webp) |
| 03 | Concept 02 — Machine Health Dashboard | CONCLUÍDO (visual APROVADO) | [CONCEPT-02](../../concepts/machine-registry/CONCEPT-02.md), [imagem](../../concepts/machine-registry/concept-02-machine-health.webp) |
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

### Bloco D01 — Machine Registry Foundation (CONCLUÍDO)

Implementação do Concept 01. Concept 02 em diante não foram implementados; o Dashboard atual segue como está.

- **Identidade:** `machine_id` UUID v4 gerado pelo LKR LAB no cadastro e persistido no `hub.db` (migration aditiva `005_machine.sql`, tabela de linha única `machine`). Hostname, IP, interfaces e hardware são atributos do snapshot; mudar qualquer um deles atualiza o snapshot e nunca cria outra máquina.
- **Fonte de verdade:** SQLite local (estado desta máquina, classe C de [STATE.md](../../STATE.md)); nada entra em `data/workspace.json`.
- **Dados do usuário:** nome deste computador, uso/local (Casa, Trabalho, Outro) e descrição opcional (até 120 caracteres). O hostname é só sugestão.
- **Detecção passiva** (`hub-core::machine`): sysinfo (sistema, CPU, RAM, discos, interfaces/IPv4, uptime), registro do Windows (DisplayVersion, build, VRAM) e adaptadores de vídeo presentes (`EnumDisplayDevicesW`); IPv4/interface ativa pela tabela de rotas (UDP `connect`, sem enviar pacotes). Nenhum programa é executado; sem MAC, serial, chaves ou variáveis de ambiente. Atributo indisponível fica ausente e não bloqueia o cadastro.
- **Validade de 6h:** ao abrir o app (leitura instantânea do cadastro e, em seguida, detecção só se expirada), ao voltar o foco/visibilidade da janela (inclui retorno de suspensão), consulta leve a cada 15 min com o app aberto (só detecta se expirado) e "Atualizar detecção"/"Atualizar agora" (força).
- **Gate global:** no backend, todo comando do app que não seja `machine_status`, `machine_refresh` ou `machine_register` é recusado com `MACHINE_NOT_REGISTERED` antes do cadastro; no frontend, o App (rotas, atalhos, paleta, carregamentos) não é montado até o cadastro, então hash/deep-link não alcançam módulos.
- **Web:** prévia da tela de cadastro, sem detecção nem dados simulados; cadastro só no desktop.
- **Depois do cadastro:** redireciona ao Dashboard; nome da máquina na barra superior; painel "Este computador" em Configurações com "Atualizar agora".
- **Testes:** `crates/hub-core/tests/machine.rs` (identidade, persistência, 6h, refresh manual, mudança de IP e de hardware, dados indisponíveis, gate, validação, isolamento do workspace portátil, migration 005 contra banco v4 legado) e `src/shared/machine.test.ts`.
- **Limitações:** não há edição do nome/uso/descrição depois do cadastro; GPU só no Windows; sem histórico de snapshots.

## Roadmap de concepts

| # | Concept | Status |
|---|---------|--------|
| 01 | Primeiro acesso / Computador não cadastrado | APROVADO |
| 02 | Dashboard da Máquina / Machine Health | APROVADO |
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

Além do bloco D01: alterações em Dashboard/sidebar, páginas React, migrations, tabelas, SQLite, APIs, bridge, DDAE visual, Worktrees, Planning, geração de imagens e implementação dos concepts já aprovados.

## Convenção

Não havia estrutura DDAE no repositório. Esta é a primeira sessão e define a convenção mínima: `docs/ddae/sessions/SESSION-NNN-<slug>.md`. Concepts ficam em `docs/concepts/<tema>/`. Convenção sujeita a validação.
