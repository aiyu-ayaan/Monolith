import { createResource, createSignal, For, Show } from "solid-js";
import { useParams } from "@solidjs/router";
import { api, enc, ready, type ApiError, type AuthConfig, type AuthProvider, type AuthProviderDef, type AuthProviderInput } from "../api";
import { Drawer, Icon, toast } from "../ui";
import ErrorBox from "./ErrorBox";

type Editing = { kind: "email" } | { kind: "provider"; def: AuthProviderDef; existing?: AuthProvider };

const EXTRA_LABELS: Record<string, string> = {
  teamId: "Team ID",
  keyId: "Key ID",
  tenant: "Tenant (ID, or common)",
  baseUrl: "Base URL (self-hosted, optional)",
  issuer: "Issuer URL",
};

/** Firebase-style Authentication for one app: email/password plus any number of providers. */
export default function Auth() {
  const params = useParams();
  const url = () => `/api/v1/apps/${enc(params.app!)}/auth/config`;
  const [cfg, { mutate }] = createResource(url, (u) => api<AuthConfig>("GET", u));
  const [editing, setEditing] = createSignal<Editing | null>(null);
  const [picking, setPicking] = createSignal(false);
  const c = () => ready(cfg);

  const toInput = (p: AuthProvider): AuthProviderInput => ({ id: p.id, enabled: p.enabled, clientId: p.clientId, extra: p.extra });
  /** Sends the whole config; secrets of untouched providers are kept by the server. */
  const save = async (emailPassword: AuthConfig["emailPassword"], providers: AuthProviderInput[]) => {
    mutate(await api<AuthConfig>("PUT", url(), { emailPassword, providers }));
  };
  const others = (id?: string) => (c()?.providers ?? []).filter((p) => p.id !== id).map(toInput);
  const available = () => (c()?.catalog ?? []).filter((d) => !c()!.providers.some((p) => p.id === d.id));

  const toggle = async (p: AuthProvider) => {
    try {
      await save(c()!.emailPassword, (c()!.providers).map((x) => (x.id === p.id ? { ...toInput(x), enabled: !x.enabled } : toInput(x))));
      toast(`${p.name} ${p.enabled ? "disabled" : "enabled"}`);
    } catch (e) {
      toast((e as ApiError).problem?.errors?.[0]?.message ?? (e as Error).message);
    }
  };

  return (
    <div class="page">
      <header class="page-head">
        <div class="min0">
          <p class="crumbs">
            <a href={`#/apps/${enc(params.app!)}`}>{params.app}</a>
          </p>
          <h1>Authentication</h1>
          <p class="muted">Choose how people sign in to this app. Connect as many providers as you need.</p>
        </div>
      </header>
      <ErrorBox error={cfg.error as ApiError | undefined} />
      <p class="notice">
        <Icon name="alert" size={16} />
        <span>
          This saves your sign-in configuration. The sign-in flow itself (tokens, OAuth redirects) ships with Phase 2 auth, so end users cannot sign in yet.
        </span>
      </p>

      <Show when={c()}>
        {(c) => (
          <section class="card">
            <div class="card-head">
              <div>
                <h2 class="card-title">Sign-in providers</h2>
                <p class="muted small">{c().providers.length} connected · email/password is built in.</p>
              </div>
              <button class="btn" onClick={() => setPicking(!picking())} aria-expanded={picking()}>
                <Icon name="plus" />
                Add new provider
              </button>
            </div>

            <Show when={picking()}>
              <div class="picker" role="group" aria-label="Available providers">
                <For each={available()} fallback={<p class="muted small">Every available provider is connected.</p>}>
                  {(d) => (
                    <button
                      class="picker-item"
                      onClick={() => {
                        setPicking(false);
                        setEditing({ kind: "provider", def: d });
                      }}
                    >
                      <span class="avatar">{d.name[0]}</span>
                      {d.name}
                    </button>
                  )}
                </For>
              </div>
            </Show>

            <div class="table-wrap flat">
              <table class="grid">
                <thead>
                  <tr>
                    <th scope="col">Provider</th>
                    <th scope="col">Status</th>
                    <th scope="col">Client ID</th>
                    <th scope="col" class="num">
                      <span class="sr-only">Actions</span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  <tr>
                    <td>
                      <span class="prov">
                        <span class="avatar">@</span>
                        <strong>Email / password</strong>
                      </span>
                    </td>
                    <td>
                      <span class="badge" classList={{ on: c().emailPassword.enabled }}>{c().emailPassword.enabled ? "Enabled" : "Disabled"}</span>
                    </td>
                    <td class="muted small">
                      min {c().emailPassword.minPasswordLength} characters · sign-up {c().emailPassword.allowSignup ? "open" : "closed"}
                    </td>
                    <td class="num">
                      <button class="btn secondary sm" onClick={() => setEditing({ kind: "email" })}>
                        Edit
                      </button>
                    </td>
                  </tr>
                  <For each={c().providers}>
                    {(p) => (
                      <tr>
                        <td>
                          <span class="prov">
                            <span class="avatar">{p.name[0]}</span>
                            <strong>{p.name}</strong>
                          </span>
                        </td>
                        <td>
                          <button class="badge btn-badge" classList={{ on: p.enabled }} aria-pressed={p.enabled} title="Click to toggle" onClick={() => toggle(p)}>
                            {p.enabled ? "Enabled" : "Disabled"}
                          </button>
                        </td>
                        <td class="mono small truncate-cell">{p.clientId || "–"}</td>
                        <td class="num">
                          <button class="btn secondary sm" onClick={() => setEditing({ kind: "provider", def: c().catalog.find((d) => d.id === p.id)!, existing: p })}>
                            Edit
                          </button>
                        </td>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
            </div>
          </section>
        )}
      </Show>

      <Show when={editing()}>
        {(e) => (
          <Show
            when={e().kind === "email"}
            fallback={
              <ProviderDrawer
                e={e() as Extract<Editing, { kind: "provider" }>}
                onClose={() => setEditing(null)}
                onSave={async (input) => {
                  await save(c()!.emailPassword, [...others(input.id), input]);
                  toast("Provider saved");
                  setEditing(null);
                }}
                onRemove={async (id) => {
                  await save(c()!.emailPassword, others(id));
                  toast("Provider removed");
                  setEditing(null);
                }}
              />
            }
          >
            <EmailDrawer
              initial={c()!.emailPassword}
              onClose={() => setEditing(null)}
              onSave={async (ep) => {
                await save(ep, c()!.providers.map(toInput));
                toast("Email/password saved");
                setEditing(null);
              }}
            />
          </Show>
        )}
      </Show>
    </div>
  );
}

function EmailDrawer(p: { initial: AuthConfig["emailPassword"]; onClose: () => void; onSave: (ep: AuthConfig["emailPassword"]) => Promise<void> }) {
  const [ep, setEp] = createSignal({ ...p.initial });
  const [error, setError] = createSignal<ApiError | null>(null);
  const [busy, setBusy] = createSignal(false);
  const submit = async (e: SubmitEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await p.onSave(ep());
    } catch (err) {
      setError(err as ApiError);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Drawer
      title="Email / password"
      onClose={p.onClose}
      onSubmit={submit}
      footer={
        <>
          <button type="button" class="btn secondary" onClick={p.onClose}>
            Cancel
          </button>
          <button class="btn" type="submit" disabled={busy()}>
            Save
          </button>
        </>
      }
    >
      <ErrorBox error={error()} />
      <label class="check">
        <input type="checkbox" checked={ep().enabled} onChange={(e) => setEp({ ...ep(), enabled: e.currentTarget.checked })} />
        Enable email/password sign-in
      </label>
      <label class="check">
        <input type="checkbox" checked={ep().allowSignup} onChange={(e) => setEp({ ...ep(), allowSignup: e.currentTarget.checked })} />
        Allow new users to sign up
      </label>
      <div class="field">
        <label for="minpw">Minimum password length</label>
        <input id="minpw" type="number" min="8" max="72" value={ep().minPasswordLength} onInput={(e) => setEp({ ...ep(), minPasswordLength: Number(e.currentTarget.value) })} />
        <span class="help">8 to 72 characters. Passwords are stored as Argon2id hashes.</span>
      </div>
    </Drawer>
  );
}

function ProviderDrawer(p: {
  e: Extract<Editing, { kind: "provider" }>;
  onClose: () => void;
  onSave: (i: AuthProviderInput) => Promise<void>;
  onRemove: (id: string) => Promise<void>;
}) {
  const { def, existing } = p.e;
  const params = useParams();
  const [enabled, setEnabled] = createSignal(existing?.enabled ?? true);
  const [clientId, setClientId] = createSignal(existing?.clientId ?? "");
  const [secret, setSecret] = createSignal("");
  const [clear, setClear] = createSignal(false);
  const [extra, setExtra] = createSignal<Record<string, string>>({ ...(existing?.extra ?? {}) });
  const [error, setError] = createSignal<ApiError | null>(null);
  const [busy, setBusy] = createSignal(false);
  const callback = () => `${location.origin}${existing?.callbackPath ?? `/api/v1/apps/${enc(params.app!)}/auth/oauth/${def.id}/callback`}`;

  const run = async (f: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await f();
    } catch (err) {
      setError(err as ApiError);
    } finally {
      setBusy(false);
    }
  };
  const submit = (e: SubmitEvent) => {
    e.preventDefault();
    const input: AuthProviderInput = { id: def.id, enabled: enabled(), clientId: clientId(), extra: extra() };
    if (clear()) input.clientSecret = "";
    else if (secret()) input.clientSecret = secret();
    run(() => p.onSave(input));
  };
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(callback());
      toast("Redirect URL copied");
    } catch {
      toast("Copy failed: select the URL manually");
    }
  };

  return (
    <Drawer
      title={`${existing ? "Edit" : "Add"} ${def.name}`}
      onClose={p.onClose}
      onSubmit={submit}
      footer={
        <>
          <Show when={existing}>
            <button type="button" class="btn danger" disabled={busy()} onClick={() => confirm(`Remove ${def.name}?`) && run(() => p.onRemove(def.id))}>
              Remove
            </button>
          </Show>
          <span class="grow" />
          <button type="button" class="btn secondary" onClick={p.onClose}>
            Cancel
          </button>
          <button class="btn" type="submit" disabled={busy()}>
            Save
          </button>
        </>
      }
    >
      <ErrorBox error={error()} />
      <label class="check">
        <input type="checkbox" checked={enabled()} onChange={(e) => setEnabled(e.currentTarget.checked)} />
        Enable {def.name} sign-in
      </label>
      <div class="field">
        <label for="cid">Client ID</label>
        <input id="cid" class="mono" autocomplete="off" value={clientId()} onInput={(e) => setClientId(e.currentTarget.value)} />
      </div>
      <div class="field">
        <label for="csec">Client secret</label>
        <input
          id="csec"
          class="mono"
          type="password"
          autocomplete="new-password"
          disabled={clear()}
          placeholder={existing?.clientSecretSet ? "•••••••• (unchanged)" : ""}
          value={secret()}
          onInput={(e) => setSecret(e.currentTarget.value)}
        />
        <span class="help">Write-only: the server never shows it again.</span>
        <Show when={existing?.clientSecretSet}>
          <label class="check">
            <input type="checkbox" checked={clear()} onChange={(e) => setClear(e.currentTarget.checked)} />
            Clear the stored secret
          </label>
        </Show>
      </div>
      <For each={def.fields}>
        {(f) => (
          <div class="field">
            <label for={`x-${f}`}>
              {EXTRA_LABELS[f] ?? f}
              <Show when={def.required.includes(f)}>
                <span class="req" aria-hidden="true">*</span>
              </Show>
            </label>
            <input id={`x-${f}`} class="mono" value={extra()[f] ?? ""} onInput={(e) => setExtra({ ...extra(), [f]: e.currentTarget.value })} />
          </div>
        )}
      </For>
      <div class="field">
        <label for="cb">Authorized redirect URI</label>
        <div class="affix">
          <input id="cb" class="mono" readonly value={callback()} onFocus={(e) => e.currentTarget.select()} />
          <button type="button" class="affix-btn" aria-label="Copy redirect URI" onClick={copy}>
            <Icon name="copy" size={16} />
          </button>
        </div>
        <span class="help">Register this exact URL in the {def.name} developer console (use your public origin in production).</span>
      </div>
    </Drawer>
  );
}
