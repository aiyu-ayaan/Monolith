import { createResource, createSignal, For, Show } from "solid-js";
import { useNavigate, useParams } from "@solidjs/router";
import { api, enc, me, ready, type ApiError, type AppInfo, type Superuser } from "../api";
import { Drawer, fmtDate, Icon, PasswordInput, toast } from "../ui";
import ErrorBox from "./ErrorBox";
import ApiKeys from "./ApiKeys";

type TabId = "general" | "keys" | "users";
const TABS: { id: TabId; label: string; project: boolean }[] = [
  { id: "general", label: "General", project: true },
  { id: "keys", label: "API keys", project: true },
  { id: "users", label: "Users and permissions", project: false },
];

/**
 * Settings, Firebase-style tabs. Under a project: General, API keys, Users and permissions.
 * Outside a project (`#/settings`): Users and permissions only, since console users span all projects.
 */
export default function Settings() {
  const params = useParams();
  const tabs = () => TABS.filter((t) => params.app || !t.project);
  const tab = () => tabs().find((t) => t.id === params.tab)?.id ?? tabs()[0].id;
  const href = (t: TabId) => (params.app ? `#/apps/${enc(params.app)}/settings/${t}` : `#/settings/${t}`);

  return (
    <div class="page">
      <header class="page-head">
        <div class="min0">
          <Show when={params.app}>
            <p class="crumbs">
              <a href={`#/apps/${enc(params.app!)}`}>{params.app}</a>
            </p>
          </Show>
          <h1>{params.app ? "Project settings" : "Console settings"}</h1>
        </div>
      </header>
      <nav class="tabs settings-tabs" aria-label="Settings sections">
        <For each={tabs()}>
          {(t) => (
            <a class="tab" classList={{ active: tab() === t.id }} href={href(t.id)} aria-current={tab() === t.id ? "page" : undefined}>
              {t.label}
            </a>
          )}
        </For>
      </nav>
      <Show when={tab() === "general"}>
        <General app={params.app!} />
      </Show>
      <Show when={tab() === "keys"}>
        <ApiKeys embedded />
      </Show>
      <Show when={tab() === "users"}>
        <Users />
      </Show>
    </div>
  );
}

function Row(p: { label: string; children: any }) {
  return (
    <div class="kv-row">
      <span class="muted small">{p.label}</span>
      <div class="min0">{p.children}</div>
    </div>
  );
}

function Copyable(p: { value: string }) {
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(p.value);
      toast("Copied");
    } catch {
      toast("Select the text and copy it manually");
    }
  };
  return (
    <span class="copyable">
      <code class="mono truncate">{p.value}</code>
      <button class="icon-btn" aria-label={`Copy ${p.value}`} title="Copy" onClick={copy}>
        <Icon name="copy" size={14} />
      </button>
    </span>
  );
}

function General(p: { app: string }) {
  const navigate = useNavigate();
  const [info] = createResource(() => p.app, (a) => api<AppInfo>("GET", `/api/v1/system/apps/${enc(a)}`));
  const [error, setError] = createSignal<ApiError | null>(null);
  const origin = location.origin;
  const rest = () => `${origin}/api/v1/apps/${enc(p.app)}`;

  const remove = async () => {
    if (prompt(`Delete project "${p.app}" and all its data? This cannot be undone.\nType the project name to confirm.`) !== p.app) return;
    try {
      await api("DELETE", `/api/v1/system/apps/${enc(p.app)}`);
      toast(`Project "${p.app}" deleted`);
      navigate("/");
    } catch (e) {
      setError(e as ApiError);
    }
  };

  return (
    <div class="stack">
      <ErrorBox error={error() ?? (info.error as ApiError | undefined)} />
      <section class="card settings-card">
        <h2 class="card-title">Your project</h2>
        <Row label="Project ID">
          <Copyable value={p.app} />
        </Row>
        <Row label="Created">{ready(info) ? fmtDate(ready(info)!.created) : "…"}</Row>
        <Row label="Data directory">
          <code class="mono small">/data/apps/{p.app}/</code>
        </Row>
      </section>
      <section class="card settings-card">
        <h2 class="card-title">Endpoints</h2>
        <p class="muted small">Use these from any client. REST and realtime work without an SDK.</p>
        <Row label="REST base URL">
          <Copyable value={rest()} />
        </Row>
        <Row label="Realtime (SSE)">
          <Copyable value={`${rest()}/realtime`} />
        </Row>
      </section>
      <section class="card settings-card danger-zone">
        <h2 class="card-title">Danger zone</h2>
        <div class="kv-row">
          <div class="min0">
            <strong>Delete this project</strong>
            <p class="muted small">Removes its database, files and backups. This cannot be undone.</p>
          </div>
          <button class="btn secondary danger" onClick={remove}>
            <Icon name="trash" />
            Delete project
          </button>
        </div>
      </section>
    </div>
  );
}

