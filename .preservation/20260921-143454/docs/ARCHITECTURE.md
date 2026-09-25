# Architecture

## Limites

Renderer React → `api<T>`/invoke → lista explícita de Tauri commands → `hub-core` → SQLite/OS/CLIs.

O renderer não recebe um shell nem SQL livre. Tauri commands aceitam DTOs e IDs de projetos; o backend busca o caminho cadastrado. Descoberta é o único fluxo de leitura de pasta ainda não cadastrada e exige entrada/seleção explícita. Exportação usa diálogo de destino.

## Núcleo Rust

- database: conexão, migration transacional, cadastro e templates parametrizados.
- projects: canonicalização, validação, descoberta sem execução.
- commands: executáveis no PATH absoluto, argumentos separados, stdin desativado, prazo de 12 s, saída de 512 KiB, sem stderr sensível propagado.
- git / github: Git local e GitHostingProvider/GitHubProvider.
- system / ports: sysinfo e netstat2. Nenhuma coleta periódica oculta.
- agents: AgentProvider/ClaudeProvider, apenas presença de arquivos e nomes de skills.
- launchers: ações enumeradas; Windows Terminal com comando Claude fixo.
- snapshot: composição local, marca fontes não consultadas como NOT VERIFIED.

Integrações pesadas rodam em `spawn_blocking`, liberando a thread de UI. Mutex protege SQLite. Locks são liberados antes de consultas lentas ao OS/CLI.

## Frontend

App coordena hash routing, projeto ativo, atualização e modais. Features extraídas: Projects, Ports/Processes, Prompts. Shared contém tipos, bridge, lógica de templates e componentes. Sem estado falso na prévia web. Toda rejeição IPC vira mensagem visível.

## Escolhas proporcionais

Project é agregado persistido em JSON validado, com caminho indexado relacionalmente. Templates e atividades têm FK. Não foram criadas tabelas vazias para roadmap. Providers só existem onde já há comportamento; não há framework de plugins antecipado.

## Próximas melhorias técnicas

Extrair Dashboard e ProjectDetails do App conforme crescerem; cache por ID para Git; background polling opt-in com timestamp por fonte; runner cancelável com árvore de processos e isolamento adicional; registry de providers para terminais/knowledge quando houver uma segunda implementação real.
