import { expect, it } from "vitest";
import { visibleRange } from "./VirtualList";
it("bounds DOM rows while retaining access to the end of a large collection", () => {
  const start = performance.now();
  for (let index = 0; index < 10000; index++) {
    const range = visibleRange(10000, 64, index * 64, 480);
    expect(range.end - range.start).toBeLessThanOrEqual(16);
    expect(range.end).toBeLessThanOrEqual(10000);
  }
  expect(visibleRange(10000, 64, 639520, 480).end).toBe(10000);
  expect(visibleRange(0, 64, 100, 480)).toEqual({ start: 0, end: 0 });
  console.info(`10k virtual ranges + assertions: ${(performance.now() - start).toFixed(1)} ms`);
});
