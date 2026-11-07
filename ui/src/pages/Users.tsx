import { createResource, createSignal, For, Show } from "solid-js";
import { useParams } from "@solidjs/router";
import { api, enc, ready, type ApiError, type AppUser } from "../api";
import { Drawer, fmtDate, Icon, toast } from "../ui";
import ErrorBox from "./ErrorBox";

/** End users of one app: list, add, disable, delete. */
export default function Users() {
  const params = useParams();
  const base = () => `/api/v1/apps/${enc(params.app!)}/users`;
  const [after, setAfter] = createSignal<string | null>(null);
  const [trail, setTrail] = createSignal<(string | null)[]>([]);
  const src = () => `${base()}?limit=50${after() ? `&after=${enc(after()!)}` : ""}`;
  const [list, { refetch }] = createResource(src, (u) => api<{ items: AppUser[]; next: string | null }>("GET", u));
  const [error, setError] = createSignal<ApiError | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [form, setForm] = createSignal(false);
  const [email, setEmail] = createSignal("");
  const [name, setName] = createSignal("");
  const [password, setPassword] = createSignal("");

  const create = async (e: SubmitEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api("POST", base(), { email: email(), name: name(), password: password() });
      toast("User created");
      setForm(false);
      setEmail("");
      setName("");
      setPassword("");
      refetch();
    } catch (err) {
      setError(err as ApiError);
    } finally {
      setBusy(false);
    }
  };

  const toggle = async (u: AppUser) => {
    try {
      await api("PATCH", `${base()}/${enc(u.id)}`, { disabled: !u.disabled });
      toast(u.disabled ? "User enabled" : "User disabled and signed out");
      refetch();
    } catch (err) {
      setError(err as ApiError);
    }
  };

  const remove = async (u: AppUser) => {
    if (!confirm(`Delete ${u.email ?? u.name ?? u.id}? This cannot be undone.`)) return;
    try {
      await api("DELETE", `${base()}/${enc(u.id)}`);
      toast("User deleted");
      refetch();
    } catch (err) {
      setError(err as ApiError);
    }
  };

  return (
    <div class="page">
      <header class="page-head">
        <div class="min0">
          <p class="crumbs">
            <a href={`#/apps/${enc(params.app!)}`}>{params.app}</a>
          </p>
          <h1>Users</h1>
          <p class="muted">People who signed up to this app, by email or through a provider.</p>
        </div>
        <button class="btn" onClick={() => { setError(null); setForm(true); }}>
          <Icon name="plus" />
          Add user
        </button>
      </header>
      <ErrorBox error={form() ? null : (error() ?? (list.error as ApiError | undefined))} />
      <div class="table-wrap">
        <table class="grid">
          <thead>
            <tr>
              <th scope="col">User</th>
              <th scope="col">Sign-in</th>
              <th scope="col">Created</th>
              <th scope="col">Last sign-in</th>
              <th scope="col">State</th>
              <th scope="col"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            <For each={ready(list)?.items ?? []} fallback={<tr><td colspan="6" class="empty-row">{list.loading ? "Loading…" : "No users yet."}</td></tr>}>
              {(u) => (
                <tr>
                  <td>
                    {u.email ?? <span class="muted">no email</span>}
                    <Show when={u.name}>
                      <br />
                      <span class="muted small">{u.name}</span>
                    </Show>
                  </td>
                  <td class="small">{u.providers.join(", ")}</td>
                  <td class="small">{fmtDate(u.created)}</td>
                  <td class="small">{u.lastLogin ? fmtDate(u.lastLogin) : "Never"}</td>
                  <td class="small">{u.disabled ? "Disabled" : "Active"}</td>
                  <td class="num nowrap">
                    <button class="btn secondary sm" onClick={() => toggle(u)}>{u.disabled ? "Enable" : "Disable"}</button>{" "}
                    <button class="icon-btn danger" aria-label={`Delete ${u.email ?? u.id}`} title="Delete" onClick={() => remove(u)}>
                      <Icon name="trash" />
                    </button>
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>
      <div class="pager">
        <button class="btn secondary sm" disabled={trail().length === 0} onClick={() => { const t = [...trail()]; setAfter(t.pop() ?? null); setTrail(t); }}>Previous</button>{" "}
        <button class="btn secondary sm" disabled={!ready(list)?.next} onClick={() => { setTrail([...trail(), after()]); setAfter(ready(list)!.next); }}>Next</button>
      </div>

      <Show when={form()}>
        <Drawer
          title="Add user"
          onClose={() => setForm(false)}
          onSubmit={create}
          footer={
            <>
              <button type="button" class="btn secondary" onClick={() => setForm(false)}>Cancel</button>
              <button class="btn" type="submit" disabled={busy()}>Create</button>
            </>
          }
        >
          <ErrorBox error={error()} />
          <div class="field">
            <label for="uemail">Email</label>
            <input id="uemail" type="email" required value={email()} onInput={(e) => setEmail(e.currentTarget.value)} />
          </div>
          <div class="field">
            <label for="uname">Name</label>
            <input id="uname" maxlength="100" value={name()} onInput={(e) => setName(e.currentTarget.value)} />
          </div>
          <div class="field">
            <label for="upass">Password</label>
            <input id="upass" type="password" required autocomplete="new-password" value={password()} onInput={(e) => setPassword(e.currentTarget.value)} />
            <span class="help">At least the minimum length set under Authentication.</span>
          </div>
        </Drawer>
      </Show>
    </div>
  );
}
