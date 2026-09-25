Quero iniciar o desenvolvimento de um novo software desktop chamado provisoriamente **LK Dev Hub**, da **LK Technologies Brasil**.

Você está no modo Work. Atue como **Product Architect + Staff Software Engineer + UX Engineer + Security Engineer**, e conduza o trabalho de forma prática.

NÃO quero apenas uma análise ou um plano conceitual.

Quero que você:

1. Estruture o produto.
2. Defina a arquitetura.
3. Crie a documentação.
4. Crie o projeto.
5. Implemente a fundação.
6. Implemente o máximo possível do MVP nesta sessão.
7. Teste o que for possível testar.
8. Registre claramente o que foi concluído, o que ficou pendente e quais são os próximos passos.

Se alguma etapa não puder ser executada no ambiente do Work por depender diretamente do meu computador Windows, não finja que executou. Prepare a implementação, testes e comandos necessários e marque explicitamente a validação local como pendente.

---

# 1. VISÃO DO PRODUTO

O LK Dev Hub será um **Developer Control Center / Development Operating System local**.

Não quero apenas um gerenciador de repositórios.

A aplicação deve entender a relação entre:

- projetos;
- repositórios;
- Git;
- GitHub;
- branches;
- worktrees;
- processos;
- portas;
- serviços locais;
- ambientes;
- Claude Code;
- Codex e outros agentes futuramente;
- skills;
- MCP servers;
- prompts;
- documentação;
- Obsidian;
- bancos;
- Docker;
- terminal;
- sessões de IA;
- contexto de desenvolvimento.

Hoje essas informações ficam espalhadas entre terminal, VS Code, GitHub, Claude Code, Obsidian, Docker e minha memória.

O LK Dev Hub deve centralizar esse contexto.

Conceitualmente:

> O GitHub conhece os repositórios.
> Docker conhece os containers.
> VS Code conhece o código.
> Claude conhece a sessão de IA.
> Obsidian conhece a documentação.
>
> O LK Dev Hub deve conhecer **como todas essas coisas fazem parte de um mesmo projeto**.

O produto é inicialmente uma ferramenta pessoal/interna, mas a arquitetura deve permitir que futuramente ele possa se tornar um produto comercial da LK Technologies.

---

# 2. PLATAFORMA

Prioridade absoluta inicial:

**Windows Desktop.**

Arquitetura desejada:

- Tauri 2
- React
- TypeScript
- Vite
- Rust
- SQLite

Antes de criar o projeto, verifique as versões estáveis atuais das tecnologias e utilize versões compatíveis entre si.

Não use Electron salvo se surgir um impedimento técnico concreto e documentado que justifique abandonar Tauri.

Para a UI, escolha uma solução moderna e sustentável, sem transformar o projeto em uma coleção excessiva de dependências.

Rust deve ser responsável principalmente pelas integrações nativas com:

- filesystem;
- processos;
- portas;
- sistema operacional;
- Git;
- comandos locais;
- SQLite;
- segurança;
- comunicação controlada com CLIs.

React/TypeScript deve ser responsável pela interface e estado de apresentação.

---

# 3. IDENTIDADE VISUAL

Existe uma imagem conceitual do LK Dev Hub anexada a esta tarefa.

Use-a como **referência visual principal**, não como mockup descartável.

Quero preservar a direção apresentada nela:

- dark mode;
- azul escuro/navy;
- azul/ciano discreto como destaque;
- interface profissional;
- sidebar fixa à esquerda;
- alta densidade de informações;
- cards compactos;
- cantos discretamente arredondados;
- foco em legibilidade;
- aparência de ferramenta de engenharia;
- indicadores verde/amarelo/vermelho somente quando semanticamente necessários;
- dashboard operacional;
- pouco espaço desperdiçado;
- visual premium, mas não extravagante.

Evite:

- glassmorphism exagerado;
- animações inúteis;
- gradientes excessivos;
- cards gigantes;
- interface parecida com landing page;
- excesso de ícones;
- aparência gamer.

A aplicação deve parecer uma ferramenta profissional utilizada diariamente por um desenvolvedor.

---

# 4. NAVEGAÇÃO PRINCIPAL

A sidebar deve prever:

