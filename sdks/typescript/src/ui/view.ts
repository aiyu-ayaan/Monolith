import type { AppUser, PublicProvider } from "../auth/app.js";
import {
  type AuthApi,
  type AuthController,
  type AuthControllerOptions,
  type AuthUiState,
  createAuthController,
  selectAuthView,
} from "./controller.js";
import { type AuthLabels, fill, resolveLabels } from "./labels.js";
import { type ProviderBrand, brandFor } from "./providers.js";

export interface AuthTheme {
  accent?: string;
  accentText?: string;
  background?: string;
  foreground?: string;
  muted?: string;
  border?: string;
  danger?: string;
  /** CSS length, e.g. `12px`. */
  radius?: string;
  font?: string;
  /** Provider and submit buttons. Default `filled` for submit, brand colors for providers. */
  buttonStyle?: "filled" | "outline";
  /** `auto` follows `prefers-color-scheme`. */
  mode?: "auto" | "light" | "dark";
}

export interface ProviderButtonContext {
  provider: PublicProvider;
  brand: ProviderBrand;
  /** "Continue with {name}" in the active language. */
  label: string;
  /** This provider is being used right now. */
  busy: boolean;
  /** Another request is running. */
  disabled: boolean;
  signIn(): void;
}

export interface AuthUiOptions extends AuthControllerOptions {
  /** Header title; default "Sign in to {app}" / "Create your {app} account". */
  title?: string;
  subtitle?: string;
  /** Image URL, or an element. */
  logo?: string | Node;
  termsUrl?: string;
  privacyUrl?: string;
  /** `card` (default), `fullscreen`, or `plain` (no chrome). */
  layout?: "card" | "fullscreen" | "plain";
  theme?: AuthTheme;
  /** Replace the button of a provider. Return `null`/`undefined` to keep the built-in one. */
  providerButton?: (ctx: ProviderButtonContext) => Node | string | null | undefined;
}

export interface AuthMount {
  readonly controller: AuthController;
  /** Merges new options and redraws. */
  update(options: Partial<AuthUiOptions>): void;
  /** Replaces all options and redraws. */
  replace(options: AuthUiOptions): void;
  destroy(): void;
}

type Attr = string | number | boolean | null | undefined | ((ev: Event) => void);
type Child = Node | string | null | false | undefined;

