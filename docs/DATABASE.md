# Database

SQLite bundled por rusqlite, `app_data_dir()/hub.db`. WAL, foreign_keys ON e busy timeout de 5 segundos.

Migration `crates/hub-core/migrations/001_initial.sql`: executada em transação quando user_version=0. Versão 1 não executa novamente seeds. Versão futura é recusada.

| Tabela | Propósito |
|---|---|
| projects | id UUID, local_path único canonicalizado, data JSON válido do agregado Project, updated_at |
| prompt_templates | id, title, category, project_id opcional FK CASCADE, body |
| activities | id incremental, project_id FK SET NULL, action genérica, created_at UTC |

O agregado contém nome, slug, descrição, path, URL HTTPS, stack, tags, portas, comandos declarados e timestamps. Listas pequenas coesas não exigem joins neste estágio. Migrar ports/services para tabelas apenas quando execução e consultas cruzadas justificarem.

Usa parâmetros SQL em toda entrada. Save+activity e delete+activity são atômicos. Caminho duplicado é rejeitado. Deletar projeto remove templates locais, mantém histórico sem vínculo e preserva arquivos.

Não armazena valores de ambiente, credenciais de CLIs ou tokens. Texto digitado pelo usuário não é criptografado. Feche o app para copiar hub.db com segurança; backups online e exportação do cadastro não implementados.