- Dashboard
- Projetos
- Repositórios
- IA / Agents
- Prompts
- Portas
- Processos
- Git / PRs
- Worktrees
- Ambientes
- Conhecimento
- Terminal
- Configurações

A arquitetura das rotas deve permitir adicionar outros módulos futuramente.

---

# 5. DASHBOARD

A home deve funcionar como cockpit do ambiente.

Ela deve possuir inicialmente:

## Resumo do ambiente

Mostrar estado de integrações como:

- GitHub
- Git
- Claude
- Docker
- MySQL
- Obsidian

Estados:

- conectado;
- disponível;
- indisponível;
- não configurado.

Não inventar conexões.

Detectar quando tecnicamente possível.

---

## Performance do sistema

Exibir:

- CPU;
- memória;
- disco;
- eventualmente rede.

Não precisa ser extremamente detalhado no MVP.

---

## Projetos recentes

Cards compactos como:

LK Wallet

Frontend :3004
Backend :4004

Branch atual

Repository

Stack

Status dos serviços

Ações rápidas:

- abrir projeto;
- abrir terminal;
- abrir VS Code;
- abrir Claude;
- abrir GitHub.

---

## Monitor de portas

Exemplo:

\| Porta | Processo | Projeto | PID | Status |
\| 3004 | node.exe | LK Wallet | 18344 | Ativo |
\| 3333 | node.exe | Vaultify | 9321 | Ativo |
\| 3306 | mysqld.exe | MySQL | 3121 | Ativo |
\| 5173 | node.exe | Desconhecido | 10442 | Atenção |

---

## Git/GitHub

Exibir dados como:

- branch atual;
- working tree;
- ahead;
- behind;
- PR associado;
- CI;
- review;
- mergeability.

Exemplo:

PR #111

CI: PASS
Review: PASS
Branch: feat/wallet-login-premium
Working tree: CLEAN

---

## IA

Mostrar:

- skills disponíveis;
- MCP servers;
- contexto do projeto;
- prompts rápidos;
- botão para abrir projeto no Claude Code;
- botão para gerar contexto.

---

## Sessões de IA

Planejar suporte para visualizar sessões relacionadas aos projetos.

Exemplo:

LK Wallet — Login Premium
Status: ativa

ICT — T006 Audit
Status: ativa

LK SEO
Status: encerrada

Cada sessão deve poder ter relação com:

- projeto;
- repositório;
- branch;
- worktree;
- agente;
- horário.

Não dependa de scraping instável para implementar essa função.

Crie uma abstração/provider para agentes.

Claude deverá ser o primeiro provider.

Se for possível detectar de forma documentada e segura qual conta do Claude CLI está conectada, implemente.

Caso contrário, modele a funcionalidade e documente a limitação.

NÃO tente extrair tokens, cookies, credenciais ou dados privados de maneira insegura.

---

# 6. PROJECT REGISTRY

Esse é um dos componentes centrais.

Quero poder cadastrar projetos.

Modelo conceitual:

Project

- id
- name
- slug
- description
- localPath
- repository
- provider
- stack
- tags
- createdAt
- updatedAt

Um projeto também pode possuir:

ports
services
commands
environments
documentation
agents
skills
MCP servers
prompts

Exemplo conceitual:

name:
LK Wallet

repository:
github.com/.../lk-wallet

local\_path:
C:/Projetos/LK-Wallet

description:
Sistema financeiro da LK Technologies

stack:

- React
- Node
- MySQL

ports:

frontend:
3004

backend:
4004

commands:

dev:
npm run dev

test:
npm test

lint:
npm run lint

documentation:

vault:
LK-Technologies-Brasil-Vault

---

# 7. PROJECT DETAILS

Ao abrir um projeto, quero uma visão central contendo:

## Overview

Nome
Descrição
Repository
Local Path
Stack

## Git

Branch
HEAD
Working tree
Ahead
Behind
Commits recentes

## Services

Frontend
Backend
Banco
Outros

## Ports

Portas esperadas versus portas realmente encontradas.

## GitHub

PRs
Issues
CI

## AI

Skills
Prompts
MCP
Sessions

## Environment

Variáveis necessárias e status.

## Knowledge

Documentos relacionados.

