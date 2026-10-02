# Concept 09 — Planejamento

**Status:** APROVADO (direção visual)
**Sessão DDAE:** [SESSION-001](../../ddae/sessions/SESSION-001-machine-context-workspace-foundation.md)
**Imagem canônica:** [concept-09-planning.webp](concept-09-planning.webp)
**Anterior:** [Concept 08](CONCEPT-08.md)

![Concept 09](concept-09-planning.webp)

## Objetivo

Definir a página **Planejamento** do Project Control Center: a área para organizar o que está planejado para o sistema e transformar planos em execução, ligando itens planejados às sessões DDAE e às worktrees.

## Conteúdo do concept

- **Topbar:** breadcrumb Workspace › Projetos › LKR LAB › Planejamento. Rótulo da página "Projeto".
- **Cabeçalho:** título "Planejamento", subtítulo "Organize o que vem a seguir e transforme planos em execução", ações **Atualizar**, **Novo item** (com menu) e menu.
- **Cards de resumo:** total de itens, Backlog, Em execução, Concluídos, com Session DDAE e sem Session.
- **Filtros:** busca, filtro por status (Todos, Backlog, Em execução, Concluídos), por vínculo (Com Session, Sem Session), por Prioridade, Área e "Mais filtros".
- **Tabela agrupada por status**, com grupos recolhíveis: Em execução, Backlog e Concluídos, cada um com contador. Colunas: Item e descrição, Status, Prioridade (Alta, Média, Baixa), **Session DDAE**, **Worktree**, Última atualização, responsável e Ações.
- **Item em execução:** mostra a sessão vinculada com o progresso em blocos e a worktree, com link para abri-la.
- **Item de backlog:** sem sessão nem worktree, com a ação **Criar Session**.
- **Item concluído:** mantém o vínculo com a sessão, com o progresso registrado, e com a worktree ou branch onde foi feito.

## Regras canônicas

1. Planejamento é uma **área do projeto** para organizar o que está planejado para o sistema: backlog, itens, ideias e próximos passos.
2. O ciclo do item é: **Backlog → Em execução → Concluído**. O ciclo vem do fluxo definido na [SESSION-001](../../ddae/sessions/SESSION-001-machine-context-workspace-foundation.md): item planejado → criar Session DDAE → Session ativa → conclusão → item concluído.
3. **Criar Session** é a ação que transforma um item de backlog em trabalho: cria uma sessão DDAE vinculada ao item ([Concept 06](CONCEPT-06.md)) e o item passa a Em execução.
4. Um item em execução tem **sessão DDAE** e **worktree** vinculadas ([Concept 08](CONCEPT-08.md)), formando a cadeia item de planejamento → sessão DDAE → worktree.
5. Itens podem existir **sem sessão**. O concept conta e filtra os itens "com Session" e "sem Session".
6. Cada item tem **prioridade** (Alta, Média, Baixa), **área** e responsável.
7. Os agrupamentos usam o mesmo sistema de estados e cores do restante do LKR LAB. Apenas o que está ativo ganha destaque forte.
8. Status do item, vínculo e prioridade são **metadata operacional do LKR LAB**, não dados do Git.
9. O vínculo com worktree é uma referência. O path e o estado Git continuam sendo da máquina ([Concept 01](CONCEPT-01.md), [Concept 08](CONCEPT-08.md)).

> Nota: os valores do concept (itens, descrições, branches, contagens, horários) são ilustrativos da referência visual, não dados reais nem contrato de campos. Os itens de exemplo incluem sessões, progressos e worktrees que não existem no registro atual.

## Observações para a implementação futura

- **Mesma sessão em vários itens:** no concept, os três itens em execução e os concluídos apontam para a mesma SESSION-001 com progressos diferentes (8 / 10 e 1 / 10, 2 / 10, 3 / 10). Isso contradiz "uma sessão representa uma feature" ([Concept 07](CONCEPT-07.md)) e o modelo de um item por sessão. É preciso definir se vários itens podem compartilhar uma sessão e como o progresso de cada um é medido.
- **Progresso nos concluídos:** itens concluídos mostram progresso parcial (1 / 10, 2 / 10, 3 / 10) vinculado à sessão. Falta definir o que significa "progresso do item" quando o item é concluído.
- **Contagens do resumo:** o card mostra 12 itens no total, mas Backlog (4) + Em execução (3) + Concluídos (3) somam 10. Os cards "5 com Session DDAE" e "2 sem Session" somam 7, e o concept exibe 6 itens com sessão e 4 sem sessão. Falta definir a regra de contagem (itens arquivados, outros status ou apenas dados ilustrativos).
- **Origem dos itens:** o concept mostra "Novo item" com menu, mas o conteúdo do menu e o formulário de criação não foram desenhados. Não está definido se itens vêm também de outras fontes (TASKS.md, issues).
- **Relação com `TASKS.md`:** o repositório já tem `TASKS.md` e `docs/ROADMAP.md`. Falta decidir se o Planejamento substitui, importa ou coexiste com esses arquivos.
- **Armazenamento:** onde ficam os itens (banco local, arquivo portátil, ou ambos) segue sem decisão.
- **Área:** os valores possíveis do filtro de área não estão definidos.
- **Responsável:** o concept mostra um avatar por item. Não está definido o modelo de usuários, já que o produto é local-first e hoje de um só usuário.

## Fim do roadmap de concepts

Com este concept, os 9 concepts do roadmap estão aprovados. A SESSION-001 permanece **ATIVA**; os próximos passos são o fechamento/validação dos concepts (consolidar as decisões em aberto) e a decisão de como e em que ordem implementar.

## Fora de escopo

Este registro é documental. Nada aqui implementa a página, os itens, o vínculo com sessões e worktrees, banco, APIs ou bridge.
