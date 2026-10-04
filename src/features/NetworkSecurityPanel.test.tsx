import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { NetworkSecurityPanel } from "./NetworkSecurityPanel";
import { NOW, healthySnapshot, listener, note, withOverall, withStatus } from "../shared/networkSecurity.fixtures";
import type { NetworkSecuritySnapshot } from "../shared/types";

const ready = (snapshot: NetworkSecuritySnapshot | null, error: string | null = null) =>
  renderToStaticMarkup(<NetworkSecurityPanel state={{ snapshot, error, loading: false }} now={NOW + 5_000} />);

const domain = (html: string, title: string) => {
  const start = html.indexOf(`aria-label="${title}"`);
  expect(start).toBeGreaterThan(-1);
  const next = html.indexOf('<details class="mh-win-domain"', start + 10);
  return html.slice(start, next === -1 ? undefined : next);
};

describe("Network & Security — máquina saudável (não inventa problema)", () => {
  const html = ready(healthySnapshot());
  it("mostra o estado geral sem alertas", () => {
    expect(html).toContain("Network &amp; Security");
    expect(html).toContain("Saudável");
    expect(html).toContain("4 de 5 verificações avaliadas");
    expect(html).not.toContain('role="alert"');
    expect(html).not.toMatch(/mh-win-reasons (attention|critical)/);
  });
  it("resumo com os indicadores", () => {
    const summary = html.slice(html.indexOf('aria-label="Resumo de rede e segurança"'), html.indexOf('aria-label="Rede"'));
    for (const label of ["Rede", "Firewall", "Antivírus", "BitLocker", "Secure Boot", "TPM", "Portas em escuta"]) {
      expect(summary).toContain(label);
    }
  });
  it("rede: interface ativa, IP local, gateway, DNS", () => {
    const net = domain(html, "Rede");
    expect(net).toContain("Interface ativa");
    expect(net).toContain("Wi-Fi");
    expect(net).toContain("192.168.1.42");
    expect(net).toContain("192.168.1.1");
    expect(net).toContain("1.1.1.1, 8.8.8.8");
    expect(net).toContain("Rota padrão");
    expect(net).toContain("574 Mbps");
    expect(net).toContain("IPv6 fe80::1");
  });
  it("IP público: 'Não consultado', sem requisição", () => {
    const net = domain(html, "Rede");
    expect(net).toContain("IP público");
    expect(net).toContain("Não consultado");
    expect(net).toContain("não faz requisições externas");
  });
  it("categoria da rede desconhecida não vira Pública", () => {
    const net = domain(html, "Rede");
    expect(net).toMatch(/Categoria da rede<\/dt><dd>Desconhecido/);
    expect(net).not.toContain("Categoria da rede</dt><dd>Público");
  });
  it("tudo recolhido por padrão e sem nenhuma ação de alteração", () => {
    expect(html).not.toMatch(/<details[^>]* open/);
    expect(html).not.toMatch(/<button[^>]*>(?:Corrigir|Ativar|Bloquear|Encerrar|Remover|Escanear|Reparar)/i);
    expect(html).toContain("Somente observação");
    expect(html).toContain("não ativa firewall");
  });
  it("rodapé afirma que conexões ficam só na máquina", () => {
    expect(html).toContain("ficam só nesta máquina");
  });
});

