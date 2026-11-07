// Minimal typing so the SDK builds without @types/react (react is an optional peer dependency).
declare module "react" {
  export type ReactNode = unknown;
  export type ReactElement = unknown;
  export function createElement(type: unknown, props?: Record<string, unknown> | null, ...children: unknown[]): ReactElement;
  export function useEffect(effect: () => void | (() => void), deps?: readonly unknown[]): void;
  export function useRef<T>(initial: T): { current: T };
  export function useCallback<T extends (...args: never[]) => unknown>(fn: T, deps: readonly unknown[]): T;
  export function useSyncExternalStore<T>(subscribe: (cb: () => void) => () => void, getSnapshot: () => T, getServerSnapshot?: () => T): T;
}
