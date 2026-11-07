import { createElement, useCallback, useEffect, useRef, useSyncExternalStore } from "react";
import type { ReactNode } from "react";
import type { AppUser, AuthState } from "../auth/app.js";
import { type AuthApi, type AuthUiOptions, mountAuth, type AuthMount } from "../ui/index.js";

export type { AuthUiOptions } from "../ui/index.js";

type Mono = { auth: AuthApi };

export interface UseAuth {
  state: AuthState;
  user: AppUser | null;
  /** The session is still being restored. */
  loading: boolean;
  signedIn: boolean;
  signOut(): Promise<void>;
}

/** Subscribes to the auth state of a client. Re-renders on every change. */
export function useAuth(mono: Mono): UseAuth {
  const subscribe = useCallback((cb: () => void) => mono.auth.onChange(() => cb()), [mono]);
  const state = useSyncExternalStore(
    subscribe,
    () => mono.auth.state,
    () => mono.auth.state,
  );
  return {
    state,
    user: state.user,
    loading: state.status === "unknown",
    signedIn: state.status === "signedIn",
    signOut: () => mono.auth.signOut(),
  };
}

export interface MonolithAuthProps {
  mono: Mono;
  options?: AuthUiOptions;
}

/** The drop-in sign-in screen (the same one as `<monolith-auth>`). */
export function MonolithAuth(props: MonolithAuthProps): ReactNode {
  const host = useRef<HTMLElement | null>(null);
  const mount = useRef<AuthMount | null>(null);
  const latest = useRef(props.options);
  latest.current = props.options;

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    // Callbacks go through `latest` so inline arrow functions do not matter.
    const stable: AuthUiOptions = {
      ...latest.current,
      onSignedIn: (u) => latest.current?.onSignedIn?.(u),
      onError: (e) => latest.current?.onError?.(e),
    };
    const m = mountAuth(el, props.mono, stable);
    mount.current = m;
    return () => {
      m.destroy();
      mount.current = null;
    };
  }, [props.mono]);

  useEffect(() => {
    const o = props.options;
    if (!mount.current || !o) return;
    mount.current.replace({
      ...o,
      onSignedIn: (u) => latest.current?.onSignedIn?.(u),
      onError: (e) => latest.current?.onError?.(e),
    });
  });

  return createElement("div", { ref: host });
}

export interface AuthGateProps {
  mono: Mono;
  /** Shown while signed out. Default: `<MonolithAuth mono options={options}>`. */
  fallback?: ReactNode;
  options?: AuthUiOptions;
  /** Shown while the session is restored. Default: nothing. */
  loading?: ReactNode;
  children?: ReactNode;
}

/** Shows the sign-in screen until signed in, then the children. */
export function AuthGate(props: AuthGateProps): ReactNode {
  const { state } = useAuth(props.mono);
  if (state.status === "unknown") return props.loading ?? null;
  if (state.status === "signedOut") {
    return props.fallback ?? createElement(MonolithAuth, { mono: props.mono, options: props.options });
  }
  return props.children ?? null;
}