---

# 8. PROJECT HEALTH

Criar um conceito de **Project Health**.

Exemplo:

Repository

✓ Git encontrado
✓ Origin configurado
✓ GitHub acessível

Git

Branch: feat/wallet-login-premium
Ahead: 2
Behind: 0
Working tree: CLEAN

Environment

✓ .env.local
✓ Node installed
✓ npm installed

Services

● Frontend :3004
● Backend :4004
● MySQL :3306

GitHub

PR #111
CI: PASS
Review: PASS
Mergeable: CLEAN

Não criar inicialmente um score arbitrário de 0–100.

Prefira checks concretos.

---

# 9. PORT MONITOR

Quero um módulo completo de monitoramento de portas.

Ele deve detectar:

- porta;
- protocolo quando relevante;
- PID;
- processo;
- executable;
- projeto relacionado, quando puder ser inferido;
- status.

Ações:

- abrir localhost;
- abrir terminal;
- mostrar processo;
- associar processo/prota a projeto;
- encerrar processo;
- eventualmente reiniciar serviço.

Ação destrutiva como `Kill process` deve exigir confirmação explícita.

Criar mecanismo para detectar conflito.

Exemplo:

PORT CONFLICT

LK Wallet requires:

:3004

PID 19432 is already using it.

Process:

node.exe

Possible project:

Old LK Wallet session

Ações:

- Show process
- Kill process
- Ignore

Nunca mate automaticamente um processo.

---

# 10. PROCESS MANAGER

Além de portas, criar base para mostrar processos de desenvolvimento.

Filtros:

- todos;
- Node;
- Rust;
- Python;
- Java;
- Docker;
- banco;
- processos pertencentes a projetos.

Relacionar processo ao projeto quando houver evidência suficiente.

Não usar heurística perigosa como verdade absoluta.

Mostrar nível de confiança quando necessário.

---

# 11. LOCAL SERVICES

Um projeto poderá declarar seus serviços.

Exemplo:

LK System

Frontend ● Running
Backend ● Running
Database ● Running

Ações:

START ALL
STOP ALL
RESTART
OPEN APP

Comandos poderão utilizar:

- npm
- pnpm
- bun
- cargo
- docker compose
- python
- java
- clasp

A arquitetura deverá permitir providers de comandos.

Não executar comandos arbitrários escondidos.

O usuário deve conseguir visualizar qual comando será executado.

---

# 12. GIT INTEGRATION

Inicialmente priorize Git CLI instalado localmente.

Detectar:

- repository;
- branch;
- HEAD;
- status;
- staged;
- unstaged;
- untracked;
- upstream;
- ahead;
- behind;
- commits recentes;
- remotes.

Não realizar automaticamente:

- commit;
- push;
- reset;
- checkout destrutivo;
- merge;
- rebase;
- force push.

Essas ações podem existir futuramente com confirmação explícita.

---

# 13. GITHUB INTEGRATION

Inicialmente prefira integração através do:

`gh`

GitHub CLI.

Detectar se:

- gh está instalado;
- usuário está autenticado;
- repository possui GitHub remote.

Obter quando possível:

- PR atual;
- PRs abertas;
- issues;
- checks;
- CI;
- review;
- mergeability.

A arquitetura deve possuir um GitProvider para futuramente permitir:

- GitHub API;
- GitLab;
- Bitbucket.

Não faça a aplicação depender estruturalmente do GitHub.

---

# 14. WORKTREE MANAGER

Quero suporte forte para Git worktrees.

Mostrar:

Repository

main
C:/repos/project

audit-t006
C:/repos/project-audit-t006

feature-x
C:/repos/project-feature-x

Ações:

- Create Worktree
- Open Terminal
- Open VS Code
- Open Claude
- Remove Worktree

Antes de remover:

verificar working tree.

Se houver alterações:

bloquear ou exigir confirmação extremamente explícita.

Nunca remover automaticamente worktrees com alterações.

---

# 15. ENVIRONMENT MANAGER

O sistema poderá validar variáveis de ambiente.

Exemplo:

DATABASE\_URL ✓ configured
JWT\_SECRET ✓ configured
AWS\_ACCESS\_KEY ✓ configured
SMTP\_PASSWORD ✕ missing

