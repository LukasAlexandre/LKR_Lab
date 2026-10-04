import { describe, expect, it } from "vitest";
import {
  WARMING,
  batteryStatus,
  coreBars,
  cpuClockDetail,
  diskRows,
  domainNote,
  gpuIdentity,
  memoryVirtual,
  rateText,
  splitInterfaces,
  splitTemperatures,
  formatBits,
  formatRate,
} from "./telemetry";
import { deviceOf, gpuOf, ifaceOf, telemetry } from "./telemetry.fixtures";
import type { TemperatureReading } from "./types";

const GB = 1024 ** 3;

describe("CPU: núcleos, clock e calibração", () => {
  it("primeira amostra é 'calibrando', nunca um uso inventado", () => {
    expect(cpuClockDetail({ usage: 0, clockMhz: 3000, baseMhz: 3302, cores: [], ready: false })).toBe(WARMING);
  });
  it("mostra clock efetivo e base quando existem", () => {
    const detail = cpuClockDetail({ usage: 5, clockMhz: 4136, baseMhz: 3302, cores: [], ready: true });
    expect(detail).toContain("4,14 GHz");
    expect(detail).toContain("base 3,3 GHz");
  });
  it("sem clock efetivo usa só a base; sem nenhum, cai no rótulo neutro", () => {
    expect(cpuClockDetail({ usage: 5, clockMhz: null, baseMhz: 3302, cores: [], ready: true })).toBe("base 3,3 GHz");
    expect(cpuClockDetail({ usage: 5, clockMhz: null, baseMhz: null, cores: [], ready: true })).toBe("Uso atual");
  });
  it("uma barra por núcleo, limitada a 0–100", () => {
    const bars = coreBars([10, 250, -5, Number.NaN]);
    expect(bars.map((b) => b.percent)).toEqual([10, 100, 0, 0]);
    expect(bars.map((b) => b.index)).toEqual([0, 1, 2, 3]);
  });
});

describe("memória: RAM física separada de commit e pagefile", () => {
  it("formata commit e pagefile quando o Windows informa", () => {
    const { commit, pagefile } = memoryVirtual({
      ...telemetry().memory,
      swapTotal: 24 * GB, commitUsed: 36 * GB, commitLimit: 48 * GB, pagefileUsed: 3 * GB,
    });
    expect(commit).toBe("36 GB / 48 GB");
    expect(pagefile).toBe("3 GB / 24 GB");
  });
  it("sem leitura não há valor (nada de 0 B)", () => {
    expect(memoryVirtual(telemetry().memory)).toEqual({ commit: null, pagefile: null });
    expect(memoryVirtual({ ...telemetry().memory, commitUsed: 5, commitLimit: 0 }).commit).toBeNull();
  });
});

describe("GPU: identidade só com o que o Windows informou", () => {
  it("fabricante e driver", () => {
    expect(gpuIdentity(gpuOf({ vendor: "NVIDIA", driverVersion: "32.0.16.1047" }))).toBe("NVIDIA · driver 32.0.16.1047");
    expect(gpuIdentity(gpuOf({ vendor: "Intel" }))).toBe("Intel");
  });
  it("sem nenhum dado fica ausente", () => {
    expect(gpuIdentity(gpuOf())).toBeNull();
  });
});

describe("temperaturas: só sensores encontrados", () => {
  const reading = (id: string, label: string, celsius: number | null): TemperatureReading => ({
    id, label, source: "gpu", celsius, warning: null, critical: null, level: celsius == null ? "unavailable" : "normal",
  });
  it("separa o que tem leitura do que a máquina não entrega", () => {
    const { found, missing } = splitTemperatures([
      reading("cpu", "CPU", null),
      reading("gpu", "GPU · NVIDIA", 61.3),
      reading("disk:0", "NVMe · Samsung", 45),
      reading("motherboard", "Placa-mãe", null),
    ]);
    expect(found.map((r) => r.id)).toEqual(["gpu", "disk:0"]);
    expect(missing).toEqual(["CPU", "Placa-mãe"]);
  });
  it("zero graus real é uma leitura; null não é", () => {
    expect(splitTemperatures([reading("a", "A", 0)]).found).toHaveLength(1);
    expect(splitTemperatures([reading("a", "A", null)]).found).toHaveLength(0);
  });
});