function Users() {
  const [list, { refetch }] = createResource(() => api<{ items: Superuser[] }>("GET", "/api/v1/system/superusers"));
  const [adding, setAdding] = createSignal(false);
  const [email, setEmail] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<ApiError | null>(null);

  const open = () => {
    setEmail("");
    setPassword("");
    setError(null);
    setAdding(true);
  };

  const add = async (e: SubmitEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api("POST", "/api/v1/system/superusers", { email: email(), password: password() });
      setAdding(false);
      toast(`Added ${email().trim().toLowerCase()}`);
      refetch();
    } catch (err) {
      setError(err as ApiError);
    } finally {
      setBusy(false);
    }
  };

  const remove = async (u: Superuser) => {
    if (!confirm(`Remove ${u.email}? They lose console access. A session they already have ends within 1 hour.`)) return;
    setError(null);
    try {
      await api("DELETE", `/api/v1/system/superusers/${enc(u.id)}`);
      toast(`Removed ${u.email}`);
      refetch();
    } catch (err) {
      setError(err as ApiError);
    }
  };

  return (
    <div class="stack">
      <ErrorBox error={adding() ? null : (error() ?? (list.error as ApiError | undefined))} />
      <section class="card settings-card">
        <div class="card-head">
          <div class="min0">
            <h2 class="card-title">Users</h2>
            <p class="muted small">People who can sign in to this console. Users span every project on this server.</p>
          </div>
          <button class="btn" onClick={open}>
            <Icon name="plus" />
            Add user
          </button>
        </div>
        <div class="table-wrap">
          <table class="grid">
            <thead>
              <tr>
                <th scope="col">User</th>
                <th scope="col">Role</th>
                <th scope="col">Added</th>
                <th scope="col"><span class="sr-only">Actions</span></th>
              </tr>
            </thead>
            <tbody>
              <For each={ready(list)?.items ?? []} fallback={<tr><td colspan="4" class="empty-row">{list.loading ? "Loading…" : "No users."}</td></tr>}>
                {(u) => {
                  const self = () => me()?.id === u.id;
                  return (
                    <tr>
                      <td>
                        <span class="user-cell">
                          <span class="avatar" aria-hidden="true">{u.email[0].toUpperCase()}</span>
                          <span class="truncate">{u.email}</span>
                          <Show when={self()}>
                            <span class="badge">You</span>
                          </Show>
                        </span>
                      </td>
                      <td>Superuser</td>
                      <td class="small">{fmtDate(u.created)}</td>
                      <td class="num">
                        <button
                          class="icon-btn danger"
                          aria-label={`Remove ${u.email}`}
                          title={self() ? "You cannot remove yourself" : "Remove"}
                          disabled={self() || (ready(list)?.items.length ?? 0) < 2}
                          onClick={() => remove(u)}
                        >
                          <Icon name="trash" />
                        </button>
                      </td>
                    </tr>
                  );
                }}
              </For>
            </tbody>
          </table>
        </div>
      </section>

      <section class="card settings-card">
        <h2 class="card-title">Roles and permissions</h2>
        <p class="muted small">What each role can do. Only Superuser exists today; per-project roles are planned (design doc 08).</p>
        <div class="table-wrap">
          <table class="grid">
            <thead>
              <tr>
                <th scope="col">Role</th>
                <th scope="col">Projects</th>
                <th scope="col">Data and schema</th>
                <th scope="col">Users, keys, backups</th>
                <th scope="col">Status</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td><strong>Superuser</strong></td>
                <td>All</td>
                <td>Read and write, bypasses rules</td>
                <td>Manage</td>
                <td><span class="badge on">Active</span></td>
              </tr>
              <tr class="muted">
                <td>Project admin</td>
                <td>Assigned only</td>
                <td>Read and write</td>
                <td>Manage within the project</td>
                <td><span class="badge">Planned</span></td>
              </tr>
              <tr class="muted">
                <td>Viewer</td>
                <td>Assigned only</td>
                <td>Read only</td>
                <td>None</td>
                <td><span class="badge">Planned</span></td>
              </tr>
            </tbody>
          </table>
        </div>
        <p class="muted small">For servers and scripts, use project API keys with scopes instead of a user account.</p>
      </section>

      <Show when={adding()}>
        <Drawer
          title="Add user"
          onClose={() => setAdding(false)}
          onSubmit={add}
          footer={
            <>
              <button type="button" class="btn secondary" onClick={() => setAdding(false)}>
                Cancel
              </button>
              <button class="btn" type="submit" disabled={busy()}>
                Add user
              </button>
            </>
          }
        >
          <ErrorBox error={error()} />
          <p class="notice">
            <Icon name="alert" size={16} />
            <span>New users get the Superuser role: full access to every project. Share the password with them over a secure channel.</span>
          </p>
          <div class="field">
            <label for="uemail">Email</label>
            <input id="uemail" type="email" required autocomplete="off" value={email()} onInput={(e) => setEmail(e.currentTarget.value)} />
          </div>
          <div class="field">
            <label for="upass">Initial password</label>
            <PasswordInput id="upass" value={password()} onInput={setPassword} autocomplete="new-password" minLength={10} describedBy="upass-help" />
            <span class="help" id="upass-help">At least 10 characters.</span>
          </div>
        </Drawer>
      </Show>
    </div>
  );
}