export const STYLE = `
:host{display:block}
.root{--d-bg:#fff;--d-fg:#111827;--d-muted:#6b7280;--d-border:#d1d5db;--d-surface:#fff;--d-accent:#111827;--d-accent-fg:#fff;--d-danger:#b91c1c;--d-danger-bg:#fef2f2;
--a-bg:var(--monolith-auth-bg,var(--d-bg));--a-fg:var(--monolith-auth-fg,var(--d-fg));--a-muted:var(--monolith-auth-muted,var(--d-muted));--a-border:var(--monolith-auth-border,var(--d-border));
--a-surface:var(--monolith-auth-surface,var(--d-surface));--a-accent:var(--monolith-auth-accent,var(--d-accent));--a-accent-fg:var(--monolith-auth-accent-fg,var(--d-accent-fg));
--a-danger:var(--monolith-auth-danger,var(--d-danger));--a-danger-bg:var(--d-danger-bg);--a-radius:var(--monolith-auth-radius,10px);
--a-font:var(--monolith-auth-font,system-ui,-apple-system,"Segoe UI",Roboto,sans-serif);
font-family:var(--a-font);color:var(--a-fg);box-sizing:border-box}
@media (prefers-color-scheme:dark){.root:not([data-theme=light]){--d-bg:#16181d;--d-fg:#f3f4f6;--d-muted:#9ca3af;--d-border:#374151;--d-surface:#1f2937;--d-accent:#f3f4f6;--d-accent-fg:#111827;--d-danger:#fca5a5;--d-danger-bg:#3b1d1d}}
.root[data-theme=dark]{--d-bg:#16181d;--d-fg:#f3f4f6;--d-muted:#9ca3af;--d-border:#374151;--d-surface:#1f2937;--d-accent:#f3f4f6;--d-accent-fg:#111827;--d-danger:#fca5a5;--d-danger-bg:#3b1d1d}
.root *{box-sizing:border-box;font-family:inherit}
.layout-fullscreen{min-height:100vh;display:flex;align-items:center;justify-content:center;background:var(--a-bg);padding:16px}
.layout-fullscreen .card,.layout-card .card{width:100%;max-width:400px;padding:28px 24px;background:var(--a-bg);border:1px solid var(--a-border);border-radius:calc(var(--a-radius) + 4px);box-shadow:0 1px 3px rgba(0,0,0,.08)}
.layout-fullscreen .card{border:0;box-shadow:none}
.layout-card{display:flex;justify-content:center}
.layout-plain .card{width:100%;padding:0;background:transparent}
header{text-align:center;margin-bottom:20px}
.logo{display:block;margin:0 auto 12px;max-height:48px;max-width:160px}
h1{font-size:1.25rem;line-height:1.3;margin:0}
.subtitle{margin:6px 0 0;color:var(--a-muted);font-size:.9rem}
.providers{display:flex;flex-direction:column;gap:8px}
.pbtn{display:flex;align-items:center;justify-content:center;gap:10px;width:100%;min-height:44px;padding:0 14px;border-radius:var(--a-radius);border:1px solid var(--pborder,transparent);background:var(--pbg);color:var(--pfg);font-size:.95rem;font-weight:500;cursor:pointer}
.pbtn:hover:not(:disabled){filter:brightness(.96)}
.pbtn:focus-visible,.submit:focus-visible,.link:focus-visible,input:focus-visible{outline:2px solid var(--a-accent);outline-offset:2px}
.pbtn:disabled,.submit:disabled{opacity:.6;cursor:not-allowed}
.btn-outline .pbtn{background:transparent;color:var(--a-fg);border-color:var(--a-border)}
.icon{display:inline-flex;width:20px;height:20px}
.divider{display:flex;align-items:center;gap:12px;margin:16px 0;color:var(--a-muted);font-size:.85rem}
.divider::before,.divider::after{content:"";flex:1;height:1px;background:var(--a-border)}
form{display:flex;flex-direction:column;gap:12px}
.field{display:flex;flex-direction:column;gap:4px}
label{font-size:.85rem;font-weight:500}
input{width:100%;min-height:44px;padding:0 12px;border:1px solid var(--a-border);border-radius:var(--a-radius);background:var(--a-surface);color:var(--a-fg);font-size:1rem}
.submit{min-height:44px;border:1px solid var(--a-accent);border-radius:var(--a-radius);background:var(--a-accent);color:var(--a-accent-fg);font-size:.95rem;font-weight:600;cursor:pointer}
.btn-outline .submit{background:transparent;color:var(--a-accent)}
.link{background:none;border:0;padding:6px;margin:8px auto 0;display:block;color:var(--a-muted);font-size:.875rem;cursor:pointer;text-decoration:underline}
.error{display:flex;gap:8px;align-items:flex-start;justify-content:space-between;margin-bottom:14px;padding:10px 12px;border-radius:var(--a-radius);background:var(--a-danger-bg);color:var(--a-danger);font-size:.875rem}
.error small{display:block;margin-top:4px;opacity:.85}
.error button{background:none;border:0;color:inherit;font-size:1.1rem;line-height:1;cursor:pointer;padding:0 2px}
.status,.empty{text-align:center;color:var(--a-muted);font-size:.9rem;padding:12px 0}
.account{text-align:center}
footer{margin-top:20px;text-align:center;font-size:.8rem;color:var(--a-muted)}
footer a{color:inherit;margin:0 6px}
`;

let uid = 0;

function safeUrl(u: string | undefined): string | null {
  if (!u) return null;
  return /^(https?:\/\/|\/|\.\/|\.\.\/|#|mailto:)/i.test(u.trim()) ? u.trim() : null;
}

function h(doc: Document, tag: string, attrs: Record<string, Attr> = {}, ...children: Child[]): HTMLElement {
  const el = doc.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === undefined || v === null || v === false) continue;
    if (typeof v === "function") el.addEventListener(k.slice(2), v as EventListener);
    else if (k === "value") (el as HTMLInputElement).value = String(v);
    else el.setAttribute(k, v === true ? "" : String(v));
  }
  for (const c of children) {
    if (c === null || c === false || c === undefined) continue;
    el.appendChild(typeof c === "string" ? doc.createTextNode(c) : c);
  }
  return el;
}