describe("taxas: calibrando × traço × valor", () => {
  it("primeira amostra mostra 'calibrando'", () => {
    expect(rateText(0, false, formatBits)).toBe(WARMING);
    expect(rateText(null, false, formatRate)).toBe(WARMING);
  });
  it("sem valor depois de calibrar é traço; com valor é formatado", () => {
    expect(rateText(null, true, formatBits)).toBe("—");
    expect(rateText(4_000_000, true, formatBits)).toBe("4 Mbps");
    expect(rateText(2048, true, formatRate)).toBe("2 KB/s");
  });
});

describe("rede: interfaces conectadas × desconectadas", () => {
  it("separa pelo estado e preserva a ordem (ativa primeiro)", () => {
    const list = [
      ifaceOf({ name: "Wi-Fi", active: true }),
      ifaceOf({ name: "Radmin VPN", kind: "ethernet", active: false }),
      ifaceOf({ name: "Ethernet", kind: "ethernet", up: false, linkSpeedBps: null }),
      ifaceOf({ name: "Desconhecida", up: null }),
    ];
    const { connected, idle } = splitInterfaces(list);
    expect(connected.map((i) => i.name)).toEqual(["Wi-Fi", "Radmin VPN"]);
    expect(idle.map((i) => i.name)).toEqual(["Ethernet", "Desconhecida"]);
  });
});

describe("discos: físico (atividade) × volume (capacidade)", () => {
  const t = telemetry({
    volumes: [
      { mount: "C:\\", kind: "ssd", total: 477 * GB, available: 60 * GB, removable: false },
      { mount: "D:\\", kind: "ssd", total: 932 * GB, available: 861 * GB, removable: false },
    ],
  });
  it("liga cada disco físico aos volumes das suas letras", () => {
    const rows = diskRows({
      ...t,
      diskIo: {
        ...t.diskIo,
        devices: [deviceOf(), deviceOf({ instance: "1 D:", number: 1, model: "KINGSTON", volumes: ["D:"] })],
      },
    });
    expect(rows.map((r) => r.label)).toEqual(["SSD de teste", "KINGSTON"]);
    expect(rows[0].volumes.map((v) => v.mount)).toEqual(["C:\\"]);
    expect(rows[0].total).toBe(477 * GB);
    expect(rows[1].available).toBe(861 * GB);
  });
  it("sem modelo usa 'Disco N'; sem volume a capacidade fica zerada, sem inventar", () => {
    const rows = diskRows({ ...t, diskIo: { ...t.diskIo, devices: [deviceOf({ model: null, number: 3, volumes: [] })] } });
    expect(rows[0].label).toBe("Disco 3");
    expect(rows[0].volumes).toEqual([]);
    expect(rows[0].total).toBe(0);
  });
  it("sem dispositivos não há linhas", () => {
    expect(diskRows(t)).toEqual([]);
  });
});

describe("bateria e energia", () => {
  it("desktop é 'não aplicável', nunca 0%", () => {
    const status = batteryStatus({ present: false, percent: null, charging: null, acOnline: true, remainingSecs: null });
    expect(status.applicable).toBe(false);
    expect(status.percent).toBeNull();
    expect(status.state).toContain("não tem bateria");
  });
  it("notebook: carregando, na tomada, completa e na bateria", () => {
    const base = { present: true, percent: 80, charging: false, acOnline: true, remainingSecs: null };
    expect(batteryStatus({ ...base, charging: true }).state).toBe("Carregando");
    expect(batteryStatus(base).state).toBe("Na tomada");
    expect(batteryStatus({ ...base, percent: 100 }).state).toBe("Carga completa · na tomada");
    const onBattery = batteryStatus({ ...base, acOnline: false, remainingSecs: 7_800 });
    expect(onBattery.state).toBe("Usando a bateria");
    expect(onBattery.remaining).toBe("2h 10min restantes");
    expect(onBattery.percent).toBe("80%");
  });
  it("sem leitura de percentual não vira 0%", () => {
    expect(batteryStatus({ present: true, percent: null, charging: null, acOnline: null, remainingSecs: null }).percent).toBeNull();
  });
});

describe("domínios", () => {
  it("só domínio incompleto ganha selo", () => {
    expect(domainNote("available")).toBeNull();
    expect(domainNote(undefined)).toBeNull();
    expect(domainNote("partial")).toBe("Parcial");
    expect(domainNote("unavailable")).toBe("Não disponível");
  });
});
