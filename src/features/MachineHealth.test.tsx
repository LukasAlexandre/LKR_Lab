import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { Telemetry } from "../shared/types";
import type { MachineRegistry } from "../state/machine";
import { availability, caps, deviceOf, gpuOf, ifaceOf, telemetry } from "../shared/telemetry.fixtures";

// O componente só consome o estado de telemetria e o registro da máquina; aqui os dois são fixos.
let current: Telemetry | null = null;
vi.mock("../shared/api", () => ({ desktop: true, api: async () => null, errorText: (e: unknown) => String(e) }));
vi.mock("../state/telemetry", () => ({
  useTelemetry: () => ({ latest: current, history: [] }),
  useTelemetryWatch: () => undefined,
  refreshTelemetry: async () => undefined,
}));

const { MachineHealth } = await import("./MachineHealth");
const { MachineContext } = await import("../state/machine");

const machine = {
  status: { machine: { name: "PC Casa", lastDetectedAt: null }, snapshot: { hostname: "TRAIDE", osName: "Windows 11 Pro", cpuModel: "11th Gen Intel(R) Core(TM) i7-11370H" } },
  refreshing: false,
  refresh: async () => undefined,
} as unknown as MachineRegistry;
const render = (t: Telemetry | null) => {
  current = t;
  return renderToStaticMarkup(<MachineContext.Provider value={machine}><MachineHealth /></MachineContext.Provider>);
};

const GB = 1024 ** 3;
const full = (): Telemetry => telemetry({
  timestamp: Date.UTC(2026, 9, 4, 13, 0, 0),
  cpu: { usage: 62, clockMhz: 4136, baseMhz: 3302, cores: [70, 60, 64, 62, 70, 69, 75, 74], ready: true },
  memory: { total: 24 * GB, used: 19 * GB, available: 5 * GB, percent: 79, swapTotal: 24 * GB, swapUsed: 12 * GB, commitUsed: 36 * GB, commitLimit: 48 * GB, pagefileUsed: 3 * GB },
  gpus: [
    gpuOf({ id: "pci:0:2.0", name: "Intel(R) Iris(R) Xe Graphics", vendor: "Intel", driverVersion: "31.0.101.4502", usage: 2, dedicatedTotal: 128 * 1024 ** 2, dedicatedUsed: 0 }),
    gpuOf({ id: "pci:2:0.0", name: "NVIDIA GeForce GTX 1650", vendor: "NVIDIA", driverVersion: "32.0.16.1047", usage: 0, temperature: 61.3, dedicatedTotal: 4 * GB, dedicatedUsed: 0.2 * GB,
      capabilities: { usage: "available", dedicatedMemory: "available", sharedMemory: "available", temperature: "available" } }),
  ],
  volumes: [
    { mount: "C:\\", kind: "ssd", total: 477 * GB, available: 60 * GB, removable: false },
    { mount: "D:\\", kind: "ssd", total: 932 * GB, available: 861 * GB, removable: false },
  ],
  diskIo: {
    readPerSec: 148_000, writePerSec: 7_230_000, activity: 2, busiestDisk: "0 C:", ready: true,
    devices: [deviceOf({ model: "SAMSUNG MZALQ512HBLU" }), deviceOf({ instance: "1 D:", number: 1, model: "KINGSTON SNV2S1000G", volumes: ["D:"], readPerSec: 0, writePerSec: 0, activity: 0 })],
  },
  network: {
    interface: "Wi-Fi", ipv4: "192.168.1.42", downloadBps: 21_000, uploadBps: 8_000, ready: true,
    interfaces: [ifaceOf(), ifaceOf({ name: "Radmin VPN", kind: "ethernet", active: false, linkSpeedBps: 100_000_000, ipv4: ["26.82.176.22"], downloadBps: 0, uploadBps: 0 }),
      ifaceOf({ name: "Ethernet", kind: "ethernet", up: false, linkSpeedBps: null, active: false, ipv4: [], downloadBps: null, uploadBps: null })],
  },
  temperatures: [
    { id: "cpu", label: "CPU", source: "cpu", celsius: null, warning: null, critical: null, level: "unavailable" },
    { id: "gpu:pci:2:0.0", label: "GPU · NVIDIA GeForce GTX 1650", source: "gpu", celsius: 61.3, warning: 85, critical: 95, level: "normal" },
    { id: "disk:0", label: "NVMe · SAMSUNG", source: "storage", celsius: 45, warning: 70, critical: 80, level: "normal" },
    { id: "motherboard", label: "Placa-mãe", source: "motherboard", celsius: null, warning: null, critical: null, level: "unavailable" },
  ],
  uptime: 124_065, bootTime: 1_759_500_000,
  battery: { present: true, percent: 100, charging: false, acOnline: true, remainingSecs: null },
  capabilities: caps({ battery: "available", gpuTemperature: "available", storageTemperature: "available" }),
  availability: availability({ battery: "available" }),
});

