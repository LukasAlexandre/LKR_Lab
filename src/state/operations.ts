import { useSyncExternalStore } from "react";

export interface Operation {
  id: number; label: string; projectId?: string; status: "running" | "completed" | "error";
  startedAt: number; finishedAt?: number; error?: string;
}
let sequence = 0;
let snapshot: Operation[] = [];
const listeners = new Set<() => void>();
function publish(next: Operation[]) { snapshot = next; listeners.forEach(listener => listener()); }
const subscribe = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };
export function useOperations() { return useSyncExternalStore(subscribe, () => snapshot); }
export async function trackOperation<T>(label: string, run: () => Promise<T>, projectId?: string): Promise<T> {
  const operation: Operation = { id: ++sequence, label, projectId, status: "running", startedAt: Date.now() };
  // Retain active operations, but coalesce repeated completed refreshes and cap history.
  publish([operation, ...snapshot.filter(item => item.status === "running"), ...snapshot.filter(item => item.status !== "running" && (item.label !== label || item.projectId !== projectId)).slice(0, 49)]);
  try {
    const result = await run();
    publish(snapshot.map(item => item.id === operation.id ? { ...item, status: "completed", finishedAt: Date.now() } : item));
    return result;
  } catch (error) {
    publish(snapshot.map(item => item.id === operation.id ? { ...item, status: "error", finishedAt: Date.now(), error: error instanceof Error ? error.message : String(error) } : item));
    throw error;
  }
}