describe("Network & Security — exposição", () => {
  const html = ready(healthySnapshot());
  const exposure = domain(html, "Exposição (portas em escuta)");
  it("lista listeners de loopback, interface específica e todas as interfaces", () => {
    expect(exposure).toContain("127.0.0.1:4317");
    expect(exposure).toContain("192.168.1.42:5432");
    expect(exposure).toContain("0.0.0.0:3000");
    expect(exposure).toContain("Somente esta máquina");
    expect(exposure).toContain("Interface específica");
    expect(exposure).toContain("Todas as interfaces");
  });
  it("detalhes do listener: processo, PID, executável e Project", () => {
    expect(exposure).toContain("node.exe (PID 100)");
    expect(exposure).toContain("C:\\Dev\\Lab\\node.exe");
    expect(exposure).toContain("LKR_Lab");
  });
  it("0.0.0.0 é explicado como não sendo exposição à internet", () => {
    expect(exposure).toContain("não significa exposição à internet");
    expect(exposure).toContain("não testa");
    expect(exposure.toLowerCase()).not.toContain("vulnerab");
  });
  it("contagens e resumo descritivo", () => {
    expect(exposure).toContain("3 portas · 1 em todas as interfaces");
  });
  it("busca e filtro de escopo presentes", () => {
    expect(exposure).toContain('aria-label="Buscar porta, processo ou projeto"');
    expect(exposure).toContain('aria-label="Filtrar por escopo"');
  });
  it("sem listeners: estado vazio explícito", () => {
    const s = healthySnapshot();
    const empty = { ...s, exposure: { ...s.exposure, listeners: [], counts: { total: 0, loopback: 0, specific: 0, allInterfaces: 0, unidentified: 0 } } };
    expect(domain(ready(empty), "Exposição (portas em escuta)")).toContain("Nenhuma porta em escuta.");
  });
  it("porta sem dono identificado fica 'não identificada'", () => {
    const s = healthySnapshot();
    const orphan = { ...s, exposure: { ...s.exposure, listeners: [listener({ pid: null, processName: null, executable: null, projectName: null })] } };
    expect(domain(ready(orphan), "Exposição (portas em escuta)")).toContain("Processo não identificado");
  });
  it("mostra só parte da lista grande e oferece 'Mostrar todas'", () => {
    const s = healthySnapshot();
    const many = Array.from({ length: 60 }, (_, i) => listener({ port: 2000 + i, pid: 500 + i, processName: `p${i}.exe`, projectName: null }));
    const out = domain(ready({ ...s, exposure: { ...s.exposure, listeners: many } }), "Exposição (portas em escuta)");
    expect(out).toContain("Mostrar todas (60)");
    expect(out).not.toContain("127.0.0.1:2059");
  });
});

describe("Network & Security — conexões (visão compacta)", () => {
  const html = ready(healthySnapshot());
  const conns = domain(html, "Conexões estabelecidas");
  it("mostra contagens e, por padrão, só as remotas", () => {
    expect(conns).toContain("2 conexões · 1 remotas");
    expect(conns).toContain("140.82.112.3:443");
    expect(conns).not.toContain("127.0.0.1:4317");
  });
  it("explica que não há DNS reverso e que o dado é da máquina", () => {
    expect(conns).toContain("Sem resolução reversa de DNS");
  });
  it("sem conexões: estado vazio", () => {
    const s = healthySnapshot();
    const empty = { ...s, connections: { ...s.connections, total: 0, remote: 0, loopback: 0, items: [] } };
    expect(domain(ready(empty), "Conexões estabelecidas")).toContain("Nenhuma conexão estabelecida.");
  });
});

describe("Network & Security — firewall", () => {
  it("três perfis; sem categoria ativa, explica o que o estado reflete", () => {
    const fw = domain(ready(healthySnapshot()), "Firewall do Windows");
    for (const profile of ["Domínio", "Privado", "Público"]) expect(fw).toContain(profile);
    expect(fw).toContain("Ativado");
    expect(fw).toContain("A categoria da rede ativa não foi informada");
    expect(fw).toContain("Bom");
  });
  it("perfil ativo destacado", () => {
    const s = healthySnapshot();
    const profiles = s.firewall.profiles.map((p) => ({ ...p, active: p.kind === "private" }));
    const fw = domain(ready(withStatus(s, "firewall", "healthy", [], { profiles, activeProfile: "private" })), "Firewall do Windows");
    expect(fw).toContain("Perfil ativo");
    expect(fw).toContain("perfil Privado");
  });
  it("firewall desativado no perfil ativo: crítico com motivo visível", () => {
    const s = healthySnapshot();
    const profiles = s.firewall.profiles.map((p) => ({ ...p, enabled: false, active: p.kind === "private" }));
    const reason = "Firewall do Windows desativado no perfil ativo (Privado) e nenhum outro firewall informado pelo Security Center.";
    const html = ready(withOverall(withStatus(s, "firewall", "critical", [reason], { profiles, activeProfile: "private", securityCenter: "poor" }), "critical", [{ domain: "firewall", text: reason }]));
    expect(html).toContain("Crítico");
    expect(html).toContain('role="alert"');
    expect(html).toContain("Firewall:");
    expect(domain(html, "Firewall do Windows")).toContain("Desativado");
    expect(domain(html, "Firewall do Windows")).toContain("Em risco");
  });
  it("política de entrada por padrão aparece quando existe", () => {
    const s = healthySnapshot();
    const profiles = s.firewall.profiles.map((p) => ({ ...p, defaultInbound: "block" as const, defaultOutbound: "allow" as const }));
    const fw = domain(ready(withStatus(s, "firewall", "healthy", [], { profiles })), "Firewall do Windows");
    expect(fw).toContain("entrada: Bloquear");
    expect(fw).toContain("saída: Permitir");
  });
});