IMPORTANTE:

Não mostrar secrets por padrão.

Não registrar secrets em:

- SQLite;
- logs;
- telemetry;
- crash reports.

Inicialmente prefira armazenar apenas:

- nome da variável;
- required;
- configured;
- source.

Se futuramente houver gerenciamento de segredo real, projetar integração com mecanismo seguro do sistema operacional.

Nunca inventar um vault criptográfico próprio sem necessidade.

---

# 16. AI / AGENT WORKSPACE

Esse é um dos principais diferenciais.

Cada projeto deve possuir um **AI Context Pack**.

Estrutura conceitual:

AI

CLAUDE.md
AGENTS.md

Skills

github-audit
code-review
security-review
pre-deploy

Prompts

audit-project
investigate-bug
create-pr
continue-session
production-check

MCP Servers

filesystem
github
database
etc.

O LK Dev Hub precisa saber:

- quais agentes estão disponíveis;
- quais skills estão disponíveis;
- quais MCPs pertencem ao projeto;
- quais prompts pertencem ao projeto;
- quais instruções de IA pertencem ao projeto.

Crie uma arquitetura extensível.

Algo conceitualmente como:

AgentProvider

ClaudeProvider
CodexProvider
FutureProvider

Não codifique toda a aplicação diretamente para Claude.

---

# 17. PROMPT LIBRARY

Quero transformar prompts em recursos reutilizáveis.

Categorias iniciais:

Development

- Audit repository
- Investigate bug
- Implement feature
- Refactor safely

GitHub

- Review PR
- Prepare PR
- Check CI
- Investigate issue

Security

- Dependency audit
- Threat analysis
- Secret scan

Deployment

- Pre-deploy
- Production gate
- Smoke test

Prompts poderão ser globais ou por projeto.

Implementar suporte a templates.

Variáveis conceituais:

{{project.name}}

{{project.path}}

{{git.branch}}

{{git.head}}

{{github.pullRequest}}

{{project.stack}}

{{project.instructions}}

O sistema poderá gerar um prompt final utilizando contexto atual.

---

# 18. PROJECT SNAPSHOT

Quero uma feature chamada:

**Generate Development Context**

Ela deve reunir automaticamente:

Repository
Current branch
HEAD
Git status
Recent commits
Open PR
Issues relevantes
Running services
Ports
Installed skills
MCP servers
Project documentation
TODOs
Environment health

E gerar um documento semelhante a:

# Project Development Context

Project:
...

Repository:
...

Current Branch:
...

HEAD:
...

Current State:
...

Services:
...

Open PR:
...

Pending Tasks:
...

Development Rules:
...

Esse resultado deve poder ser:

- visualizado;
- copiado;
- salvo;
- enviado para um agente futuramente.

Ele será um dos principais recursos para evitar perda de contexto entre sessões de IA.

---

# 19. KNOWLEDGE / OBSIDIAN

Quero poder associar projetos a diretórios do Obsidian ou documentação normal.

Categorias:

Planning
Architecture
Decisions
Current Status
Changelog
Issues
Investigations

Permitir ações:

- Open file
- Open folder
- Open in Obsidian

Não crie dependência obrigatória de Obsidian.

O backend deve considerar esses arquivos simplesmente como Knowledge Sources, com Obsidian funcionando como uma integração opcional.

---

# 20. TERMINAL

Planejar terminal integrado.

Sugestão:

xterm.js ou solução equivalente.

Porém, terminal completo NÃO deve atrasar o MVP.

Para o primeiro milestone é aceitável:

- Open Windows Terminal;
- Open PowerShell;
- Open terminal at project;
- Open terminal at worktree.

Criar abstração para futuramente incorporar terminal real.

---

# 21. COMMAND PALETTE

Implementar ou planejar desde cedo:

Ctrl + K

Busca global por:

- projetos;
- comandos;
- arquivos;
- prompts;
- ações;
- repositories.

Exemplo:

> Open LK Wallet

> Start LK Wallet

> Open GitHub

> Generate Context

> Audit Repository

Isso deve virar futuramente uma das principais formas de navegar pela aplicação.

---

# 22. ACTIVITY LOG

