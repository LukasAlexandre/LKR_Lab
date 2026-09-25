# Product specification

## Problema e objetivo

O contexto de desenvolvimento está dividido entre repositórios, terminal, processos, GitHub e agentes. O hub relaciona essas fontes por projeto local. Windows é o primeiro alvo; uso pessoal/interno, com separação de providers para futura evolução comercial.

O briefing integral autorizado está em [USER_BRIEF.md](USER_BRIEF.md). Este documento descreve o recorte implementado; não substitui requisitos futuros.

## Modelo de experiência

Sidebar fixa, dark navy, densidade alta, painéis compactos, estados semânticos. Primeiro uso orienta selecionar uma pasta existente. Nenhum scan de disco automático. A descoberta preenche sugestão de nome, stack e remote; salvar é uma ação separada de confirmação.

Rotas: Dashboard, Projetos, Repositórios, IA / Agents, Prompts, Portas, Processos, Git / PRs, Worktrees, Ambientes, Conhecimento, Terminal, Configurações. Rotas futuras apresentam escopo e ausência de implementação claramente.

## Fluxos v0.1

1. Adicionar pasta → detectar → revisar → salvar no SQLite.
2. Selecionar projeto → consultar Git → abrir ferramentas no diretório.
3. Atualizar ambiente → consultar ferramentas, métricas, portas e processos.
4. Consultar GitHub explicitamente → listar PRs e issues via gh autenticado.
5. Escolher template e projeto → renderizar → copiar; editar/salvar global ou local.
6. Gerar contexto → revisar → copiar ou escolher destino para Markdown.
7. Remover cadastro ou encerrar processo → confirmação explícita.

## Semântica dos dados

- Available significa executável encontrado, não daemon saudável nem usuário autenticado.
- Connected no GitHub depende de `gh auth status` com sucesso.
- Porta observada significa socket ocupado, não HTTP saudável.
- ExpectedBy significa declaração do usuário; projectId só é inferido pelo CWD do processo.
- Conflito: múltiplos projetos declarando a porta ou CWD identificado em projeto diferente do esperado. Propriedade desconhecida permanece desconhecida.
- Git ahead/behind usam refs locais; não há fetch automático.
- Não existe score de health arbitrário.

## Critérios de aceitação

Persistência após reiniciar, validação de entradas no backend, erros visíveis, ausência de operações Git mutantes, confirmações destrutivas e ausência de dados fictícios. Verificação em Windows é obrigatória antes de declarar o aplicativo desktop pronto.
