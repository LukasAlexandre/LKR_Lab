# Auditoria — Concept 08 (Worktrees)

**Tipo:** auditoria somente leitura, anterior à implementação. Nada aqui foi implementado.
**Base:** `main` em `de2c605` (Concept 07 integrado). **Branch:** `feat/session-001-worktrees-foundation`.
**Fontes lidas:** [CONCEPT-08](../../concepts/machine-registry/CONCEPT-08.md) e a imagem aprovada, `hub-core::git` (`worktrees`, `worktree_path`, `create_worktree`, `remove_worktree`), comandos Tauri `list/create/remove/launch_worktree`, `Worktrees.tsx`, `overview::real_git`, `runtime::detect`, supervisor, `activities`, modelo DDAE (Sessions, `ddae_events`), workspace portátil v3 e o roteamento do Project.

## 1. Três conceitos que NÃO são a mesma coisa

| Conceito | O que é | Onde vive | Classe de estado |
|---|---|---|---|
| **Git worktree real** | o checkout que o Git conhece (`git worktree list`) | Git / disco | máquina (C) |
| **Worktree do LKR LAB** | metadata persistente: nome de exibição, descrição, vínculo com Session/Block, resultado | SQLite + workspace portátil | portátil (B) |
| **Estado operacional** | ATIVO / CONGELADO / PARADO / FINALIZADO | metadata do LKR LAB | portátil (B) |

O estado operacional **nunca** é derivado de Git (dirty/clean/ahead/behind/merged/branch apagada) nem de runtime. Exemplos válidos: ATIVO + Git Clean; ATIVO + Dirty; CONGELADO + Ahead; FINALIZADO + branch ainda existente. **FINALIZADO é só metadado:** não remove worktree, pasta nem branch, não faz merge, push nem checkout.

## 2. O que existe hoje (Git e backend)

- **`git::worktrees(repo)`** roda `git worktree list --porcelain` (somente leitura) e devolve `{ path, head, branch, locked }`. A `branch` perde o prefixo `refs/heads/`; **detached = `branch` vazia** (a UI mostra "Detached HEAD"; a struct não expõe um `detached` explícito). `locked` vem de qualquer linha que comece com `locked` (o **motivo** é descartado). **`prunable` e `bare` não são lidos**: uma worktree prunable aparece como normal; um repositório bare apareceria com branch vazia, parecendo detached.
- **Checkout principal:** `git worktree list` o inclui sempre como **primeira** entrada; o backend o identifica só pela posição (`is_main` = primeiro item). A UI atual o lista com o rótulo "Principal" e não permite remover.
- **Identidade atual:** somente o **path** (a UI usa `tree.path` como `key`; `worktree_path()` confere por caminho canônico que o path pertence ao repositório). **Não existe ID estável**: HEAD muda a cada commit e a branch pode ser renomeada, recriada ou ficar detached.
- **Criar:** `create_worktree(repo, path, branch)` cria uma **branch nova a partir do HEAD atual** (`git worktree add -b`), exige path **absoluto**, novo, sem `..`, **fora** do repositório. Não há base branch escolhida, nem branch existente, nem vínculo com Session. Registra só `activities` "Worktree criada" (nível Project, texto livre).
- **Remover:** `remove_worktree(repo, path, confirmed)` exige confirmação explícita; recusa a principal, a atual e as `locked`; recusa se `git status --porcelain --ignored` não estiver vazio; **nunca usa `--force` nem `remove_dir_all`**; preserva a branch. Registra `activities` "Worktree removida". **Nenhuma operação atual é perigosa**, mas "Remover…" é um `git worktree remove` real e hoje não existe nada que o distinga de "finalizar".
- **Abrir:** `launch_worktree` abre pasta/terminal/VS Code só se o path pertencer ao repositório.
- **Abertura passiva:** a lista usa **somente** `git worktree list`. Não há fetch, pull, checkout, switch, prune nem repair automáticos. Isso precisa continuar: **a página Worktrees ao abrir é READ-ONLY.**
- **Git por worktree:** `overview::real_git(path)` e `runtime::detect(path)` recebem um `&Path` qualquer (uma chamada `git status` sem mutação; leitura de manifests). Hoje só são chamados no root do Project, mas **servem para o path de cada worktree**. A atribuição de processos/portas e de execuções gerenciadas é **por Project** (`runs_for(project_id)`, fatos do overview): o runtime **por worktree** pede atribuição por pasta (cwd/path dos processos) — não existe; deve ser verificada na implementação. Runtime e estado operacional são independentes ("Parado" de runtime ≠ PARADO operacional).
- **Sem vínculo com DDAE:** a UI mostra o texto fixo "Agente não associado". Nada liga Worktree a Session ou Block.