describe("Network & Security — antivírus, Defender e criptografia", () => {
  it("Defender ativo: versão, assinaturas, ameaças 'Não consultado'", () => {
    const av = domain(ready(healthySnapshot()), "Antivírus e Defender");
    expect(av).toContain("Microsoft Defender");
    expect(av).toContain("Ativo");
    expect(av).toContain("1.459.546.0");
    expect(av).toContain("1.1.26080.3");
    expect(av).toContain("Nenhum registrado");
    expect(av).toMatch(/Ameaças ativas<\/dt><dd>Não consultado/);
  });
  it("antivírus de terceiros com Defender passivo não é problema", () => {
    const s = healthySnapshot();
    const html = ready(withStatus(s, "antivirus", "healthy", [], {
      provider: "third_party", thirdPartyCount: 1,
      defender: { ...s.antivirus.defender, state: "passive" },
      notes: ["O Defender está passivo porque outro antivírus está registrado. Isso é esperado, não um problema."],
    }));
    const av = domain(html, "Antivírus e Defender");
    expect(av).toContain("Antivírus de terceiros");
    expect(av).toContain("Passivo (outro antivírus protege)");
    expect(av).toContain("1 registrado(s)");
    expect(av).toContain("Isso é esperado, não um problema.");
    expect(html).not.toMatch(/mh-win-reasons (attention|critical)/);
  });
  it("Defender desativado sem antivírus: crítico", () => {
    const s = healthySnapshot();
    const reason = "Nenhum antivírus ativo: o Defender está desativado e não há antivírus de terceiros registrado.";
    const html = ready(withOverall(withStatus(s, "antivirus", "critical", [reason], {
      provider: "none", defender: { ...s.antivirus.defender, state: "disabled" }, securityCenter: "not_monitored",
    }), "critical", [{ domain: "antivirus", text: reason }]));
    expect(html).toContain("Nenhum antivírus ativo");
    expect(domain(html, "Antivírus e Defender")).toContain("Desativado");
  });
  it("ameaça ativa é mostrada com a contagem", () => {
    const s = healthySnapshot();
    const html = ready(withStatus(s, "antivirus", "critical", ["O Microsoft Defender informa 2 ameaça(s) ativa(s)."], {
      defender: { ...s.antivirus.defender, activeThreats: 2 },
    }));
    expect(domain(html, "Antivírus e Defender")).toMatch(/Ameaças ativas<\/dt><dd>2/);
  });
  it("BitLocker indisponível: Desconhecido, 'privilégio administrativo' e sem chave de recuperação", () => {
    const html = ready(healthySnapshot());
    const enc = domain(html, "Criptografia (BitLocker)");
    expect(enc).toContain("Desconhecido");
    expect(enc).toContain("Requer privilégio administrativo");
    expect(enc).toContain("Chaves de recuperação nunca são lidas");
  });
  it("BitLocker protegido, suspenso e desligado", () => {
    const s = healthySnapshot();
    const volumes = [
      { mount: "C:", system: true, state: "protected" as const },
      { mount: "D:", system: false, state: "suspended" as const },
      { mount: "E:", system: false, state: "off" as const },
    ];
    const enc = domain(ready(withStatus(s, "encryption", "attention", ["BitLocker suspenso em D:."], { volumes })), "Criptografia (BitLocker)");
    expect(enc).toContain("Protegido");
    expect(enc).toContain("Suspenso");
    expect(enc).toContain("Desligado");
    expect(enc).toContain("sistema");
  });
});

