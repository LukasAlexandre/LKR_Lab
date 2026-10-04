# Concept 09 — Planejamento: auditoria e especificação refinada

**Status do Concept 09: EM REFINAMENTO** (não aprovado, não "a iniciar", não implementado). Este documento é uma proposta para uma **nova revisão visual**; nada aqui foi implementado nem decidido de forma definitiva.
**Base:** `main` em `cfebf88` (Concepts 01–08 integrados). **Branch:** `feat/session-001-planning-foundation`.
**Fontes lidas:** [CONCEPT-09](../../concepts/machine-registry/CONCEPT-09.md) e a imagem atual, CONCEPT-06/07/08, a [SESSION-001](../sessions/SESSION-001-machine-context-workspace-foundation.md), `TASKS.md` / `docs/ROADMAP.md`, o modelo real de DDAE (Sessions, Blocks, critérios, `ddae_events`), Worktrees (`managed_worktrees`, `worktree_events`), o workspace portátil v4, o roteador de Project e o Project Control Center.

## A. Objetivo do módulo

Planejamento responde **"O QUE VEM DEPOIS?"**; o DDAE responde **"O QUE ESTAMOS EXECUTANDO AGORA?"**. É uma fila **ordenada e leve** de **features ainda não iniciadas** de um Project. Cada item, quando iniciado, **vira uma Session DDAE** (SESSION = FEATURE, regra canônica). Não é Jira, nem Trello, nem backlog enterprise, nem uma segunda DDAE, nem uma segunda lista de Blocks.
Princípio de captura: **anotar uma ideia deve custar segundos** (título + descrição curta). O trabalho de definir objetivo, resultado desejado, critérios e blocos acontece **na Session**, ao iniciar.

## B. Boundaries

| Conceito | NÃO é | Por quê |
|---|---|---|
| **Planning Item** | uma Session | é a feature **antes** da execução; ao iniciar, cria-se UMA Session ligada a ele |
| **Planning Item** | um Block | Blocks decompõem a Session **em execução**; Planejamento é pré-execução |
| **Planning Item** | um Worktree | o Worktree pertence à Session; a cadeia é Planning → Session → Worktree (sem relação direta) |
| **Planning Item** | uma issue do Git | é metadata operacional do LKR LAB (classe B), sem ID de forge |
| **Planejamento** | roadmap de releases / épicos / sprints | sem fases, épicos, milestones nem prioridades P0/P1 |

## C. Modelo conceitual (mínimo)

`PlanningItem`: `id` (UUID v4), `projectId`, `title` (≤120), `description` (≤2000, texto curto; vira o *objetivo* da Session), `position` (ordem manual), `status` **armazenado** (`open | cancelled`), `cancelReason?`, `createdAt`, `updatedAt`, `cancelledAt?`.
**Não existem** no item: prioridade, área, responsável, tags, categoria, milestone, percentual de progresso, status "em execução/concluído", nem `sessionId` — o vínculo vive na **Session** (ver E). Itens ganham eventos portáteis (J/R).
Sem `ready`: um estado "pronto" seria cosmético (não há uma regra mensurável além de "tem título"); a prontidão real é a da Session (Ready for AI, derivado).

## D. Lifecycle e estados

**Armazenado (só dois):** `open` e `cancelled`. **Derivado (nunca gravado)** a partir da Session vinculada:

| Situação | Fase exibida | Observação |
|---|---|---|
| `open`, sem Session | **PLANEJADO** | fila ordenada; o 1º é o "Próximo" |
| `open`, Session `active` | **EM EXECUÇÃO** | |
| `open`, Session `frozen` / `stopped` | **EM EXECUÇÃO** + chip com o estado da Session (CONGELADA / PARADA) | o estado do DDAE aparece **como o da Session**, sem criar um segundo estado de Planejamento |
| `open`, Session `completed` | **CONCLUÍDO** | derivado direto; data = `completedAt` da Session; resultado = `result` da Session |
| `cancelled` | **CANCELADO** | só itens **sem Session**; restaurável |

"Backlog" do mock vira **PLANEJADO** (a própria nota do Concept 09 pede o grupo "Planejado"). "In progress" **nunca é persistido**: duplicaria o estado da Session e divergiria dele.

## E. Relação Planning → Session

