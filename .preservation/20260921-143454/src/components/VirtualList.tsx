import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";

export function visibleRange(count: number, rowHeight: number, scrollTop: number, height: number, overscan = 4) {
  const start = Math.min(Math.max(0, count - 1), Math.max(0, Math.floor(scrollTop / rowHeight) - overscan));
  return { start, end: Math.min(count, start + Math.ceil(height / rowHeight) + overscan * 2) };
}

/** Fixed-height rows, native scrolling and keyboard paging; no silent truncation. */
export function VirtualList<T>({ items, rowHeight, height = 480, label, itemKey, children }: {
  items: T[]; rowHeight: number; height?: number; label: string;
  itemKey: (item: T) => string | number; children: (item: T) => ReactNode;
}) {
  const [scroll, setScroll] = useState(0);
  const viewport = useRef<HTMLDivElement>(null);
  const lastItems = useRef(items);
  useEffect(() => {
    if (lastItems.current !== items) {
      lastItems.current = items;
      if (viewport.current) {
        const top = Math.min(viewport.current.scrollTop, Math.max(0, items.length * rowHeight - height));
        viewport.current.scrollTop = top;
        setScroll(top);
      }
    }
  }, [items, rowHeight, height]);
  const range = visibleRange(items.length, rowHeight, scroll, height);
  return <div ref={viewport} role="list" aria-label={label} tabIndex={0} className="virtual-list"
    style={{ height: Math.min(items.length * rowHeight, height), overflow: "auto", position: "relative" }}
    onScroll={event => setScroll(event.currentTarget.scrollTop)}>
    <div style={{ height: items.length * rowHeight, position: "relative" }}>
      {items.slice(range.start, range.end).map((item, offset) => <div role="listitem" aria-posinset={range.start + offset + 1} aria-setsize={items.length}
        key={itemKey(item)} style={{ position: "absolute", top: (range.start + offset) * rowHeight, height: rowHeight, width: "100%" }}>{children(item)}</div>)}
    </div>
  </div>;
}
