# ADR-002 — Local SQLite

Status: accepted.

SQLite bundled com rusqlite: instalação sem serviço externo, controle nativo no Rust, SQL parametrizado e migration transacional. Renderer recebe DTOs, nunca conexão/SQL.

Project é agregado JSON com path único indexado; prompts e atividades normalizados. Essa escolha evita dezenas de tabelas não usadas. Próximas migrations podem decompor serviços/portas quando funcionalidades exigirem consultas relacionais.

Dados ficam no app data do usuário, sem cloud nem telemetria. Nenhuma criptografia caseira. Gerenciamento real de segredos futuro depende do mecanismo seguro do OS.