Criar um histórico operacional local.

Exemplos:

14:32
Port 3004 opened
LK Wallet

13:18
PR #111 checked
CI passed

11:45
Development context generated
LK Wallet

10:22
MySQL started

Não registrar:

- secrets;
- tokens;
- passwords;
- conteúdo sensível desnecessário.

---

# 23. BANCO LOCAL

Usar SQLite.

Projete um schema limpo.

Entidades candidatas:

projects
repositories
project\_ports
project\_services
project\_commands
project\_environments
project\_environment\_keys
skills
project\_skills
mcp\_servers
project\_mcp\_servers
prompt\_templates
project\_prompts
agent\_sessions
knowledge\_sources
worktrees
activities
settings

Não crie tabelas desnecessárias apenas porque estão listadas acima.

Normalize onde fizer sentido, mas não transforme um aplicativo desktop em um ERP.

Crie migrations.

---

# 24. SEGURANÇA

Considere segurança desde o início.

Regras:

1. Nunca armazenar secrets em plaintext sem necessidade.
2. Nunca registrar tokens.
3. Não mostrar conteúdo de `.env` inteiro.
4. Não executar comandos destrutivos automaticamente.
5. Confirmar operações como:
   - kill process;
   - delete worktree;
   - delete project;
   - stop all services.
6. Projetos poderão conter comandos customizados. Trate-os como comandos potencialmente perigosos.
7. Não criar shell commands concatenando input sem sanitização.
8. Isolar frontend de operações privilegiadas.
9. Criar allowlist clara de Tauri commands.
10. Não habilitar capabilities/permissões Tauri mais amplas que o necessário.
11. Aplicar princípio de least privilege.
12. Documentar threat model inicial.

---

# 25. ARQUITETURA INTERNA

Evite um arquivo Rust gigantesco contendo tudo.

Separe aproximadamente por domínios:

Rust:

system
processes
ports
git
github
projects
database
agents
commands
knowledge
settings

Frontend:

features/dashboard
features/projects
features/ports
features/git
features/agents
features/prompts
features/worktrees
features/settings

shared/components
shared/hooks
shared/types
shared/services

Não siga essa estrutura cegamente se encontrar arquitetura melhor.

Quero separação de responsabilidades e baixo acoplamento.

---

# 26. PROVIDERS

Onde houver integrações externas, prefira abstrações.

Exemplos:

GitHostingProvider

GitHubProvider

AgentProvider

ClaudeProvider
CodexProvider

KnowledgeProvider

FilesystemKnowledgeProvider
ObsidianProvider

TerminalProvider

WindowsTerminalProvider

A abstração deve ser proporcional ao problema.

Não crie arquitetura enterprise artificial para um MVP.

---

# 27. WINDOWS INTEGRATION

Investigue e implemente da maneira mais segura possível:

Process listing

Port detection

Open URL

Open folder

Open terminal

Open VS Code

Git detection

GitHub CLI detection

Claude CLI detection

Docker detection

Node detection

Rust detection

Python detection

MySQL detection quando possível.

Quando houver diferença entre PowerShell, CMD e APIs nativas, prefira solução estável.

Evite parsear output localizado do Windows se existir API Rust confiável para obter a informação.

---

# 28. MVP v0.1

Quero que o primeiro MVP tenha prioridade nestes módulos:

## 1 — Foundation

Tauri
React
TypeScript
Rust
SQLite
Routing
Design system
App layout

## 2 — Projects

Cadastrar projeto
Editar projeto
Remover projeto com confirmação
Local path
Repository
Description
Stack
Ports
Commands

## 3 — Project Overview

Git state
Services
Ports
Quick actions

## 4 — Ports

Detectar portas
PID
Process
Associate project
Open localhost
Kill com confirmação

## 5 — Git

Branch
Status
HEAD
Ahead/behind
Recent commits

## 6 — GitHub

gh detection
Authentication state
PR
CI/checks básicos

## 7 — Launchers

Open folder
Open terminal
Open VS Code
Open Claude Code

## 8 — Prompts

Prompt templates
Project prompts
Copy generated prompt

## 9 — Development Context

Generate Project Snapshot

---

# 29. NÃO BLOQUEAR O MVP COM