## 3. O Concept aprovado (imagem + texto)

Cabeçalho (Projeto, Máquina, Branch principal, Git) com **Atualizar** e **Novo worktree**; cards de resumo (total, Ativo, Congelado, Parado, Finalizado, com alterações Git); busca + filtro operacional + filtro Git (Todos/Clean/Alterações); card por worktree (nome, descrição, branch, path, Git, HEAD com copiar, runtime, **sessão DDAE**, **bloco relacionado**, última atividade, **Abrir worktree**, menu; finalizadas mostram **Resultado**); rodapé com a Session ativa e a Próxima ação.
**A `main` aparece como a primeira worktree**, ATIVA, "Branch principal do projeto". Não há detalhe de worktree, nem estado vazio, erro Git ou "não localizado". O texto do concept declara: estado operacional, vínculo e resultado **podem ser portáteis**; path, HEAD e estado Git **são da máquina**.

## 4. Proposta de modelo (a decidir; nada criado)

**Três camadas, como em Projects:**

| Camada | Conteúdo | Portável? |
|---|---|---|
| **Worktree ID** | UUID estável do LKR LAB | sim |
| **Locator (Git)** | `repository locator` do Project + `ref` (branch) como **dica**, + (quando existir) o nome da worktree no Git; nunca path | sim |
| **Binding local** | path absoluto desta máquina (`worktree_bindings`) | **não** (classe C) |

- **Branch não é identidade** (renomeia, recria, pode estar detached); é metadado/dica do locator. **Detached HEAD** tem de ser suportado: locator sem branch, casado só pelo binding.
- **Metadata portátil** (`worktrees`): `id`, `projectId`, `displayName?`, `description?`, `branchHint?`, `sessionId?`, `blockId?`, `state` (`active|frozen|stopped|completed`), `result?`, carimbos. **Eventos portáteis** (`worktree_events`, append-only, UUID próprio, nos moldes de `ddae_events`).
- **Principal:** o checkout principal aparece como primeiro card (como no concept), mas sua metadata é **por Project** (um registro `primary`), sem path. Decisão aberta se pode ser congelada/parada.
- **Nome de exibição:** persistido e opcional; o padrão **derivado** (não gravado) é a branch/nome da pasta.

## 5. Cardinalidade e relações

- **PROJECT → WORKTREE:** 1..N (cada worktree LKR pertence a exatamente 1 Project).
- **SESSION → WORKTREE:** **0..N** (a FK fica na worktree: `sessionId` opcional, muitas worktrees por Session). O concept mostra 1:1 por ilustração; o modelo comporta N (feature + experimento + validação).
- **WORKTREE → SESSION:** 0..1. **Worktree sem Session: SIM** (Git worktrees existem independentemente; o card mostra "DDAE: —", **sem escolher a SESSION-001 por conta própria**).
- **WORKTREE → BLOCK:** 0..1, **opcional**, e o bloco precisa pertencer à Session vinculada. Sem relação real: "Bloco: —". **Não** exibir o bloco atual da Session como se a worktree pertencesse a ele.
- **Criar a relação sem path absoluto:** a relação é `worktreeId ↔ sessionId/blockId` (UUIDs); o path só existe no binding local.

## 6. Ciclo de vida operacional