- **SESSION = FEATURE**, logo **Planning Item = futura Feature → 1 item : 0..1 Session** no MVP. Isso evita épicos e o caso do mock (3 itens apontando para a mesma SESSION-001).
- **Onde mora o vínculo:** na **Session** (`planningItemId?`, UUID do item), no mesmo padrão de `managed_worktrees.session_id` (FK no lado "muitos"). O banco impõe 1:1 hoje por **índice único parcial** (`WHERE planning_item_id IS NOT NULL`); se o produto passar a querer follow-ups, basta remover o índice (migration) sem mexer no modelo.
- **Session terminal:** `completed` conclui o item (D). Uma Session **parada** e abandonada deixa o item "Em execução / PARADA"; se o usuário quiser recomeçar com outra Session, é a **decisão D2** (1:N vs. cancelar/recriar o item).
- Itens **sem Session** são o estado normal de Planejado. A SESSION-001 real **não** ganha item retroativo automaticamente.

## F. Regras derivadas

1. `fase(item)` = função pura de `(item.status, session?.status)` (tabela D); existe uma única implementação (backend), reutilizada por lista, PCC e Next Action.
2. **Contadores** calculados sobre itens **não cancelados**: `planejados + em execução + concluídos = total` (cancelados têm contador próprio no filtro, fora do total). Nenhum número do mock é reproduzido.
3. "Com Session" ≡ em execução + concluídos e "Sem Session" ≡ planejados: **filtros/cards redundantes são removidos** (o mock os trazia e eles ainda se contradiziam).
4. Progresso exibido é **sempre da Session** (blocos concluídos / total, derivado); item **concluído não mostra progresso parcial** (mostra resultado e data).
5. **Worktrees do item** = worktrees da Session vinculada (leitura derivada; 0..N); nunca guardados no item.
6. Concluir um item **não é ação**: ocorre quando a Session é finalizada explicitamente (finalizar Session continua sendo a ação do DDAE).

## G. Anatomia do item (linha da tabela)

Mantém a **tabela agrupada e recolhível** do mock (densidade técnica > cards/kanban). Colunas propostas: **Item** (título + descrição de 1 linha) · **Fase** (chip PLANEJADO / EM EXECUÇÃO / CONCLUÍDO / CANCELADO; chip secundário CONGELADA/PARADA quando a Session estiver assim) · **Session DDAE** (`SESSION-001` + título + progresso `9/10` derivado + estado da Session; "—" se não houver) · **Worktrees** (derivados da Session: nomes/contagem; "—") · **Última atividade** (o mais recente entre eventos do item e `updatedAt` da Session; "—" se nenhum) · **Ações**.
Ações por fase: **Planejado** → *Iniciar* (abre a Session pré-preenchida), menu: Editar, Mover (↑ ↓ topo), Cancelar. **Em execução** → *Abrir Session* (rota `#project/<id>/ddae/<session-uuid>`). **Concluído** → *Abrir Session*. **Cancelado** → *Restaurar*.
Ordem dos grupos: **Em execução**, **Planejado** (ordem manual), **Concluídos**; "Cancelados" só no filtro.

## H. Resumo (cards superiores)

Total de itens · Planejados · Em execução · Concluídos · **Próximo** (título do primeiro item planejado; "—" se nenhum). Todos derivados e fechando com o total; sem "com/sem Session".

## I. Busca, filtros e ordem

- **Busca:** título, descrição, rótulo/título da Session vinculada.
- **Filtros (um único grupo):** Todos · Planejados · Em execução · Concluídos · Cancelados. **Removidos:** Prioridade, Área, "Mais filtros", responsável.
- **Ordem manual é a fonte da verdade** (a posição no topo = prioridade prática); **filtrar ou buscar nunca altera a posição**. Reordenar começa com **Mover para cima/baixo/topo**; arrastar é refinamento posterior. Posições inteiras com **espaçamento** (ex.: 1000) para mover um item tocar uma única linha.
- **Sem agrupamento "Agora/Próximo/Depois" persistido:** "Próximo" é o 1º planejado (derivado).

## J. Criar item (captura rápida)

"Novo item": **título** (obrigatório) + **descrição** (opcional) e vai para o **fim** da fila (ou "no topo"). Sem resultado desejado, critérios, blocos, prioridade ou área. Evento `PLANNING_ITEM_CREATED`.

## K. Iniciar item (criar Session)

