/** Where the session is kept. Sync or async; failures are ignored (the app then runs without persistence). */
export interface AuthStorage {
  getItem(key: string): string | null | Promise<string | null>;
  setItem(key: string, value: string): void | Promise<void>;
  removeItem(key: string): void | Promise<void>;
}

export function memoryStorage(): AuthStorage {
  const map = new Map<string, string>();
  return {
    getItem: (k) => map.get(k) ?? null,
    setItem: (k, v) => void map.set(k, v),
    removeItem: (k) => void map.delete(k),
  };
}

/** `localStorage` when the page can use it, memory otherwise (servers, scripts, blocked storage). */
export function defaultStorage(): AuthStorage {
  try {
    const ls = (globalThis as { localStorage?: Storage }).localStorage;
    if (ls) {
      const probe = "__monolith_probe__";
      ls.setItem(probe, "1");
      ls.removeItem(probe);
      return ls;
    }
  } catch {
    // blocked or unavailable
  }
  return memoryStorage();
}
