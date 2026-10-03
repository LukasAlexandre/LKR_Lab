# Auditoria — Concept 07 (DDAE / Detalhe da Session)

**Tipo:** auditoria somente leitura, anterior à implementação. Nada aqui foi implementado.
**Base:** `main` em `bf3982f` (Concept 06 integrado). **Branch:** `feat/session-001-ddae-session-detail`.
**Fontes lidas:** [CONCEPT-07](../../concepts/machine-registry/CONCEPT-07.md) e a imagem aprovada, [CONCEPT-06](../../concepts/machine-registry/CONCEPT-06.md), [SESSION-001](../sessions/SESSION-001-machine-context-workspace-foundation.md), `hub-core::ddae`, migrations 006/007, comandos Tauri `ddae_*`, `DdaeSessions.tsx`, Project Control Center, `projectRoute.ts`, `types.ts`, workspace portátil (Rust e JS) e a tabela `activities`.

O Concept 07 **não é uma entidade nova**: é o detalhe operacional da MESMA Session DDAE. Não há segunda store.

## 1. Rota e navegação

| Pergunta | Hoje | Recomendação |
|---|---|---|
| Rota atual | `#project/<id>/<área>`; um 4º segmento é tratado como inválido e **normalizado** (a URL é reescrita sem ele) | `#project/<project-id>/ddae/<session-uuid>` |
| Identidade | — | **UUID da Session**; nunca `SESSION-NNN` nem título |
| Reload / deep link | funcionam para as áreas (o roteador é puro e a rota é a fonte de verdade) | funcionam igual, desde que o parser aceite o 4º segmento só para `area=ddae` |
| Project + Session validados juntos | não existe | o backend resolve `(project_id, session_id)`; Session de outro Project = recusa |
| Session inexistente/removida/de outro Project | — | volta para `#project/<id>/ddae` com aviso, como `invalid-project` faz hoje (nunca elege outra) |
| Breadcrumb | `Workspace › Projetos › Projeto [› Área]` (`App.tsx`) | acrescentar `› SESSION-NNN` (display humano; o link da área continua para a lista) |
| Sidebar | "PROJETO ATUAL" com 8 áreas | **inalterada**; "DDAE / Sessões" continua selecionado no detalhe. Nenhum item global novo |

## 2. Abas (imagem aprovada)

Visão geral · Blocos · **Planejamento** · Decisões · Arquivos · Anotações.
A imagem chama a terceira aba de "Planejamento"; **recomendação: renomear para "Plano da sessão"** para não colidir com o Concept 09 (Planejamento do Project, EM REFINAMENTO). Só a Visão geral foi desenhada; as demais abas não têm design.
"Plano da sessão" **não pode** ser uma segunda estrutura de blocos. Proposta: visão de leitura/edição de Objetivo, Resultado desejado, Restrições, Critérios de conclusão e o Escopo (= a lista de Blocks). Os campos já existem; o escopo é derivado dos blocks.

## 3. Cobertura por elemento

Legenda: **OK** = backend já tem; **UI** = só falta tela; **API** = falta API; **ESQUEMA** = falta dado; **ADIAR** = outro concept.

| Elemento do Concept 07 | Estado | Observação |
|---|---|---|
| SESSION-NNN, título, estado, início, "Tipo Feature" | OK / UI | `createdAt` e `Feature` constante |
| **Criador** ("Lukas") | **ESQUEMA — não implementar** | não há campo de autor; é valor ilustrativo. Omitir, não inventar |
| Última atualização | OK | `updatedAt` |
| Bloco atual, progresso, próximo, contagens | OK | derivados no backend (`SessionView`) |
| **Categoria do bloco** ("DESIGN") | **ESQUEMA — ADIAR** | `Block` só tem `id/title/status` |
| Objetivo | OK + API | `ddae_update_details` substitui o conjunto de campos |
| Resultado desejado | OK + UI | migration 007; **vazio na SESSION-001**; o 07 deve permitir preenchê-lo |
| **Escopo (lista de blocos)** | **Derivado** | **não existe campo `scope`**. O "Escopo da Session" do concept é a lista de Blocks (inclusive um bloco final "Fechamento / validação", que é um Block comum). **Gap registrado; recomendação: não criar `scope`** |
| Restrições | OK + UI | lista de textos (007) |
| **Critérios de conclusão (checklist)** | **ESQUEMA** | a imagem mostra itens marcados/desmarcados; hoje são **strings sem estado**. Ver decisão D1 |
| Decisões (lista resumida) | OK / UI | `ddae_decisions`; só adicionar. **Sem** vínculo opcional a Block e **sem** editar/remover |
| Blocos recentes / Escopo | OK / UI | |
| Contexto da Session / Pronto para IA | OK / UI | `readyForAi` derivado + `ddae_generate_context`; mostra exatamente o que falta |
| Arquivos e referências | OK + API | `references` (`project_path` relativo, `url` https); **sem rótulo**; sem seletor |
| Anotações | OK / UI | `notes: string[]` no JSON da Session; sem id, sem data |
| **Histórico recente** | **ESQUEMA** | ver §5 |
| Relação com Worktrees | **ESQUEMA — ADIAR (Concept 08)** | nada persiste a relação; `Worktree` é só fato do Git. Não mostrar no 07 |
| Continuar sessão / Gerar contexto | UI | "Continuar" = ações explícitas de bloco; **Gerar contexto não chama IA** |
| Próxima ação | OK | `deriveDdaeNextAction` |

