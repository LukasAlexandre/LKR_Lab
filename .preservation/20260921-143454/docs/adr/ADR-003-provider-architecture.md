# ADR-003 — Proportional providers

Status: accepted.

GitHostingProvider recebe path e retorna DTO HostingState; primeira implementação usa gh. AgentProvider expõe contexto, primeira implementação Claude. Git local é domínio próprio, independente de GitHub.

Não criar APIs enterprise sem implementações. Terminal/knowledge permanecem módulos simples até precisarem de segundo provider. Sessões e contas de IA não são inferidas de scraping; ausência é explícita. Futuro CodexProvider não deve alterar ProjectRegistry.