Não priorize ainda:

- cloud sync;
- multi-user;
- mobile;
- remote server;
- collaboration;
- marketplace;
- telemetry SaaS;
- billing;
- Kubernetes;
- plugin marketplace completo;
- sistema complexo de permissões;
- terminal embutido completo;
- execução autônoma de agentes;
- orchestration multi-agent;
- editor de código próprio.

Esses itens pertencem ao futuro.

---

# 30. ROADMAP CONCEITUAL

Depois do v0.1:

## v0.2

Skills
MCP management
Worktrees
Environment validation
Project Health

## v0.3

AI Sessions
Agent Providers
Docker monitoring
Project Snapshot avançado

## v0.4

Obsidian / Knowledge
Automated documentation
Development gates

## v0.5

Agent execution
Task orchestration
Multi-agent workflows

Essa ordem pode ser ajustada se houver justificativa técnica forte.

---

# 31. EXEMPLOS REAIS DE PROJETOS

O software será utilizado em projetos como:

- LK System
- LK Wallet
- Vaultify
- LK Clients
- LK Board
- HUB Troca de Turno
- Credenciamento de ICTs

Portanto, o sistema precisa suportar projetos bastante diferentes:

- React;
- Node;
- APIs;
- MySQL;
- AWS;
- Vercel;
- Render;
- Apps Script;
- Solidity/Web3;
- documentação;
- automações.

Não crie regras específicas exclusivamente para um projeto.

---

# 32. PRIMEIRO USO

Pense no onboarding.

Primeira abertura:

Welcome to LK Dev Hub

Ações:

Add Existing Project
Scan Development Folder
Configure GitHub
Configure AI Tools

Para `Scan Development Folder`, futuramente deve ser possível selecionar algo como:

C:/Projetos/

E detectar possíveis repositories através de `.git`.

Não escaneie todo o disco automaticamente.

---

# 33. EXPERIÊNCIA DE CADASTRO

Ao adicionar projeto:

Select Folder

Depois detectar automaticamente:

- `.git`;
- remote;
- package.json;
- Cargo.toml;
- requirements.txt;
- docker-compose;
- stack provável.

Mostrar resultado para confirmação antes de salvar.

Exemplo:

Detected:

React
Node.js
TypeScript
Git
GitHub

Repository:

github.com/lukas/lk-wallet

Suggested name:

LK Wallet

O usuário confirma.

---

# 34. ERROR HANDLING

Não esconda erros.

Quero mensagens úteis.

Ruim:

Something went wrong.

Bom:

Git executable was not found.

Expected:

git.exe

You can install Git or configure its path in Settings.

Logs técnicos podem existir separadamente.

---

# 35. TESTES

Crie estratégia de testes desde o início.

Frontend:

unit tests para lógica relevante.

Rust:

unit tests para parsers e services.

Integration:

Git fixture repository.

SQLite:

migration tests.

Port/process detection:

isolar interfaces para permitir testes sem matar processos reais.

Não execute operações destrutivas durante testes.

---

# 36. DOCUMENTAÇÃO OBRIGATÓRIA

Crie desde já:

README.md

docs/PRODUCT\_SPEC.md

docs/ARCHITECTURE.md

docs/SECURITY.md

docs/ROADMAP.md

docs/DATABASE.md

docs/DEVELOPMENT.md

docs/adr/

Pelo menos:

ADR-001-desktop-stack.md

ADR-002-local-database.md

ADR-003-provider-architecture.md

Além disso:

TASKS.md

ou outro mecanismo simples que mostre claramente:

NOW
NEXT
LATER
BLOCKED

---

# 37. README

O README precisa explicar:

- o que é LK Dev Hub;
- problema que resolve;
- screenshots/mockup;
- stack;
- arquitetura;
- prerequisites;
- development;
- build;
- testing;
- project structure;
- roadmap.

Não use marketing exagerado.

---

# 38. REGRAS DE DESENVOLVIMENTO

Quero estas regras:

1. Não alterar arquitetura sem registrar motivo.
2. Commits pequenos e semanticamente coerentes, se o ambiente permitir Git.
3. Não fazer merge automático.
4. Não usar `--force`.
5. Não apagar arquivos sem verificar impacto.
6. Não colocar secrets no repository.
7. Não instalar dependências sem necessidade clara.
8. Verificar licença das dependências.
9. Manter TypeScript strict.
10. Evitar `any`.
11. Rust sem `unwrap()` irresponsável em caminhos de runtime.
12. Erros devem possuir tratamento apropriado.
13. Não criar mocks permanentes para esconder funcionalidade não implementada.
14. Quando algo ainda não funciona, mostrar claramente como unavailable/not configured.

---

# 39. FASE DE EXECUÇÃO

Execute o trabalho em fases.

## Fase 0 — Discovery

Verifique:

- ambiente disponível;
- ferramentas disponíveis;
- versões atuais;
- restrições do Work;
- se já existe algum projeto/repository relacionado.

Se nenhum projeto existir, crie um novo projeto limpo.

Nome sugerido:

`lk-dev-hub`

Não reaproveite repository não relacionado.

---

## Fase 1 — Product Foundation

Criar:

documentação
arquitetura
ADRs
estrutura de projeto
SQLite
migrations
layout principal
sidebar
routing
dashboard skeleton

---

## Fase 2 — Project Registry

Implementar funcionalmente:

create project
list projects
view project
edit project
delete with confirmation

Dados persistidos no SQLite.

---

## Fase 3 — System Detection

Implementar:

Git detection
Node detection
Claude detection
gh detection
Docker detection

Mostrar no dashboard.

---

## Fase 4 — Git Integration

Implementar status real para repositories registrados.

---

## Fase 5 — Port Monitor

Implementar detecção real no Windows quando o ambiente permitir.

Se Work não estiver executando Windows, implemente a camada e testes, deixando a validação Windows explicitamente marcada como pendente.

---

## Fase 6 — Project Snapshot

Implementar Generate Development Context.

---

# 40. IMPORTANTE: NÃO PARE APÓS PLANEJAR

Depois de criar arquitetura e documentação:

CONTINUE.

Crie o projeto.

Implemente a fundação.

Implemente funcionalidades.

Execute testes.

Corrija problemas encontrados.

Não entregue apenas:

“este seria o plano”.

Quero sair desta sessão com **código real**.

---

# 41. ATUALIZAÇÕES DURANTE O TRABALHO

Durante a execução, mantenha atualizações curtas contendo:

- fase atual;
- o que foi concluído;
- decisão importante tomada;
- bloqueios reais.

Não me bombardeie com logs triviais.

Se precisar tomar uma decisão técnica reversível e de baixo risco, tome a decisão e documente.

Não interrompa o trabalho para pedir confirmação sobre escolhas triviais.

---

# 42. CRITÉRIO DE QUALIDADE

Não considero concluído apenas porque compila.

Ao final, verifique:

- build;
- lint;
- tests;
- TypeScript;
- Rust;
- migrations;
- startup;
- persistência;
- erro handling;
- arquitetura;
- documentação.

Se alguma verificação não puder ser executada, marque claramente:

NOT VERIFIED

e explique exatamente por quê.

---

# 43. ENTREGA FINAL DA SESSÃO

No final quero um relatório objetivo:

# LK DEV HUB — DEVELOPMENT CHECKPOINT

## Implementado

## Arquivos principais

## Arquitetura atual

## Banco

## Features funcionais

## Testes executados

## Build status

## Limitações

## Validações que precisam ser feitas no meu Windows

## Pendências

## Próxima fase recomendada

## Como executar localmente

Também quero uma árvore resumida do repository.

---

# 44. OBJETIVO DESTA PRIMEIRA SESSÃO

O resultado ideal é possuir uma aplicação desktop inicial que já abra e demonstre que a arquitetura funciona, com:

- visual semelhante ao mockup;
- sidebar;
- dashboard;
- SQLite funcional;
- project registry funcional;
- pelo menos algumas detecções reais do ambiente;
- Git integration básica;
- foundation para ports/process monitoring;
- geração de Development Context;
- arquitetura preparada para Claude/Agents/Skills/MCP.

Priorize **fundação sólida e funcionalidade real** em vez de tentar simular todas as telas do produto futuro.

Comece agora pela análise do ambiente e siga diretamente para implementação.