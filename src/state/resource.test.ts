import { describe, expect, it, vi } from "vitest";
import { createResource } from "./resource";

describe("independent workspace resources", () => {
  it("deduplicates concurrent reads and respects freshness", async () => {
    const fetcher = vi.fn(async () => [1]);
    const resource = createResource<number[]>([], fetcher);
    const first = resource.refresh();
    expect(resource.refresh()).toBe(first);
    await first;
    await resource.refresh(30_000);
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(resource.getSnapshot().data).toEqual([1]);
  });
  it("keeps stale data when revalidation fails", async () => {
    const fetcher = vi.fn<() => Promise<number[]>>().mockResolvedValueOnce([9]).mockRejectedValueOnce(new Error("offline"));
    const resource = createResource<number[]>([], fetcher);
    await resource.refresh();
    const timestamp = resource.getSnapshot().lastUpdated;
    await resource.refresh();
    expect(resource.getSnapshot()).toMatchObject({ data: [9], error: "offline", status: "error", loading: false, lastUpdated: timestamp });
  });
  it("a slow source does not block another source or publish to its subscribers", async () => {
    let finish!: (value: string) => void;
    const slow = createResource("", () => new Promise<string>((resolve) => { finish = resolve; }));
    const fast = createResource("", async () => "ready");
    const listener = vi.fn();
    const unsubscribe = slow.subscribe(listener);
    const pending = slow.refresh();
    const calls = listener.mock.calls.length;
    await fast.refresh();
    expect(fast.getSnapshot().data).toBe("ready");
    expect(slow.getSnapshot().loading).toBe(true);
    expect(listener).toHaveBeenCalledTimes(calls);
    unsubscribe(); finish("done"); await pending;
    expect(listener).toHaveBeenCalledTimes(calls);
  });
});
