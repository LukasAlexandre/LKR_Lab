# ADR-001 — Desktop stack

Status: accepted. Data: 2026-09-19.

Windows primeiro, Tauri 2/React/TypeScript/Vite/Rust, conforme briefing. CSS próprio e Lucide para limitar dependências visuais. Não existe impedimento documentado para abandonar Tauri por Electron.

Registros oficiais consultados: React 19.3.0, Vite 8.3.0, Tauri JS API 2.11.1/CLI 2.11.4, crate Tauri 2.11.5. Rust stable instalado 1.98.1. Node disponível 24.19.0 atende engines Vite ^20.19 ou >=22.12.

TypeScript latest 7.0.2 foi avaliado e rejeitado: typescript-eslint 8.70 requer >=4.8.4 <6.1. Selecionado TypeScript 6.0.3, estável compatível, sem --force ou legacy-peer-deps. O lock registra as versões resolvidas.

Núcleo independente da janela permite validar banco/OS em headless. A ausência de GTK/WebKit no Work limita a validação do shell Tauri, não justifica trocar a stack.