## 4. Operações existentes × faltantes

**Existem (Tauri):** `ddae_overview` (lista com blocks, decisões, detalhes e `readyForAi` de TODAS as sessões do Project), `ddae_create_session`, `ddae_add_block`, `ddae_start_block`, `ddae_complete_block`, `ddae_freeze`, `ddae_stop`, `ddae_resume`, `ddae_complete`, `ddae_add_decision`, `ddae_update_details`, `ddae_generate_context`.

**Faltam para o 07:**
1. **Ler UMA Session validando o Project** (`ddae_session_detail(project_id, session_id)`): hoje `Database::ddae_session(session_id)` existe mas **não é exposto** e não confere o Project; as mutações também recebem só `session_id`. O UUID é único, então não há colisão, mas a rota exige a checagem de pertencimento.
2. **Editar o título** (UUID e número imutáveis). Provável: sim, é seguro.
3. **Renomear Block** e **remover Block pendente** (essenciais para o plano ser editável); **reordenar** e **descrição/categoria** ficam para depois.
4. **Critérios com estado** (D1), se a decisão for checklist.
5. **Histórico por Session** (§5).
6. **Converter arquivo escolhido em picker em caminho RELATIVO ao Project** (o backend conhece o binding; precisa canonicalizar e recusar o que estiver fora do Project). Viável, mas é comando novo.
7. Rótulo de referência (opcional).

## 5. Histórico: a tabela `activities` não basta

`activities(id, project_id, action TEXT, created_at)`. O DDAE grava textos do tipo `DDAE: SESSION-001 congelada` — **associados só ao Project**. Filtrar por Session seria por `LIKE` no texto (frágil; o número é por Project e o texto muda). Além disso `activities` é **estado da máquina** (não viaja no workspace): o histórico de uma Session não apareceria no outro PC.
Recomendação: uma tabela **`ddae_events(id, session_id FK CASCADE, kind, block_id?, created_at)`** (migration 008) escrita na mesma transação das mutações. Decisão D2: local (classe C, recomendado para o MVP) ou portátil.

## 6. Ciclo de vida

- **ACTIVE → FROZEN/STOPPED** exigem motivo: reaproveitar o diálogo de motivo do Concept 06. **FROZEN/STOPPED → ACTIVE**: o backend já recusa se houver outra ACTIVE; a UI explica e **não** altera a outra Session. **COMPLETED** é terminal.
- **Completar o último Block não finaliza a Session** (confirmado: `ddae_complete_block` só muda o bloco; 9/10 → 10/10 mantém a Session ativa). Finalizar continua uma ação explícita, com confirmação.
- Hoje `ddae_complete` exige ≥1 bloco, todos concluídos e nenhum em andamento. **D3:** exigir também todos os critérios concluídos? Recomendação: **sim, quando existirem critérios** (senão o checklist é decorativo); sem critérios, nada muda (a SESSION-001 legada não fica bloqueada). Critérios **não** entram no progresso, que continua sendo blocos.
- O concept só desenha o estado ATIVA; faltam as variações CONGELADA/PARADA/FINALIZADA (ações, "reabrir" não existe no MVP).

## 7. Edição

