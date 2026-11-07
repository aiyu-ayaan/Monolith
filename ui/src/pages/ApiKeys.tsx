import { createResource, createSignal, For, Show } from "solid-js";
import { useParams } from "@solidjs/router";
import { api, enc, ready, type ApiError, type ApiKey } from "../api";
import { Drawer, fmtDate, Icon, toast } from "../ui";
import ErrorBox from "./ErrorBox";

const PRESETS: { id: string; label: string; help: string; scopes: string }[] = [
  { id: "service", label: "Server (full records access)", help: "Bypasses collection rules for this app. Keep it on your server.", scopes: "records:admin" },
  { id: "read", label: "Read-only", help: "Reads records; collection rules still apply as for an anonymous caller.", scopes: "records:read" },
  { id: "live", label: "Realtime listener", help: "Subscribes to changes of the listed collections.", scopes: "realtime:subscribe,records:read" },
  { id: "custom", label: "Custom scopes", help: "Comma separated, e.g. records:read:tasks, records:write:tasks, files:read:photos.", scopes: "" },
];

/** API keys of one app: create (secret shown once), list, revoke. */
export default function ApiKeys() {
  const params = useParams();
  const base = () => `/api/v1/apps/${enc(params.app!)}/api-keys`;
  const [list, { refetch }] = createResource(base, (b) => api<{ items: ApiKey[] }>("GET", b));
  const [error, setError] = createSignal<ApiError | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [creating, setCreating] = createSignal(false);
  const [secret, setSecret] = createSignal<string | null>(null);
  const [name, setName] = createSignal("");
  const [preset, setPreset] = createSignal("service");
  const [custom, setCustom] = createSignal("");
  const [days, setDays] = createSignal("90");

  const scopes = () =>
    (preset() === "custom" ? custom() : PRESETS.find((p) => p.id === preset())!.scopes)
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);

  const open = () => {
    setName("");
    setPreset("service");
    setCustom("");
    setDays("90");
    setError(null);
    setCreating(true);
  };

  const create = async (e: SubmitEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const k = await api<ApiKey & { key: string }>("POST", base(), {
        name: name(),
        scopes: scopes(),
        ...(days() ? { expiresInDays: Number(days()) } : {}),
      });
      setCreating(false);
      setSecret(k.key);
      refetch();
    } catch (err) {
      setError(err as ApiError);
    } finally {
      setBusy(false);
    }
  };

  const revoke = async (k: ApiKey) => {
    if (!confirm(`Revoke "${k.name}" (${k.prefix}…)? Clients using it stop working immediately.`)) return;
    setError(null);
    try {
      await api("DELETE", `${base()}/${enc(k.id)}`);
      toast("Key revoked");
      refetch();
    } catch (err) {
      setError(err as ApiError);
    }
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(secret()!);
      toast("Copied");
    } catch {
      toast("Select the key and copy it manually");
    }
  };

  return (
    <div class="page">
      <header class="page-head">
        <div class="min0">
          <p class="crumbs">
            <a href={`#/apps/${enc(params.app!)}`}>{params.app}</a>
          </p>
          <h1>API keys</h1>
          <p class="muted">Credentials for servers and scripts. Send as <code>Authorization: Bearer mk_…</code>. A key works only on this app.</p>
        </div>
        <button class="btn" onClick={open}>
          <Icon name="key" />
          Create key
        </button>
      </header>
      <ErrorBox error={creating() ? null : (error() ?? (list.error as ApiError | undefined))} />
      <p class="notice">
        <Icon name="alert" size={16} />
        <span>
          Only <code>records:admin</code> bypasses collection rules. Other keys count as anonymous for the rules and are narrowed by their scopes (<code>@request.key</code> is planned). Keys are stored as hashes, so a lost key cannot be shown again.
        </span>
      </p>
      <Show when={secret()}>
        <div class="notice" role="status">
          <Icon name="check" size={16} />
          <span class="min0">
            Copy this key now; it is shown once.
            <br />
            <code class="mono" style={{ "word-break": "break-all" }}>{secret()}</code>
          </span>
          <button class="btn secondary sm" onClick={copy}>
            <Icon name="copy" size={14} />
            Copy
          </button>
          <button class="btn secondary sm" onClick={() => setSecret(null)}>
            Done
          </button>
        </div>
      </Show>
      <div class="table-wrap">
        <table class="grid">
          <thead>
            <tr>
              <th scope="col">Name</th>
              <th scope="col">Key</th>
              <th scope="col">Scopes</th>
              <th scope="col">Created</th>
              <th scope="col">Expires</th>
              <th scope="col"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            <For each={ready(list)?.items ?? []} fallback={<tr><td colspan="6" class="empty-row">{list.loading ? "Loading…" : "No API keys yet."}</td></tr>}>
              {(k) => (
                <tr>
                  <td>{k.name}</td>
                  <td class="mono small">{k.prefix}…</td>
                  <td class="mono small">{k.scopes.join(", ")}</td>
                  <td class="small">{fmtDate(k.created)}</td>
                  <td class="small">{k.expires ? fmtDate(k.expires) : "Never"}</td>
                  <td class="num">
                    <button class="icon-btn danger" aria-label={`Revoke ${k.name}`} title="Revoke" onClick={() => revoke(k)}>
                      <Icon name="trash" />
                    </button>
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>

      <Show when={creating()}>
        <Drawer
          title="Create API key"
          onClose={() => setCreating(false)}
          onSubmit={create}
          footer={
            <>
              <button type="button" class="btn secondary" onClick={() => setCreating(false)}>
                Cancel
              </button>
              <button class="btn" type="submit" disabled={busy()}>
                Create
              </button>
            </>
          }
        >
          <ErrorBox error={error()} />
          <div class="field">
            <label for="kname">Name</label>
            <input id="kname" required maxlength="100" value={name()} onInput={(e) => setName(e.currentTarget.value)} />
          </div>
          <div class="field">
            <label for="kpreset">Access</label>
            <select id="kpreset" value={preset()} onChange={(e) => setPreset(e.currentTarget.value)}>
              <For each={PRESETS}>{(p) => <option value={p.id}>{p.label}</option>}</For>
            </select>
            <span class="help">{PRESETS.find((p) => p.id === preset())!.help}</span>
          </div>
          <Show when={preset() === "custom"}>
            <div class="field">
              <label for="kscopes">Scopes</label>
              <input id="kscopes" class="mono" required value={custom()} onInput={(e) => setCustom(e.currentTarget.value)} />
            </div>
          </Show>
          <div class="field">
            <label for="kdays">Expires after (days)</label>
            <input id="kdays" type="number" min="1" max="3650" value={days()} onInput={(e) => setDays(e.currentTarget.value)} />
            <span class="help">Leave empty for a key that never expires.</span>
          </div>
        </Drawer>
      </Show>
    </div>
  );
}