describe("Machine Health — telemetria completa", () => {
  const html = render(full());
  it("CPU: uso, clock efetivo e base, e uma barra por núcleo", () => {
    expect(html).toContain("4,14 GHz");
    expect(html).toContain("base 3,3 GHz");
    expect((html.match(/class="mh-core"/g) ?? []).length).toBe(8);
    expect(html).toContain("8 processadores lógicos");
  });
  it("memória: RAM física separada de commit e pagefile", () => {
    expect(html).toContain("RAM em uso");
    expect(html).toContain("Commit (RAM + pagefile)");
    expect(html).toContain("36 GB / 48 GB");
    expect(html).toContain("Pagefile em uso");
    expect(html).toContain("3 GB / 24 GB");
  });
  it("cada GPU com fabricante e driver; multi-GPU e temperatura só onde existe", () => {
    expect(html).toContain("Intel(R) Iris(R) Xe Graphics");
    expect(html).toContain("Intel · driver 31.0.101.4502");
    expect(html).toContain("NVIDIA · driver 32.0.16.1047");
    expect(html).toContain("61 °C");
  });
  it("temperaturas: só sensores com leitura, o resto numa linha neutra", () => {
    const temps = html.slice(html.indexOf("mh-temps"), html.indexOf("Sensores sem leitura"));
    expect(temps).toContain("GPU · NVIDIA GeForce GTX 1650");
    expect(temps).toContain("45 °C");
    expect(temps).not.toContain("Placa-mãe");
    expect(html).toMatch(/Não disponível nesta máquina: CPU · Placa-mãe\./);
  });
  it("discos físicos (atividade) separados dos volumes (capacidade)", () => {
    expect(html).toContain("SAMSUNG MZALQ512HBLU");
    expect(html).toContain("KINGSTON SNV2S1000G");
    expect(html).toContain("ops/s");
    expect(html).toContain("livres de");
    expect(html).toContain("Capacidade dos volumes");
  });
  it("rede: interface ativa, velocidade de enlace e desconectadas recolhidas", () => {
    expect(html).toContain("574 Mbps");
    expect(html).toContain("Wi-Fi · ativa");
    expect(html).toContain("Radmin VPN");
    expect(html).toContain("1 interface desconectada");
    expect(html).toContain("192.168.1.42");
  });
  it("bateria presente: carga, tomada e saúde como 'Não disponível'", () => {
    expect(html).toContain("Bateria e energia");
    expect(html).toContain("100%");
    expect(html).toContain("Carga completa · na tomada");
    expect(html).toContain("Conectada");
    expect(html).toMatch(/Saúde da bateria<\/dt><dd[^>]*>Não disponível/);
  });
  it("uptime e última amostra aparecem", () => {
    expect(html).toContain("1d 10h");
    expect(html).toContain("Atualizando ao vivo");
  });
  it("só domínio parcial leva selo (temperaturas), e CPU completa não", () => {
    expect((html.match(/class="mh-domain"/g) ?? []).length).toBe(1);
    expect(html.slice(html.indexOf("Temperaturas"), html.indexOf("Temperaturas") + 400)).toContain("Parcial");
  });
});

describe("Machine Health — dados parciais e ausentes", () => {
  it("primeira amostra: 'Calibrando…' em vez de zeros", () => {
    const html = render(telemetry({
      cpu: { usage: 0, clockMhz: null, baseMhz: 3302, cores: [0, 0], ready: false },
      diskIo: { readPerSec: 0, writePerSec: 0, activity: null, busiestDisk: null, devices: [], ready: false },
      network: { interface: "Wi-Fi", ipv4: "1.2.3.4", downloadBps: 0, uploadBps: 0, interfaces: [], ready: false },
    }));
    expect(html).toContain("Calibrando…");
    expect(html).not.toContain("0 bps");
  });
  it("desktop sem bateria: 'Não aplicável' e nenhum card de bateria", () => {
    const html = render(telemetry());
    expect(html).not.toContain("Bateria e energia");
    expect(html).toMatch(/Bateria<\/dt><dd>Não aplicável/);
  });
  it("GPU sem métricas mostra 'Não disponível' e não inventa temperatura", () => {
    const html = render(telemetry({
      gpus: [gpuOf({ name: "GPU sem driver de métricas", usage: null, capabilities: { usage: "unavailable", dedicatedMemory: "unavailable", sharedMemory: "unavailable", temperature: "unavailable" } })],
      capabilities: caps({ gpuUsage: "unavailable", gpuMemory: "unavailable" }),
      availability: availability({ gpu: "partial" }),
    }));
    // A última ocorrência é a célula da tabela de GPUs (a primeira está no resumo do topo).
    const row = html.slice(html.lastIndexOf("GPU sem driver de métricas"));
    expect(row.slice(0, 500)).toContain("Não disponível");
    expect(row.slice(0, 500)).not.toContain("0 °C");
    expect(html).toContain("Parcial");
  });
  it("sem leitura de memória virtual: 'Não disponível', não zero", () => {
    const html = render(telemetry());
    expect(html).toMatch(/Commit \(RAM \+ pagefile\)<\/dt><dd>Não disponível/);
    expect(html).toMatch(/Pagefile em uso<\/dt><dd>Não disponível/);
  });
  it("disco sem contador por dispositivo: nenhuma lista de discos físicos", () => {
    const html = render(telemetry({ capabilities: caps({ diskPerDevice: "unavailable" }) }));
    expect(html).not.toContain("Discos físicos");
  });
  it("sem telemetria, estado de espera honesto (sem números)", () => {
    const html = render(null);
    expect(html).toContain("Coletando a telemetria desta máquina");
    expect(html).not.toContain("mh-core");
  });
  it("sempre oferece atualização manual", () => {
    expect(render(telemetry())).toContain("Atualizar agora");
  });
});
