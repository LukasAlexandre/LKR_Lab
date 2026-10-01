# ADR-004 — Estado portátil reconciliado por base

Status: accepted.

## Contexto

O Lab Setup guardava o checklist só no localStorage; `data/lab-setup.json` era um backup que o usuário precisava restaurar manualmente. Em outra máquina (ou outro endereço do navegador) o ambiente não voltava sozinho. O desktop tinha preferências espalhadas em chaves soltas do localStorage, sem distinguir o que é da máquina.

## Decisão

- Cada módulo tem três camadas: **cache local** (autosave instantâneo, offline), **arquivo portátil** versionado em `data/<módulo>.json` e **metadados desta máquina**. Ver [STATE.md](../STATE.md).
- A reconciliação compara o hash do cache, o hash do arquivo e a **base** (hash do último conteúdo portátil reconciliado). Com isso distingue "o repositório mudou" de "eu mudei aqui" sem depender de relógios. Adota automaticamente só quando nada local se perde; caso contrário, sinaliza pendência ou conflito.
- Publicar continua explícito (sync → commit de um único arquivo → push). Nenhum autosave gera commit nem altera a árvore de trabalho.
- O arquivo portátil contém só estado semântico validado (nunca uma cópia do localStorage); filtros, metadados de sync e "desfazer" ficam na máquina.
- No desktop, toda preferência de interface passa por `src/shared/preferences.ts`, com schema, validação, migração das chaves antigas e escopo declarado por campo. Observações da máquina (ex.: `pathAvailable`) são calculadas a cada leitura e nunca persistidas.

## Alternativas descartadas

- **Gravar o arquivo do repositório a cada edição**: deixaria a árvore sempre suja e travaria o "Verificar atualização" (fast-forward recusa arquivo modificado) justamente quando há edições locais e o remoto avançou.
- **Comparar timestamps**: relógios de máquinas diferentes não são confiáveis e "mais novo" não significa "sem perda".
- **Copiar o localStorage inteiro para JSON**: levaria estado de máquina e lixo para o Git.

## Consequências

- Edições ainda não sincronizadas vivem no cache do navegador até o sync (limpar os dados do navegador as perde; Exportar continua disponível).
- A exportação portátil do workspace do desktop (projetos sem caminho, prompts, knowledge) reutiliza a mesma ideia e é o próximo bloco.
