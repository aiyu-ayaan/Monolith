import { createSignal, onCleanup, Show } from "solid-js";
import { me, setToken } from "../api";
import { Icon } from "../ui";

/** Signed-in superuser chip with a menu (console settings, sign out). `compact` shows the avatar only. */
export default function UserMenu(p: { compact?: boolean; settingsHref: string }) {
  const [open, setOpen] = createSignal(false);
  let root!: HTMLDivElement;
  const email = () => me()?.email ?? "";
  const close = (e: Event) => {
    if (e instanceof KeyboardEvent ? e.key === "Escape" : !root.contains(e.target as Node)) setOpen(false);
  };
  document.addEventListener("pointerdown", close);
  document.addEventListener("keydown", close);
  onCleanup(() => {
    document.removeEventListener("pointerdown", close);
    document.removeEventListener("keydown", close);
  });

  return (
    <div class="user-menu" classList={{ compact: p.compact }} ref={root}>
      <button class="user-chip" aria-haspopup="menu" aria-expanded={open()} title={email()} onClick={() => setOpen(!open())}>
        <span class="avatar" aria-hidden="true">
          {(email()[0] ?? "?").toUpperCase()}
        </span>
        <Show when={!p.compact}>
          <span class="user-text min0">
            <span class="truncate">{email() || "Signed in"}</span>
            <span class="muted small">Superuser</span>
          </span>
          <Icon name="chevronDown" size={14} />
        </Show>
      </button>
      <Show when={open()}>
        <div class="user-pop" role="menu">
          <div class="user-pop-head">
            <strong class="truncate">{email()}</strong>
            <span class="muted small">Full access to every project</span>
          </div>
          <a role="menuitem" class="nav-item" href={p.settingsHref} onClick={() => setOpen(false)}>
            <Icon name="users" size={16} />
            <span>Users and permissions</span>
          </a>
          <button role="menuitem" class="nav-item" onClick={() => setToken(null)}>
            <Icon name="logout" size={16} />
            <span>Sign out</span>
          </button>
        </div>
      </Show>
    </div>
  );
}
