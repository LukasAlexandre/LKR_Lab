export type ResourceStatus = "idle" | "loading" | "ready" | "error";
export interface ResourceSnapshot<T> {
  data: T;
  status: ResourceStatus;
  loading: boolean;
  error: string | null;
  lastUpdated: number | null;
  durationMs: number | null;
}

/** Independent SWR source: stable snapshots, deduplicated reads, retained data on failure. */
export function createResource<T>(initial: T, fetcher: () => Promise<T>) {
  let snapshot: ResourceSnapshot<T> = {
    data: initial, status: "idle", loading: false, error: null,
    lastUpdated: null, durationMs: null,
  };
  const listeners = new Set<() => void>();
  let pending: Promise<void> | null = null;
  const publish = (update: Partial<ResourceSnapshot<T>>) => {
    snapshot = { ...snapshot, ...update };
    listeners.forEach((listener) => listener());
  };
  return {
    getSnapshot: () => snapshot,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    refresh(maxAgeMs = 0): Promise<void> {
      if (pending) return pending;
      if (snapshot.lastUpdated !== null && Date.now() - snapshot.lastUpdated < maxAgeMs)
        return Promise.resolve();
      const started = performance.now();
      // Schedule the fetch after pending is assigned, including synchronous reentrant subscribers.
      pending = Promise.resolve().then(fetcher).then(
        (data) => publish({ data, status: "ready", error: null, lastUpdated: Date.now() }),
        (error: unknown) => publish({ status: "error", error: error instanceof Error ? error.message : String(error) }),
      ).finally(() => {
        pending = null;
        publish({ loading: false, durationMs: performance.now() - started });
      });
      publish({ loading: true, status: "loading", error: null });
      return pending;
    },
  };
}
export type Resource<T> = ReturnType<typeof createResource<T>>;
