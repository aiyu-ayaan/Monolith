// Shared UI primitives: icons, drawer, toasts, auth card, password input.
// No component library: keeps the embedded bundle small (doc 15).

import { createSignal, For, onCleanup, onMount, Show, type JSX } from "solid-js";
import { Dynamic } from "solid-js/web";

// Lucide icon paths (ISC license), inlined so the UI makes no external requests.
const ICONS = {
  // Monolith Keystone M (logo/monolith-symbol.svg), 256 grid scaled to 24.
  logo: '<path transform="scale(0.09375)" fill="currentColor" stroke="none" d="M58 24H98L128 136L158 24H198A8 8 0 0 1 206 32V224A8 8 0 0 1 198 232H58A8 8 0 0 1 50 224V32A8 8 0 0 1 58 24Z"/>',
  layers:
    '<path d="m12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83Z"/><path d="m22 17.65-9.17 4.16a2 2 0 0 1-1.66 0L2 17.65"/><path d="m22 12.65-9.17 4.16a2 2 0 0 1-1.66 0L2 12.65"/>',
  database: '<ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M3 5V19A9 3 0 0 0 21 19V5"/><path d="M3 12A9 3 0 0 0 21 12"/>',
  folder:
    '<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>',
  plus: '<path d="M5 12h14"/><path d="M12 5v14"/>',
  trash: '<path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/>',
  settings: '<path d="M20 7h-9"/><path d="M14 17H5"/><circle cx="17" cy="17" r="3"/><circle cx="7" cy="7" r="3"/>',
  logout: '<path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><path d="m16 17 5-5-5-5"/><path d="M21 12H9"/>',
  search: '<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>',
  x: '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>',
  check: '<path d="M20 6 9 17l-5-5"/>',
  chevronLeft: '<path d="m15 18-6-6 6-6"/>',
  chevronRight: '<path d="m9 18 6-6-6-6"/>',
  refresh: '<path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/>',
  lock: '<rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>',
  alert: '<circle cx="12" cy="12" r="10"/><path d="M12 8v4"/><path d="M12 16h.01"/>',
  eye: '<path d="M2.06 12.35a1 1 0 0 1 0-.7 10.75 10.75 0 0 1 19.88 0 1 1 0 0 1 0 .7 10.75 10.75 0 0 1-19.88 0"/><circle cx="12" cy="12" r="3"/>',
  eyeOff:
    '<path d="M10.73 5.08A10.43 10.43 0 0 1 12 5c7 0 10 7 10 7a13.16 13.16 0 0 1-1.67 2.68"/><path d="M6.61 6.61A13.53 13.53 0 0 0 2 12s3 7 10 7a9.74 9.74 0 0 0 5.39-1.61"/><path d="M14.12 14.12a3 3 0 1 1-4.24-4.24"/><path d="m2 2 20 20"/>',
  chevronDown: '<path d="m6 9 6 6 6-6"/>',
  storage:
    '<line x1="22" x2="2" y1="12" y2="12"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/><line x1="6" x2="6.01" y1="16" y2="16"/><line x1="10" x2="10.01" y1="16" y2="16"/>',
  radio:
    '<path d="M4.9 19.1C1 15.2 1 8.8 4.9 4.9"/><path d="M7.8 16.2c-2.3-2.3-2.3-6.1 0-8.5"/><circle cx="12" cy="12" r="2"/><path d="M16.2 7.8c2.3 2.3 2.3 6.1 0 8.5"/><path d="M19.1 4.9C23 8.8 23 15.1 19.1 19"/>',
  shield:
    '<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/>',
  plug: '<path d="M12 22v-5"/><path d="M9 8V2"/><path d="M15 8V2"/><path d="M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8Z"/>',
  archive: '<rect width="20" height="5" x="2" y="3" rx="1"/><path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8"/><path d="M10 12h4"/>',
  list: '<path d="M3 12h.01"/><path d="M3 18h.01"/><path d="M3 6h.01"/><path d="M8 12h13"/><path d="M8 18h13"/><path d="M8 6h13"/>',
  key: '<path d="m21 2-9.6 9.6"/><circle cx="7.5" cy="15.5" r="5.5"/><path d="m15.5 7.5 3 3L22 7l-3-3"/>',
  home: '<rect width="7" height="9" x="3" y="3" rx="1"/><rect width="7" height="5" x="14" y="3" rx="1"/><rect width="7" height="9" x="14" y="12" rx="1"/><rect width="7" height="5" x="3" y="16" rx="1"/>',
  copy: '<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>',
  chart: '<path d="M3 3v16a2 2 0 0 0 2 2h16"/><path d="m19 9-5 5-4-4-3 3"/>',
};
export type IconName = keyof typeof ICONS;