/** Draws the auth screen into `el`'s shadow root. Slots: `logo`, `title`, `subtitle`, `footer`. */
export function mountAuth(el: HTMLElement, mono: { auth: AuthApi }, initial: AuthUiOptions = {}): AuthMount {
  const doc = el.ownerDocument ?? document;
  const root: ShadowRoot | HTMLElement = el.shadowRoot ?? el.attachShadow({ mode: "open" });
  let options: AuthUiOptions = initial;
  let labels: AuthLabels = resolveLabels(options.labels);
  // The controller reads these live, so `update` never has to rebuild it (and `onSignedIn` never fires twice).
  const live = new Proxy({} as AuthUiOptions, { get: (_t, k) => (options as Record<string | symbol, unknown>)[k] });
  const controller = createAuthController(mono, live);
  let unsub: () => void = () => {};
  const id = `monolith-auth-${++uid}`;
  const values = { name: "", email: "", password: "" };

  const style = doc.createElement("style");
  style.textContent = STYLE;
  const container = doc.createElement("div");
  root.replaceChildren(style, container);

  function render(): void {
    const state = controller.getState();
    const focusName = (root as ShadowRoot).activeElement?.getAttribute?.("name") ?? null;
    const refs = new Map<string, HTMLElement>();
    const t = options.theme ?? {};
    const vars: string[] = [];
    const set = (k: string, v?: string): void => void (v && vars.push(`--monolith-auth-${k}:${v}`));
    set("accent", t.accent);
    set("accent-fg", t.accentText);
    set("bg", t.background);
    set("fg", t.foreground);
    set("muted", t.muted);
    set("border", t.border);
    set("danger", t.danger);
    set("radius", t.radius);
    set("font", t.font);
    const layout = options.layout ?? "card";
    const classes = ["root", `layout-${layout}`, t.buttonStyle === "outline" ? "btn-outline" : ""].filter(Boolean).join(" ");

    const vname = (s: AuthUiState): string => s.methods?.app ?? "";
    const vars2 = { app: vname(state) };
    const rootEl = h(doc, "div", {
      class: classes,
      part: "root",
      style: vars.join(";"),
      "data-theme": t.mode && t.mode !== "auto" ? t.mode : undefined,
    });
    const card = h(doc, "div", { class: "card", part: "card" });
    rootEl.appendChild(card);

    // header
    const title =
      options.title ?? fill(state.mode === "signUp" ? labels.signUpTitle : labels.signInTitle, vars2).replace(/\s+to\s*$|\s{2,}/g, " ").trim();
    const logoUrl = typeof options.logo === "string" ? safeUrl(options.logo) : null;
    card.appendChild(
      h(
        doc,
        "header",
        { part: "header" },
        h(doc, "slot", { name: "logo" }, logoUrl ? h(doc, "img", { class: "logo", part: "logo", src: logoUrl, alt: "" }) : options.logo && typeof options.logo !== "string" ? options.logo : null),
        h(doc, "h1", { part: "title" }, h(doc, "slot", { name: "title" }, title)),
        h(doc, "slot", { name: "subtitle" }, options.subtitle ? h(doc, "p", { class: "subtitle", part: "subtitle" }, options.subtitle) : null),
      ),
    );

    if (state.error) {
      card.appendChild(
        h(
          doc,
          "div",
          { class: "error", part: "error", role: "alert", "data-code": state.error.code },
          h(doc, "div", {}, state.error.message, state.error.hint ? h(doc, "small", { part: "error-hint" }, state.error.hint) : null),
          h(doc, "button", { type: "button", part: "error-dismiss", "aria-label": labels.dismiss, onclick: () => controller.dismissError() }, "×"),
        ),
      );
    }

    if (state.user) {
      card.appendChild(accountPanel(state.user));
    } else if (state.loading) {
      card.appendChild(h(doc, "div", { class: "status", part: "loading", role: "status", "aria-live": "polite" }, labels.loading));
    } else {
      body(card, state, refs);
    }

    // footer
    const terms = safeUrl(options.termsUrl);
    const privacy = safeUrl(options.privacyUrl);
    const link = (href: string, text: string, part: string): HTMLElement =>
      h(doc, "a", { href, part, target: "_blank", rel: "noopener noreferrer" }, text);
    card.appendChild(
      h(
        doc,
        "footer",
        { part: "footer" },
        terms ? link(terms, labels.terms, "terms-link") : null,
        privacy ? link(privacy, labels.privacy, "privacy-link") : null,
        h(doc, "slot", { name: "footer" }),
      ),
    );

    container.replaceChildren(rootEl);
    if (focusName) {
      const f = refs.get(focusName) as HTMLInputElement | undefined;
      f?.focus?.();
      try {
        const n = f?.value?.length ?? 0;
        f?.setSelectionRange?.(n, n);
      } catch {
        // input type without selection
      }
    }
  }

  function accountPanel(user: AppUser): HTMLElement {
    return h(
      doc,
      "div",
      { class: "account", part: "account" },
      h(doc, "p", { part: "account-name" }, fill(labels.signedInAs, { name: user.name || user.email || user.id })),
      h(doc, "button", { type: "button", class: "submit", part: "signout", onclick: () => void controller.signOut() }, labels.signOut),
    );
  }

  function body(card: HTMLElement, state: AuthUiState, refs: Map<string, HTMLElement>): void {
    const view = selectAuthView(state, options);
    const busy = state.submitting !== false;

    if (view.empty) {
      card.appendChild(
        h(
          doc,
          "div",
          { class: "empty", part: "empty" },
          labels.noMethods,
          state.methods ? null : h(doc, "button", { type: "button", class: "link", part: "retry", onclick: () => void controller.refreshMethods() }, labels.retry),
        ),
      );
      return;
    }

    if (view.providers.length) {
      const list = h(doc, "div", { class: "providers", part: "providers" });
      for (const p of view.providers) {
        const brand = brandFor(p.id, p.name);
        const mine = state.submitting === p.id;
        const label = mine ? labels.redirecting : fill(labels.continueWith, { name: brand.name });
        const ctx: ProviderButtonContext = {
          provider: p,
          brand,
          label,
          busy: mine,
          disabled: busy,
          signIn: () => void controller.signInWithProvider(p.id),
        };
        const custom = options.providerButton?.(ctx);
        if (custom) {
          list.appendChild(h(doc, "div", { part: `provider-slot provider-slot-${p.id}` }, custom));
          continue;
        }
        const icon = h(doc, "span", { class: "icon", part: "provider-icon" });
        icon.innerHTML = brand.icon;
        list.appendChild(
          h(
            doc,
            "button",
            {
              type: "button",
              class: "pbtn",
              part: `provider-button provider-button-${p.id}`,
              "data-provider": p.id,
              "aria-label": label,
              "aria-busy": mine,
              disabled: busy,
              style: `--pbg:${brand.bg};--pfg:${brand.fg};--pborder:${brand.border ?? "transparent"}`,
              onclick: ctx.signIn,
            },
            icon,
            h(doc, "span", { part: "provider-label" }, label),
          ),
        );
      }
      card.appendChild(list);
    }

    if (view.providers.length && view.showEmail) {
      card.appendChild(h(doc, "div", { class: "divider", part: "divider", role: "separator" }, labels.or));
    }

    if (view.showEmail) {
      const signUp = state.mode === "signUp";
      const field = (name: "name" | "email" | "password", label: string, type: string, autocomplete: string, required: boolean): HTMLElement => {
        const input = h(doc, "input", {
          id: `${id}-${name}`,
          name,
          type,
          autocomplete,
          required,
          "aria-required": required,
          "aria-invalid": state.error?.code === (name === "email" ? "invalid_email" : "weak_password") ? true : undefined,
          part: `input input-${name}`,
          value: values[name],
          disabled: busy,
          oninput: (ev: Event) => void (values[name] = (ev.target as HTMLInputElement).value),
        });
        refs.set(name, input);
        return h(doc, "div", { class: "field", part: "field" }, h(doc, "label", { for: `${id}-${name}`, part: "label" }, label), input);
      };
      const form = h(
        doc,
        "form",
        {
          part: "form",
          novalidate: true,
          onsubmit: (ev: Event) => {
            ev.preventDefault();
            void (signUp
              ? controller.signUpWithEmail(values.email, values.password, values.name)
              : controller.signInWithEmail(values.email, values.password));
          },
        },
        signUp ? field("name", labels.name, "text", "name", false) : null,
        field("email", labels.email, "email", "email", true),
        field("password", labels.password, "password", signUp ? "new-password" : "current-password", true),
        h(
          doc,
          "button",
          { type: "submit", class: "submit", part: "submit", disabled: busy, "aria-busy": state.submitting === true },
          state.submitting === true ? labels.signingIn : signUp ? labels.signUp : labels.signIn,
        ),
      );
      card.appendChild(form);
      if (view.canSignUp) {
        card.appendChild(
          h(
            doc,
            "button",
            {
              type: "button",
              class: "link",
              part: "switch",
              onclick: () => {
                values.password = "";
                controller.setMode(signUp ? "signIn" : "signUp");
              },
            },
            signUp ? labels.switchToSignIn : labels.switchToSignUp,
          ),
        );
      }
    }
  }

  function bind(): void {
    unsub();
    unsub = controller.subscribe(render);
    render();
  }
  bind();

  return {
    controller,
    update(patch) {
      options = { ...options, ...patch };
      labels = resolveLabels(options.labels);
      render();
    },
    replace(next) {
      options = next;
      labels = resolveLabels(options.labels);
      render();
    },
    destroy() {
      unsub();
      controller.destroy();
      container.replaceChildren();
    },
  };
}