`ACTIVE ↔ FROZEN`, `ACTIVE → STOPPED`, `FROZEN/STOPPED → ACTIVE`, `qualquer não finalizado → COMPLETED`. Recomendação: **COMPLETED terminal** no MVP (espelha a Session e evita estado ambíguo), registrando `result` opcional. **Independente do estado da Session** (o concept mostra os dois em sincronia, mas herdar automaticamente esconderia casos reais): a UI pode **sinalizar** incoerências (Session finalizada com worktree ainda ATIVA) sem alterar nada.
**Quantos ATIVOS:** o domínio é diferente do DDAE; **não** há regra "uma ativa". Recomendação: **vários ATIVOS por Project e por Session** (decisão aberta, ver D4).

## 7. Discovery, criar, desvincular, remover

- **Discovery** (opções A auto, B adoção, C ao associar, D híbrido): recomendação **D híbrido**. A página lista as worktrees reais do Git (read-only), marcando as **sem metadata** como "não adotadas"; a metadata nasce **ao adotar/associar** ou **ao criar pelo LKR LAB**. Auto-criar metadata para tudo (A) polui o estado portátil com worktrees efêmeras de outras ferramentas; só adoção (B) esconde o que existe.
- **Novo worktree:** reaproveita `create_worktree`, mas o fluxo precisa de: branch nova ou existente, **base branch** (hoje só HEAD), path novo (absoluto só no binding), **Session/Block** opcionais e criação da metadata + binding na mesma operação.
- **Três ações distintas e nunca ambíguas:** **Desvincular do LKR LAB** (remove metadata/binding; Git intacto), **Remover do Git** (`git worktree remove` com as proteções atuais; a branch fica) e **Finalizar** (só estado). O menu do card deve nomear cada uma e jamais combiná-las.
- **Missing:** metadata portátil conhecida sem Git/binding nesta máquina → "Não localizado nesta máquina" (Git e runtime `not_applicable`, sem inventar), com **Localizar** (casar por locator/branch entre `git worktree list`; nunca por palpite), como em Projects.

## 8. Fonte real de cada dado do card

| Dado | Fonte real hoje | Observação |
|---|---|---|
| nome | derivado de branch/pasta | `displayName` persistido: **gap** |
| descrição | — | **gap** (mock) |
| estado operacional | — | **gap** (metadata) |
| branch / HEAD / locked | `git worktree list` | `prunable`/`bare`/motivo do lock não lidos |
| path | `git worktree list` | só exibição, nunca portátil |
| Git (Clean/Alterações) | `real_git(path)` | por path, read-only |
| runtime | `runtime::detect(path)` + fatos de processo | atribuição por worktree: **gap** |
| sessão DDAE / bloco | — | **gap** (metadata) |
| resultado | — | **gap** |
| última atividade | — | recomendação: último **evento operacional** (`worktree_events`); commit mais recente (`git log -1`) como dado Git **separado**; nunca mesclar |
| HEAD copiar | cliente | — |

**Dados do mock que não existem no backend:** nomes/descrições, estado operacional, vínculo Session/Bloco, resultado, última atividade, "Fechamento / validação" (e a contagem 7/10: a SESSION-001 real está em **9/10**, o concept precisa ser lido como ilustrativo). **Divergência visual a decidir:** o Concept 08 pinta PARADO em **âmbar**; o Concept 06 implementado usa **slate**. Unificar antes da UI.

## 9. Resumo e filtros

Contadores: total, Ativos, Congelados, Parados, Finalizados, com alterações Git. **A soma dos estados precisa fechar com o total:** como worktrees não adotadas não têm estado, recomenda-se um contador próprio **"Não adotadas"** (ou contar só as adotadas no total operacional e mostrar as não adotadas à parte). Filtros: busca (nome, branch, Session), **operacional** (Todos/Ativos/Congelados/Parados/Finalizados) **separado** do **Git** (Todos/Clean/Alterações). Sem filtros enterprise.

## 10. Eventos

- **`worktree_events`** portátil e append-only é recomendado (adotada, estado alterado, vinculada/desvinculada, finalizada com resultado), não parsing de `activities`.
- **Associar/desassociar uma Worktree a uma Session** é mudança semântica **da Session**: deve gerar também um `ddae_event` (novos tipos, ex.: worktree vinculada/desvinculada). Cuidado: `EventType` é um enum **fechado** em Rust e JS; novos tipos são mudança de schema (leitores antigos recusariam).
- `activities` continua para telemetria do Project e para alimentar o Project Control Center.