"Iniciar" abre o formulário de **Nova sessão** do DDAE **pré-preenchido** (título ← título do item; objetivo ← descrição) e o usuário **revisa antes de criar** (nunca silencioso). A Session recebe `planningItemId`. Seeds **só** de título e descrição; resultado desejado, critérios, restrições e blocos ficam para a Session (não se duplica conteúdo cegamente). Tudo numa transação: Session criada + vínculo + evento do item (`PLANNING_SESSION_STARTED`) + `SESSION_CREATED` com o `planningItemId`/título no payload.
**Restrição existente:** o DDAE permite **uma Session ativa por Project**; com outra ativa, "Iniciar" fica **desabilitado com a explicação** (não congela a outra). Ver D3.

## L. Conclusão

Derivada (F6): Session `completed` ⇒ item **CONCLUÍDO**, com o resultado e a data da Session. **Não há fechamento explícito do item** (SESSION = FEATURE, sem conceito extra). Concluídos continuam visíveis e filtráveis; nada é apagado.

## M. Cancelamento / arquivamento / exclusão

- **Cancelar** só para item **sem Session** (motivo opcional); item com Session é governado pelo ciclo da Session (parar/congelar/finalizar). Cancelado é **restaurável** (volta a Planejado, ao fim ou à posição original).
- **Excluir destrutivamente não existe** neste corte (preserva histórico); cancelar é o arquivamento. Possível exceção futura (D4): descartar um item recém-criado sem eventos além da criação.

## N. Project Control Center

Card **Planejamento** (hoje placeholder) mostra dados reais: **Próximo: <título>**, `N planejados · M em execução · K concluídos`. **Próxima ação (prioridade a definir na implementação, sem mudar as existentes):** depois de Localizar → conflitos → alterações → runtime → **Continuar/Iniciar bloco da Session ativa**, e **só se não houver Session ativa**, o primeiro item planejado sugere **"Iniciar <item>"**; antes de "Gerar contexto". Sem item/Session reais, nada é inventado.

## O. Integração com a lista DDAE (Concept 06)

Sessions originadas do Planejamento mostram **"Origem: Planejamento — <item>"** (link para `#project/<id>/planning`); Sessions sem item não mostram nada (a SESSION-001 não ganha origem fictícia).

## P. Integração com o Session Detail (Concept 07)

Card/linha **"Item de planejamento"** (título, fase derivada, link) quando existir `planningItemId`; evento `SESSION_CREATED` com a origem aparece no histórico. Sem item: ausente.

## Q. Relação indireta com Worktrees (Concept 08)

Nenhuma relação direta Planning ↔ Worktree. A coluna "Worktrees" é **projeção** (Item → Session → worktrees da Session). Os cards de Worktrees continuam mostrando a Session (e, opcionalmente no futuro, o item da Session) por leitura.

## R. Portabilidade e sync

Classe **B (portátil)**, pertence ao **Project**, não à máquina. Entra no workspace (**schema v5** provável: `planningItems` com eventos aninhados + `planningItemId` na Session; v1–v4 continuam legíveis, ausente = vazio). **Nunca** path absoluto, Machine ID, hostname ou IP. Sem CRDT: edição concorrente do mesmo plano em duas máquinas usa a **proteção de divergência** existente do workspace. Validação portátil: item do mesmo Project; ≤1 Session por item (MVP); Session cancelada/removida do item não existe (cancelar só sem Session).

## S. Migration provável (não criada)

`010`: `planning_items` (id, project_id FK CASCADE, title, description, position, status CHECK(open|cancelled), cancel_reason, created/updated/cancelled_at; `UNIQUE(project_id, position)`); `planning_events` (append-only, trigger contra UPDATE, portável); `ALTER TABLE ddae_sessions ADD COLUMN planning_item_id` + índice único parcial + gatilhos (item do mesmo Project; item não cancelado). Eventos de **reordenação não** são registrados (spam; a posição é estado).

## T. APIs futuras (não criadas)

`planning_overview(project_id)` (itens + fase derivada + Session + worktrees da Session + contadores) · `planning_create_item` · `planning_update_item` · `planning_move(item_id, to)` · `planning_cancel` / `planning_restore` · `planning_start_session(item_id, formulário de Session)` (reaproveita `ddae_create_session` com o vínculo, na mesma transação) · `planning_summary(project_id)` (PCC, leve). Leitura **passiva** (nada grava ao abrir a página).

## U. Testes futuros