export function Icon(p: { name: IconName; size?: number }) {
  return (
    <svg
      class="icon"
      width={p.size ?? 18}
      height={p.size ?? 18}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.75"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
      innerHTML={ICONS[p.name]}
    />
  );
}

export const fmtDate = (s: string) => {
  const d = new Date(s);
  return Number.isNaN(d.getTime()) ? s : d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
};

// ---------- toasts ----------

const [toasts, setToasts] = createSignal<{ id: number; text: string }[]>([]);
let toastSeq = 0;

export function toast(text: string) {
  const id = ++toastSeq;
  setToasts((t) => [...t, { id, text }]);
  setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 3500);
}

export function Toasts() {
  return (
    <div class="toasts" role="status" aria-live="polite">
      <For each={toasts()}>
        {(t) => (
          <div class="toast">
            <Icon name="check" size={16} />
            {t.text}
          </div>
        )}
      </For>
    </div>
  );
}

// ---------- drawer ----------

/** Right-side panel (PocketBase-style). Esc or backdrop closes; focus returns to the opener. */
// shortcut: no focus trap; Tab can leave the panel. Add one if keyboard users report it.
export function Drawer(p: {
  title: JSX.Element;
  onClose: () => void;
  onSubmit?: (e: SubmitEvent) => void;
  footer?: JSX.Element;
  wide?: boolean;
  children: JSX.Element;
}) {
  let panel!: HTMLElement;
  const opener = document.activeElement as HTMLElement | null;
  const onKey = (e: KeyboardEvent) => e.key === "Escape" && p.onClose();
  onMount(() => {
    document.addEventListener("keydown", onKey);
    (panel.querySelector<HTMLElement>(".drawer-body :is(input:not([disabled]),select,textarea,button)") ?? panel).focus();
  });
  onCleanup(() => {
    document.removeEventListener("keydown", onKey);
    opener?.focus();
  });
  return (
    <div class="drawer-wrap">
      <div class="backdrop" onClick={() => p.onClose()} />
      <Dynamic
        component={p.onSubmit ? "form" : "div"}
        ref={(el: HTMLElement) => (panel = el)}
        class="drawer"
        classList={{ wide: p.wide }}
        role="dialog"
        aria-modal="true"
        aria-labelledby="drawer-title"
        tabindex="-1"
        onSubmit={p.onSubmit}
      >
        <header class="drawer-head">
          <h2 id="drawer-title">{p.title}</h2>
          <button type="button" class="icon-btn" aria-label="Close" onClick={() => p.onClose()}>
            <Icon name="x" />
          </button>
        </header>
        <div class="drawer-body">{p.children}</div>
        <Show when={p.footer}>
          <footer class="drawer-foot">{p.footer}</footer>
        </Show>
      </Dynamic>
    </div>
  );
}

// ---------- auth ----------

export function AuthCard(p: { title: string; subtitle: string; children: JSX.Element }) {
  return (
    <main class="auth">
      <div class="auth-card">
        <div class="auth-brand">
          <Icon name="logo" size={26} />
          <span>Monolith</span>
        </div>
        <h1>{p.title}</h1>
        <p class="muted">{p.subtitle}</p>
        {p.children}
      </div>
    </main>
  );
}

export function PasswordInput(p: {
  id: string;
  value: string;
  onInput: (v: string) => void;
  autocomplete: string;
  minLength?: number;
  describedBy?: string;
}) {
  const [show, setShow] = createSignal(false);
  return (
    <div class="affix">
      <input
        id={p.id}
        type={show() ? "text" : "password"}
        required
        minLength={p.minLength}
        autocomplete={p.autocomplete}
        aria-describedby={p.describedBy}
        value={p.value}
        onInput={(e) => p.onInput(e.currentTarget.value)}
      />
      <button
        type="button"
        class="affix-btn"
        aria-label={show() ? "Hide password" : "Show password"}
        aria-pressed={show()}
        onClick={() => setShow(!show())}
      >
        <Icon name={show() ? "eyeOff" : "eye"} size={16} />
      </button>
    </div>
  );
}