## 11. Integrações futuras

- **Rota:** o Concept 08 é um **hub/lista** em `#project/<id>/worktrees`; **sem rota de detalhe** (não há desenho). Ação "Abrir worktree" = launcher (pasta/terminal/editor).
- **Project Control Center:** o card Worktrees passa de "N worktrees Git" para metadata operacional + fatos Git (contagens por estado, com alterações). A Próxima ação **não** muda de prioridade.
- **Session Detail:** novo card "Worktrees relacionados" (por `sessionId`), e a aba/lista existente passa a poder exibir a relação real.

## 12. Migration e workspace

- **Migration mínima provável (`009`):** `worktrees` (id, project_id FK CASCADE, display_name, description, branch_hint, session_id FK SET NULL, block_id, state CHECK, result, timestamps), `worktree_bindings` (worktree_id FK, `local_path` UNIQUE — classe C), `worktree_events` (append-only). Nada de `scope`.
- **Workspace v4** provável: `worktrees` + `worktreeEvents` portáteis; **sem path**. v1/v2/v3 continuam válidos (campos ausentes = vazios) e normalizam para v4; mesmo padrão aditivo da v3 (Rust + JS espelhados, golden). Impacto no `ddae` se vierem novos tipos de evento da Session.
- **Sync:** sem merge por worktree; a proteção de divergência do workspace continua o mecanismo.

## 13. Lacunas, riscos e decisões abertas

**Lacunas:** ID estável; locator; binding; metadata operacional; vínculo Session/Bloco; resultado; eventos; leitura de `prunable`/`bare`/motivo de lock; atribuição de runtime por worktree; base branch e branch existente na criação; "desvincular" inexistente.
**Riscos:** (1) confundir FINALIZADO/desvincular/remover (precisa de rótulos e confirmações separados); (2) usar branch como chave; (3) path vazando para o workspace; (4) `EventType` fechado ao estender eventos da Session; (5) detached/bare tratados como branch vazia; (6) runtime por worktree mal atribuído ao Project; (7) auto-adoção poluindo o estado portátil; (8) manter a página 100% read-only na abertura.
**Decisões abertas:**
- **D1** Discovery híbrido (recomendado) vs automático vs só adoção.
- **D2** Metadata da `main`: registro por Project; pode ser congelada/parada?
- **D3** COMPLETED terminal (recomendado)?
- **D4** Vários ATIVOS por Project/Session (recomendado)?
- **D5** Estado independente da Session, com sinalização de incoerência (recomendado)?
- **D6** Relação com Bloco opcional (recomendado) e ligada à Session.
- **D7** `worktree_events` portáteis + `ddae_event` ao associar (recomendado).
- **D8** Cor de PARADO: âmbar (concept 08) ou slate (06).
- **D9** Fluxo de "Novo worktree" (base, branch existente, Session/Bloco).
- **D10** Alterações Git em worktree PARADA/FINALIZADA: só alerta (recomendado) ou bloqueia?

## 14. Testes futuros (a escrever na implementação)

Parser de `git worktree list --porcelain` (principal, detached, bare, locked com motivo, prunable); identidade (UUID estável, branch renomeada, path diferente entre máquinas); binding só local; Missing em outra máquina; adoção e criação (metadata + binding atômicos); relação Session/Bloco (bloco de outra Session recusado); transições e terminalidade; FINALIZAR não toca no Git; desvincular ≠ remover; remover mantém as proteções (principal, locked, sujo, sem `--force`); abertura passiva (nenhum comando mutável); contadores fechando com o total; filtros operacional × Git; eventos append-only/portáteis/sem dado local; `ddae_event` ao associar; workspace v1/v2/v3/v4 e hash determinístico; sem path no estado portátil; render do card com "DDAE: —"/"Bloco: —" e do estado Missing; Project Control Center e Session Detail.

## 15. Fora do escopo

Concept 09 (Planejamento); IA/agentes; merge/push/checkout automáticos; CRDT/merge por worktree; detalhe de worktree.