describe("Network & Security — Secure Boot e TPM", () => {
  it("Secure Boot ativado e firmware UEFI; TPM presente 2.0", () => {
    const html = ready(healthySnapshot());
    expect(domain(html, "Secure Boot")).toContain("Ativado");
    expect(domain(html, "Secure Boot")).toContain("UEFI");
    expect(domain(html, "TPM")).toContain("Presente");
    expect(domain(html, "TPM")).toContain("2.0");
  });
  it("Secure Boot desativado é atenção (fato) com motivo", () => {
    const s = healthySnapshot();
    const html = ready(withOverall(withStatus(s, "secureBoot", "attention", ["Secure Boot está desativado."], { state: "disabled" }), "attention", [{ domain: "secure_boot", text: "Secure Boot está desativado." }]));
    expect(html).toContain("Atenção");
    expect(html).toContain("Secure Boot:");
    expect(domain(html, "Secure Boot")).toContain("Desativado");
  });
  it("Secure Boot indisponível e BIOS legado: nunca 'Ativado'", () => {
    const s = healthySnapshot();
    const unavailable = domain(ready(withStatus(s, "secureBoot", "unknown", ["Estado do Secure Boot indisponível."], { state: "unavailable", uefi: null })), "Secure Boot");
    expect(unavailable).toContain("Desconhecido");
    expect(unavailable).not.toContain("Ativado");
    const legacy = domain(ready(withStatus(s, "secureBoot", "unknown", [], { state: "unsupported", uefi: false })), "Secure Boot");
    expect(legacy).toContain("BIOS legado");
  });
  it("TPM indisponível é Desconhecido e ausente é 'Não detectado'", () => {
    const s = healthySnapshot();
    expect(domain(ready(withStatus(s, "tpm", "unknown", ["TPM: indisponível"], { present: null, version: null })), "TPM")).toContain("Desconhecido");
    expect(domain(ready(withStatus(s, "tpm", "attention", ["Nenhum TPM detectado."], { present: false, version: null })), "TPM")).toContain("Não detectado");
  });
});

describe("Network & Security — dados parciais, elevação e estados gerais", () => {
  it("sem snapshot: carregando; com erro e sem snapshot: mostra o erro", () => {
    expect(ready(null)).toContain("Lendo a rede e a postura de segurança");
    expect(ready(null, "falhou")).toContain("falhou");
  });
  it("erro com snapshot anterior: mantém o conteúdo e avisa", () => {
    const html = ready(healthySnapshot(), "sem resposta");
    expect(html).toContain("sem resposta");
    expect(html).toContain("mostrando a leitura anterior");
    expect(html).toContain("Wi-Fi");
  });
  it("lista de fontes não disponíveis mostra 'Requer privilégio administrativo'", () => {
    const html = ready(healthySnapshot());
    expect(html).toContain("Fontes não disponíveis (1)");
    expect(html).toContain("BitLocker por volume");
    expect(html).toContain("Requer privilégio administrativo");
  });
  it("fonte parcial mostra o motivo no domínio", () => {
    const s = healthySnapshot();
    const partial = { ...s, network: { ...s.network, sources: [note({ id: "network_profile", label: "Categoria da rede", state: "partial", reason: "O Windows não informou a categoria da rede ativa." })] } };
    expect(domain(ready(partial), "Rede")).toContain("O Windows não informou a categoria da rede ativa.");
  });
  it("tudo desconhecido: não diz que está seguro", () => {
    const s = healthySnapshot();
    const html = ready(withOverall(s, "unknown", [], 0));
    expect(html).toContain("Desconhecido");
    expect(html).toContain("nada é tratado como seguro");
  });
  it("atenção: mostra os motivos por domínio", () => {
    const html = ready(withOverall(healthySnapshot(), "attention", [{ domain: "tpm", text: "Nenhum TPM detectado." }]));
    expect(html).toContain("TPM:");
    expect(html).toContain("Nenhum TPM detectado.");
  });
  it("sem conexão ativa", () => {
    const s = healthySnapshot();
    const html = ready({ ...s, network: { ...s.network, activeInterface: null, localIpv4: null, gateway: null, dns: [] } });
    expect(html).toContain("Sem conexão ativa");
  });
});

describe("Network & Security — última leitura e validade", () => {
  it("mostra 'Última leitura' por domínio e 'agora' para leitura recente", () => {
    const html = ready(healthySnapshot());
    expect(html).toContain("Última leitura agora.");
  });
  it("marca como desatualizado quando passa do dobro do TTL", () => {
    const s = healthySnapshot();
    const old = withStatus(s, "firewall", "healthy", [], { checkedAt: NOW - 200_000 });
    expect(domain(ready(old), "Firewall do Windows")).toContain("desatualizado");
    expect(domain(ready(old), "Firewall do Windows")).toContain("Última leitura há 3 min.");
  });
});
