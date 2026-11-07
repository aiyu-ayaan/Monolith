import type { AuthApi } from "./controller.js";
import { type AuthMount, type AuthUiOptions, mountAuth } from "./view.js";

const REGISTRY = Symbol.for("monolith.client.registry");

type Client = { auth: AuthApi };

/** Finds a client made by `Monolith.init` (shared across bundle copies) by `url` and/or `app`. */
function lookup(url: string | null, app: string | null): Client | null {
  const reg = (globalThis as unknown as Record<symbol, Map<string, Client> | undefined>)[REGISTRY];
  if (!reg || reg.size === 0) return null;
  if (url && app) return reg.get(`${url.replace(/\/+$/, "")}|${app}`) ?? null;
  const all = [...reg.entries()].filter(([k]) => (app ? k.endsWith(`|${app}`) : true));
  return all.length === 1 ? all[0]![1] : null;
}

const ATTRS = ["url", "app", "email", "sign-up", "providers", "hide-providers", "title", "subtitle", "logo", "terms-url", "privacy-url", "layout"];

/** Registers `<monolith-auth>` (no-op outside a browser or when already defined). Returns the element class. */
export function defineMonolithAuth(tag = "monolith-auth"): CustomElementConstructor | null {
  if (typeof HTMLElement === "undefined" || typeof customElements === "undefined") return null;
  const existing = customElements.get(tag);
  if (existing) return existing;

  class MonolithAuthElement extends HTMLElement {
    static observedAttributes = ATTRS;
    private _mono: Client | null = null;
    private _options: AuthUiOptions = {};
    private mounted: { client: Client; mount: AuthMount } | null = null;

    get mono(): Client | null {
      return this._mono;
    }
    set mono(v: Client | null) {
      this._mono = v;
      this.refresh();
    }
    get options(): AuthUiOptions {
      return this._options;
    }
    set options(v: AuthUiOptions) {
      this._options = v ?? {};
      this.refresh();
    }

    connectedCallback(): void {
      this.refresh();
    }
    disconnectedCallback(): void {
      this.mounted?.mount.destroy();
      this.mounted = null;
    }
    attributeChangedCallback(): void {
      this.refresh();
    }

    private fromAttributes(): AuthUiOptions {
      const o: AuthUiOptions = {};
      const g = (n: string): string | null => this.getAttribute(n);
      const list = (v: string | null): string[] | undefined => (v ? v.split(",").map((s) => s.trim()).filter(Boolean) : undefined);
      const email = g("email");
      if (email === "auto" || email === "hide" || email === "force") o.email = email;
      const signUp = g("sign-up");
      if (signUp === "auto" || signUp === "hide" || signUp === "only") o.signUp = signUp;
      const providers = g("providers");
      if (providers && providers !== "auto") o.providers = list(providers);
      const hidden = list(g("hide-providers"));
      if (hidden) o.hideProviders = hidden;
      const title = g("title");
      if (title) o.title = title;
      const subtitle = g("subtitle");
      if (subtitle) o.subtitle = subtitle;
      const logo = g("logo");
      if (logo) o.logo = logo;
      const terms = g("terms-url");
      if (terms) o.termsUrl = terms;
      const privacy = g("privacy-url");
      if (privacy) o.privacyUrl = privacy;
      const layout = g("layout");
      if (layout === "card" || layout === "fullscreen" || layout === "plain") o.layout = layout;
      return o;
    }

    private refresh(): void {
      if (!this.isConnected) return;
      const mono = this._mono ?? lookup(this.getAttribute("url"), this.getAttribute("app"));
      if (!mono) return;
      const options = { ...this.fromAttributes(), ...this._options };
      if (this.mounted && this.mounted.client === mono) {
        this.mounted.mount.replace(options);
        return;
      }
      this.mounted?.mount.destroy();
      this.mounted = { client: mono, mount: mountAuth(this, mono, options) };
    }
  }

  customElements.define(tag, MonolithAuthElement);
  return MonolithAuthElement;
}