Campos editáveis: título, objetivo, resultado desejado, restrições, critérios, notas, referências. **Não editáveis:** número, UUID, projeto, estado (só por transição), blocks (por operações próprias). Recomendação de UX: **modo leitura + ação "Editar" por seção** (substitui o conjunto, como `ddae_update_details` já faz), sem edição inline global. Atenção: `ddae_update_details` substitui tudo; a UI deve enviar o estado completo (perda de atualização só importa com dois dispositivos editando antes de sincronizar).

## 8. Contexto

Separar: **dados estruturados da Session** (Visão geral/Plano) × **contexto determinístico para IA** (`ddae_generate_context`, Markdown). UX recomendada: ação **Gerar contexto** abre um painel/modal com o Markdown e "Copiar"; **sem IA, sem envio**. O contexto é gerado sempre do estado atual, então "Atualizado agora" do concept deve virar "Gerado a partir do estado atual" (não há noção de contexto defasado). "Pronto para IA" continua derivado; **não** existe botão "Marcar pronto". Na SESSION-001: faltam resultado desejado e critérios de conclusão.

## 9. Migration

- **Escopo (`scope`):** não necessário.
- **Relação com Worktree:** não necessário agora (Concept 08).
- **Histórico por Session:** **migration 008 provável** (`ddae_events`).
- **Critérios com estado:** são JSON em `criteria`; mudar de `string[]` para `{id,text,done}[]` não exige SQL, mas **muda o formato portátil** (Rust + JS + golden). Precisa ler as duas formas (string antiga e objeto) e decidir se sobe o schema do workspace (v3) ou se é aditivo.
- **Decisão ↔ Block opcional:** coluna nullable em `ddae_decisions` (adiar se não for essencial).

## 10. Riscos

1. Mudar o formato dos critérios no workspace portátil: um app antigo recusa/descarta; é preciso compatibilidade de leitura e teste golden.
2. O parser de rota hoje normaliza o 4º segmento: alterar sem cobrir todos os casos de `projectRoute.test.ts` quebra links.
3. Mutations sem checagem de Project (hoje seguro por UUID, mas a UI nova deve sempre passar pelo par validado).
4. Histórico local não sincroniza: o usuário pode esperar ver no PC B o que aconteceu no A.
5. Escopo do corte: aba "Plano da sessão" virar um planejamento paralelo (invade o Concept 09).
6. A SESSION-001 real não pode ser alterada pela implementação (9/10; Concept 09 em andamento); testar em Sessions temporárias, nunca na real.

## 11. Decisões em aberto (precisam de resposta antes da UI)

- **D1** Critérios de conclusão com estado (`done`)? Recomendado: sim.
- **D2** Histórico por Session: local (recomendado) ou portátil?
- **D3** Finalizar exige critérios concluídos? Recomendado: sim, só quando existirem.
- **D4** Nome da aba: "Plano da sessão" (recomendado) em vez de "Planejamento".
- **D5** Primeiro corte de Blocks: renomear + remover pendente (recomendado); reordenar/categoria/descrição depois.
- **D6** Decisões: só listar e adicionar neste corte (recomendado); editar/remover/vincular a Block depois.
- **D7** Notas: lista simples de textos neste corte (o JSON atual atende; sem tabela nova).

## 12. Testes necessários (a escrever na implementação)

Roteamento (4º segmento, normalização, ids malformados) · deep link/reload · mismatch Project×Session e Session removida · edição de título/detalhes e limites · critérios (criar/remover/marcar, relação com finalizar) · Blocks (renomear/remover pendente/ordem, ações bloqueadas fora de Session ativa) · decisões · referências (relativa, absoluta recusada, `..`, URL) e picker→relativo · notas · Ready for AI (todos os casos, SESSION-001 legada continua "incompleta") · contexto (determinismo, sem dado local) · transições e motivo · one-active ao retomar · finalizar (explícito, completar o último bloco não finaliza) · histórico por Session (ordem, isolamento entre Sessions, cascade) · workspace v2 com os novos campos (Rust + JS + golden) · render das abas e do estado vazio.

## 13. Fora do escopo do Concept 07

Concept 08 (relação Session↔Worktree, estado operacional de worktree), Concept 09 (Planejamento do Project), IA/agentes, CRDT/merge por Session, categoria/descrição/reordenação de blocks, autor da Session.
