import type {
  BatteryTelemetry,
  DiskDevice,
  DomainAvailability,
  NetworkInterfaceTelemetry,
  Telemetry,
  TelemetryCapabilities,
} from "./types";

/* Dados de teste do contrato de telemetria. Sensor ausente é `null`/"unavailable", como no backend. */

export const caps = (patch: Partial<TelemetryCapabilities> = {}): TelemetryCapabilities => ({
  cpuUsage: "available", cpuClock: "available", memoryUsage: "available", gpuUsage: "available",
  gpuMemory: "available", gpuProcessUsage: "available", cpuPackageTemperature: "unavailable",
  gpuTemperature: "unavailable", storageTemperature: "unavailable", thermalZoneTemperature: "unavailable",
  motherboardTemperature: "unavailable", diskIo: "available", diskActivity: "available",
  networkRate: "available", processDiskIo: "available", storagePhysicalHealth: "unavailable",
  cpuPerCore: "available", cpuBaseClock: "available", memoryCommit: "available", battery: "unavailable",
  batteryHealth: "unavailable", diskPerDevice: "available", networkInterfaces: "available", ...patch,
});

export const availability = (patch: Partial<DomainAvailability> = {}): DomainAvailability => ({
  cpu: "available", memory: "available", gpu: "available", disk: "available", network: "available",
  battery: "unavailable", temperatures: "partial", ...patch,
});

export const noBattery = (): BatteryTelemetry => ({
  present: false, percent: null, charging: null, acOnline: true, remainingSecs: null,
});

export const gpuOf = (patch: Partial<Telemetry["gpus"][number]> = {}): Telemetry["gpus"][number] => ({
  id: "pci:0:2.0", name: "gpu", usage: null, dedicatedUsed: null, dedicatedTotal: null, sharedUsed: null, sharedTotal: null,
  temperature: null, vendor: null, driverVersion: null,
  capabilities: { usage: "available", dedicatedMemory: "available", sharedMemory: "available", temperature: "unavailable" },
  ...patch,
});

export const deviceOf = (patch: Partial<DiskDevice> = {}): DiskDevice => ({
  instance: "0 C:", number: 0, model: "SSD de teste", nvme: true, volumes: ["C:"],
  readPerSec: 1_000_000, writePerSec: 2_000_000, readOpsPerSec: 10, writeOpsPerSec: 20, activity: 12, ...patch,
});

export const ifaceOf = (patch: Partial<NetworkInterfaceTelemetry> = {}): NetworkInterfaceTelemetry => ({
  name: "Wi-Fi", description: null, kind: "wifi", up: true, linkSpeedBps: 574_000_000, ipv4: ["192.168.1.42"],
  ipv6: [], receivedBytes: 1000, sentBytes: 500, downloadBps: 4_000_000, uploadBps: 1_000_000, active: true, ...patch,
});

export const telemetry = (patch: Partial<Telemetry> = {}): Telemetry => ({
  timestamp: 1000, active: true,
  cpu: { usage: 10, clockMhz: null, baseMhz: null, cores: [], ready: true },
  memory: {
    total: 100, used: 40, available: 60, percent: 40, swapTotal: 0, swapUsed: 0,
    commitUsed: null, commitLimit: null, pagefileUsed: null,
  },
  gpus: [],
  diskIo: { readPerSec: 0, writePerSec: 0, activity: null, busiestDisk: null, devices: [], ready: true },
  volumes: [],
  network: { interface: null, ipv4: null, downloadBps: 0, uploadBps: 0, interfaces: [], ready: true },
  temperatures: [], processes: { cpu: [], memory: [], gpu: [], disk: [], total: 0 },
  uptime: 0, bootTime: 0, battery: noBattery(), capabilities: caps(), availability: availability(),
  health: { status: "healthy", alerts: [], checks: [] }, ...patch,
});
