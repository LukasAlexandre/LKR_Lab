import { useEffect, useState } from "react";

export type Density = "comfortable" | "compact";

export function usePersistentState<T>(key: string, initial: T) {
  const [value, setValue] = useState<T>(() => {
    try {
      const stored = window.localStorage.getItem(key);
      return stored === null ? initial : (JSON.parse(stored) as T);
    } catch {
      return initial;
    }
  });

  useEffect(() => {
    try { window.localStorage.setItem(key, JSON.stringify(value)); }
    catch { /* Storage failure must not break navigation. */ }
  }, [key, value]);

  return [value, setValue] as const;
}
