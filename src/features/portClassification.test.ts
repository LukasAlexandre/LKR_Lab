import { expect, it } from "vitest";
import { portKind } from "./portClassification";
import type { PortInfo } from "../shared/types";
const port: PortInfo = { port: 80, address: "0.0.0.0", protocol: "TCP", pid: 100, process: "nginx.exe", executable: null, startTime: 1, projectId: null, expectedBy: [], confidence: "unknown", conflict: false };
it("does not infer system ownership from privileged port numbers", () => {
  expect(portKind(port)).toBe("unknown");
  expect(portKind({ ...port, process: "svchost.exe" })).toBe("system");
});
it("keeps declared expectation distinct from observed project ownership", () => {
  expect(portKind({ ...port, expectedBy: ["a"] })).toBe("expected");
  expect(portKind({ ...port, projectId: "a", expectedBy: ["a"] })).toBe("project");
  expect(portKind({ ...port, projectId: "a", expectedBy: ["b"] })).toBe("unexpected");
});
