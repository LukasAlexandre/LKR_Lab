# Initial threat model

## Ativos e fronteiras

Arquivos do usuário, credenciais existentes nas CLIs, processos locais e banco do hub. Conteúdo dos projetos e metadados remotos são não confiáveis. A sessão do usuário e ferramentas instaladas no PATH são a autoridade local; a aplicação não eleva privilégios.

| Ameaça | Controle atual | Limite residual |
|---|---|---|
| Injeção por caminho/nome | `Command::args`, caminhos canonicalizados, URLs HTTPS sem credentials/query; sem shell de usuário | Executáveis instalados/PATH e configuração Git continuam parte da confiança local |
| Renderer comprometido | CSP sem scripts remotos, sem eval; zero plugins de shell/fs/sql; handlers enumerados | Uma XSS no renderer poderia chamar handlers permitidos, inclusive booleanos de confirmação; não é uma fronteira de autorização independente |
| Vazamento de secrets | Não lê .env, tokens, cookies ou conteúdo MCP; não retorna stderr de CLI | Usuário pode digitar segredo em template/comando; snapshot inclui metadados de projeto/commits |
| Encerramento incorreto | Modal explícito, PID + start time rechecados, bloqueio PID <=4 e do próprio hub | Pequena corrida OS entre consulta e sinal; não eleva privilégios; nomes críticos protegidos quando reconhecidos |
| Perda de projeto | Delete remove apenas cadastro, nunca pasta; FK transacional | Templates locais associados são removidos junto com cadastro |
| Worktree com alterações | Apenas listagem disponível | Mutação deverá ter novo modelo de confirmação e rechecagem |
| Consulta travada | CLI com deadline e limite de saída; IPC de consultas em worker | Cancelamento de descendentes e leituras de filesystem em rede são melhorias futuras |
| Banco incompatível/corrompido | Migration transacional e user_version, recusa versão futura | Backup/restauração UI ainda pendentes; startup encerra com erro se banco não abrir |

## Capacidades

Janela main; nenhuma capability de plugin shell, fs, http ou SQL. Os comandos próprios são a allowlist em `generate_handler!`. Nenhum comando aceita SQL livre, código de shell, token ou caminho arbitrário para escrita. Remoção e kill exigem flags explícitas além do diálogo na UI.

## Comportamento conservador

Sem commit, push, merge, reset, rebase ou fetch automático. Sem scan de disco. Sem execução de scripts package.json na descoberta. Sem leitura automática do histórico privado de agentes. Comandos declarados são somente dados nesta versão.

## Antes de release comercial

Revisar runner e configurações Git não confiáveis; adicionar autorização de confirmação no host para operações sensíveis, auditoria de dependências transitivas, assinatura de instalador e updater; completar backup, retenção de atividades, tratamento de discos de rede e smoke tests Windows. Não há alegação de auditoria de segurança completa.

## Execução de comandos (Project Runtime Manager)

O LKR LAB passou a executar scripts, sempre por ação explícita do usuário. Limites: só scripts do `package.json` da pasta vinculada e canônica (sem escapar por symlink); nome validado por padrão restrito e comparado com o arquivo; argumentos fixos (`run <nome>`), sem shell próprio e sem interpolação; projeto `unbound`/`missing` nunca executa; dados portáteis (workspace.json, `Project.commands`) são só metadados. Parar atua apenas em árvores iniciadas pelo próprio app (Job Object); processos externos nunca são encerrados por essa via. O script em si é código do projeto local e roda com as permissões do usuário.