Fase derivada para cada combinação (item × Session, incl. frozen/stopped/completed) · contadores que fecham · ordem manual preservada sob filtro/busca · mover/espaçamento/renormalização · cancelar só sem Session, restaurar · 1:1 (índice) e validação portátil · iniciar cria Session vinculada atomicamente e recusa com outra ativa · seed só de título/descrição · eventos append-only/portáteis/sem path · workspace v1–v5 e hash determinístico · PCC (card + Próxima ação sem sobrepor prioridades existentes) · DDAE/Session Detail mostrando origem · página passiva · SESSION-001 sem item retroativo · render dos grupos/estados vazios.

## V. Alterações visuais necessárias no Concept 09 atual

**Já está bom:** tabela agrupada e recolhível (densa, não-kanban); "Em execução → Planejado → Concluído"; vínculo claro item → Session; ação "Criar Session"; busca + filtros por status; shell, cores e estados coerentes (ATIVO destacado).
**Conflitos com a arquitetura / correções:**
1. **Mesma SESSION-001 em seis itens** (viola SESSION = FEATURE) → uma Session por item; SESSION-002/003/004 e as branches/worktrees do mock **não existem**.
2. **Progressos falsos** (8/10 em três itens; 1/10, 2/10, 3/10 em concluídos; o real da SESSION-001 é **9/10**) → progresso só derivado da Session; **concluído sem progresso parcial**.
3. **Resumo inconsistente:** 12 itens, mas 4+3+3 = 10; "5 com Session / 2 sem" ≠ 6 com / 4 sem exibidos → contadores derivados que fecham; remover "com/sem Session".
4. **Falta o grupo Planejado** → "Backlog" renomeado para **PLANEJADO**.
5. **Excesso de Jira:** coluna/filtro **Prioridade** (Alta/Média/Baixa), filtro **Área**, **"Mais filtros"**, **avatar de responsável** (produto local-first de um usuário) → removidos; a ordem manual é a prioridade.
6. **Coluna Worktree direta** duplica a cadeia → projeção via Session (derivada, sem link próprio no item).
7. **"Em execução" persistido por item** → derivado; chip secundário para Session congelada/parada.
8. **Estado "Concluído" com ação implícita** → derivado da Session finalizada.
9. **Labels:** "Backlog" → "Planejado"; "Criar Session" → "Iniciar" (abre o formulário pré-preenchido); "Concluídos" mantém; "Cancelados" só no filtro.
10. **Faltam** estado vazio ("nenhum item planejado"), formulário de "Novo item" (título + descrição), menu "Novo item ▾" e menu "⋯" sem conteúdo definido, e a explicação de "Iniciar" desabilitado (outra Session ativa).
11. **Rodapé/continuidade:** mostrar a **SESSION-001 real** (9/10, bloco atual Concept 09) no contexto, sem itens inventados.

## W. Decisões ainda abertas

- **D1** Um único estado pré-execução (PLANEJADO, recomendado) vs. separar Ideias/Backlog de Planejado vs. estado `ready`.
- **D2** 1:1 (recomendado) vs. 1:N (follow-ups); o que fazer com Session `stopped` abandonada (novo item? segunda Session?).
- **D3** "Iniciar" com Session ativa: desabilitar com explicação (recomendado) vs. criar a Session como congelada/"na fila".
- **D4** Cancelar-só (recomendado) vs. permitir descartar item recém-criado sem histórico.
- **D5** Reordenar: mover ↑↓/topo primeiro (recomendado); arrastar depois.
- **D6** Vínculo guardado na Session (recomendado) vs. no item.
- **D7** `TASKS.md` / `docs/ROADMAP.md`: coexistir como documentação (recomendado), sem importar nem sincronizar.
- **D8** Eventos de Planejamento: criar/editar/cancelar/restaurar/iniciar sim; reordenar **não** (recomendado).
- **D9** Agrupar "Em execução" incluindo Sessions congeladas/paradas com chip (recomendado) vs. grupo próprio.
- **D10** Campos de seed além de título/descrição (resultado desejado, critérios): **não** (recomendado).
- **D11** Item retroativo para sessões existentes (SESSION-001): não automático; permitir "vincular Session existente" é decisão futura.
- **D12** Se "Última atividade" deve incluir eventos de worktree da Session.

## Fora de escopo

Qualquer implementação (migration, tabelas, API, rota, frontend, workspace) e a aprovação do Concept 09: continua **EM REFINAMENTO**, aguardando nova revisão visual.
