# Development guide

## Executar no Windows

1. Instalar pré-requisitos do README, usando Rust MSVC.
2. Abrir PowerShell no checkout do LK-Tools.
3. `npm ci`, depois `npm run tauri dev`.
4. Cadastrar uma pasta de teste; não usar um projeto sensível para o primeiro smoke test.

`npm run dev` abre só a camada web em http://127.0.0.1:1420. Banner informa ausência de backend; CRUD fica desabilitado. Não se usa localStorage como substituto de SQLite.

## Gates

`npm run typecheck`, `npm run lint`, `npm test`, `npm run build`.
`cargo fmt --all -- --check`, `cargo test -p hub-core`, `cargo clippy -p hub-core --all-targets -- -D warnings`.
Windows: `cargo check -p lk-dev-hub`, teste live de portas explícito, `npm run tauri build`.

O workflow Windows automatiza esses gates sem publicar. O script `scripts/validate-windows.ps1` roda os comandos localmente e interrompe na primeira falha.

## Smoke test manual Windows obrigatório

- Startup: janela abre sem console de erro; estados reais de ferramentas.
- Cadastro: selecionar pasta, detectar, revisar, salvar; editar e reiniciar app para validar persistência.
- Git: repo limpo, dirty, sem upstream, sem commits e worktree.
- Portas: executar servidor de teste; confirmar TCP/IPv4/IPv6 e PID. Porta declarada não pode aparecer como propriedade confirmada sem evidência.
- Kill: cancelar deixa processo vivo; confirmar somente sobre servidor descartável; PID/start time alterado deve recusar.
- Launchers: pasta com espaços/Unicode, Windows Terminal, Code.exe e Claude nativo. Falta de ferramenta deve gerar erro útil.
- Prompts: template global/local, variável conhecida/desconhecida, copiar.
- Contexto: revisar, copiar, escolher destino, cancelar export, conferir que .env não aparece.
- Remoção: confirmar retira somente cadastro; pasta continua intacta.
- GitHub: sem gh, não autenticado, sem remote GitHub, autenticado com/sem PR e CI pendente.

## Padrão de alterações

Registrar decisões em ADR; evitar deps desnecessárias; atualizar TASKS e checkpoint. TS strict, sem any. Runtime Rust sem unwrap de entradas. Não fazer merge automático, force push nem execução de scripts recebidos de projetos. Commits pequenos por domínio.
