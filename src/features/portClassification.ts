import type { PortInfo } from "../shared/types";
export const protectedProcesses = new Set([
  "system", "registry", "smss.exe", "csrss.exe", "wininit.exe", "services.exe",
  "lsass.exe", "winlogon.exe", "svchost.exe", "spoolsv.exe",
]);
export type PortKind = "project" | "expected" | "unexpected" | "unknown" | "system";
export function portKind(port: PortInfo): PortKind {
  if (port.projectId) return port.expectedBy.includes(port.projectId) ? "project" : "unexpected";
  if (port.expectedBy.length) return "expected";
  if (protectedProcesses.has(port.process.toLowerCase()) || (port.pid !== null && port.pid <= 4)) return "system";
  // A low port alone is not evidence of a system service (e.g. nginx :80).
  return "unknown";
}
