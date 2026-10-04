import { describe, expect, it } from "vitest";
import {
  DEFAULT_EXPOSURE_FILTER,
  exposureSummary,
  filterConnections,
  filterListeners,
  formatLinkSpeed,
  hostPort,
  listenerAddress,
  listenerOwner,
  profileLabel,
  summaryItems,
  unavailableNetSources,
} from "./networkSecurity";
import { NOW, connection, healthySnapshot, listener, withStatus } from "./networkSecurity.fixtures";

describe("filtros de exposição", () => {
  const all = healthySnapshot().exposure.listeners;
  it("sem filtro devolve tudo, na mesma ordem", () => {
    expect(filterListeners(all, DEFAULT_EXPOSURE_FILTER).map((l) => l.port)).toEqual([3000, 5432, 4317]);
  });
  it("filtra por escopo", () => {
    expect(filterListeners(all, { scope: "all_interfaces", query: "" }).map((l) => l.port)).toEqual([3000]);
    expect(filterListeners(all, { scope: "loopback", query: "" }).map((l) => l.port)).toEqual([4317]);
    expect(filterListeners(all, { scope: "specific", query: "" }).map((l) => l.port)).toEqual([5432]);
  });
  it("busca por porta, processo, projeto, PID e endereço", () => {
    expect(filterListeners(all, { scope: "all", query: "5432" }).map((l) => l.port)).toEqual([5432]);
    expect(filterListeners(all, { scope: "all", query: "VITE" }).map((l) => l.port)).toEqual([3000]);
    expect(filterListeners(all, { scope: "all", query: "lkr_lab" }).map((l) => l.port)).toEqual([4317]);
    expect(filterListeners(all, { scope: "all", query: "12" }).map((l) => l.port)).toContain(5432);
    expect(filterListeners(all, { scope: "all", query: "0.0.0.0" }).map((l) => l.port)).toEqual([3000]);
  });
  it("escopo e busca combinam; sem resultado devolve lista vazia", () => {
    expect(filterListeners(all, { scope: "loopback", query: "vite" })).toEqual([]);
  });
  it("porta sem dono não quebra a busca", () => {
    const orphan = listener({ port: 135, pid: null, processName: null, projectName: null });
    expect(filterListeners([orphan], { scope: "all", query: "135" })).toHaveLength(1);
    expect(filterListeners([orphan], { scope: "all", query: "node" })).toHaveLength(0);
  });
});

describe("conexões", () => {
  const items = [
    connection(),
    connection({ scope: "local", remoteAddress: "192.168.1.10", remotePort: 445, processName: "svchost.exe" }),
    connection({ scope: "loopback", remoteAddress: "127.0.0.1", remotePort: 4317, processName: "node.exe", projectName: "LKR_Lab" }),
  ];
  it("remotas são só as que saem da máquina", () => {
    expect(filterConnections(items, "remote").map((c) => c.scope)).toEqual(["remote"]);
  });
  it("local inclui rede local e loopback", () => {
    expect(filterConnections(items, "local").map((c) => c.scope)).toEqual(["local", "loopback"]);
  });
  it("todas + busca", () => {
    expect(filterConnections(items, "all")).toHaveLength(3);
    expect(filterConnections(items, "all", "445")).toHaveLength(1);
    expect(filterConnections(items, "all", "lkr_lab")).toHaveLength(1);
  });
});

describe("apresentação", () => {
  it("endereço IPv6 usa colchetes", () => {
    expect(listenerAddress(listener({ address: "::1", ipVersion: "v6", port: 80 }))).toBe("[::1]:80");
    expect(listenerAddress(listener({ address: "127.0.0.1", port: 80 }))).toBe("127.0.0.1:80");
  });
  it("host:porta com colchetes para IPv6", () => {
    expect(hostPort("2606:4700::1", 443)).toBe("[2606:4700::1]:443");
    expect(hostPort("140.82.112.3", 443)).toBe("140.82.112.3:443");
  });
  it("dono da porta: nome+PID, só PID ou não identificado", () => {
    expect(listenerOwner({ processName: "node.exe", pid: 7 })).toBe("node.exe (PID 7)");
    expect(listenerOwner({ processName: null, pid: 7 })).toBe("PID 7");
    expect(listenerOwner({ processName: null, pid: null })).toBe("Processo não identificado");
  });
  it("velocidade do link e categoria desconhecida", () => {
    expect(formatLinkSpeed(574_000_000)).toBe("574 Mbps");
    expect(formatLinkSpeed(1_000_000_000)).toBe("1 Gbps");
    expect(formatLinkSpeed(2_500_000_000)).toBe("2.5 Gbps");
    expect(formatLinkSpeed(null)).toBe("Desconhecido");
    expect(profileLabel(null)).toBe("Desconhecido");
    expect(profileLabel("public")).toBe("Público");
  });
  it("resumo de exposição é descritivo, nunca 'exposto'", () => {
    const text = exposureSummary(healthySnapshot());
    expect(text).toBe("3 portas · 1 em todas as interfaces");
    expect(text.toLowerCase()).not.toContain("expost");
    expect(text.toLowerCase()).not.toContain("vulner");
  });
  it("resumo: rede, firewall, antivírus, BitLocker desconhecido, Secure Boot, TPM", () => {
    const byId = Object.fromEntries(summaryItems(healthySnapshot()).map((i) => [i.id, i]));
    expect(byId.network.value).toBe("Wi-Fi · 192.168.1.42");
    expect(byId.firewall.value).toBe("3 de 3 perfis ativos");
    expect(byId.antivirus.value).toBe("Microsoft Defender");
    expect(byId.encryption.value).toBe("Desconhecido");
    expect(byId.encryption.status).toBe("unknown");
    expect(byId.secure_boot.value).toBe("Ativado");
    expect(byId.tpm.value).toBe("Presente 2.0");
  });
  it("resumo: firewall com perfil ativo e sem conexão ativa", () => {
    const s = healthySnapshot();
    const withProfile = withStatus(s, "firewall", "healthy", [], { activeProfile: "private" });
    expect(summaryItems(withProfile).find((i) => i.id === "firewall")?.value).toBe("Perfil Privado");
    const offline = { ...s, network: { ...s.network, activeInterface: null, localIpv4: null } };
    expect(summaryItems(offline).find((i) => i.id === "network")?.value).toBe("Sem conexão ativa");
  });
  it("resumo: BitLocker protegido no volume do sistema", () => {
    const s = withStatus(healthySnapshot(), "encryption", "healthy", [], { volumes: [{ mount: "C:", system: true, state: "protected" }] });
    expect(summaryItems(s).find((i) => i.id === "encryption")?.value).toBe("C: Protegido");
  });
  it("fontes indisponíveis vêm das capacidades", () => {
    expect(unavailableNetSources(healthySnapshot())).toHaveLength(1);
    expect(NOW).toBeGreaterThan(0);
  });
});
