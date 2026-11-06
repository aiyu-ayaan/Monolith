import { createResource, createSignal, For, Show } from "solid-js";
import { api, enc, type ApiError, type AppInfo } from "../api";
import ErrorBox from "./ErrorBox";

export default function Apps() {
  const [apps, { refetch }] = createResource(() => api<{ items: AppInfo[] }>("GET", "/api/v1/system/apps"));
  const [name, setName] = createSignal("");
  const [error, setError] = createSignal<ApiError | null>(null);

  const run = async (f: () => Promise<unknown>) => {
    setError(null);
    try {
      await f();
      refetch();
    } catch (e) {
      setError(e as ApiError);
    }
  };

  const create = (e: SubmitEvent) => {
    e.preventDefault();
    run(async () => {
      await api("POST", "/api/v1/system/apps", { name: name() });
      setName("");
    });
  };

  const remove = (app: string) => {
    if (!confirm(`Delete app "${app}" and all its data? This cannot be undone.`)) return;
    run(() => api("DELETE", `/api/v1/system/apps/${enc(app)}`));
  };

  return (
    <section>
      <h2>Apps</h2>
      <form class="row" onSubmit={create}>
        <input placeholder="new app name (a-z, 0-9, _)" pattern="[a-z][a-z0-9_]{0,62}" required value={name()} onInput={(e) => setName(e.currentTarget.value)} />
        <button type="submit">Create app</button>
      </form>
      <ErrorBox error={error() ?? (apps.error as ApiError | undefined)} />
      <Show when={apps()} fallback={<p class="muted">Loading…</p>}>
        <table>
          <thead>
            <tr>
              <th>Name</th>
              <th>Created</th>
              <th />
            </tr>
          </thead>
          <tbody>
            <For each={apps()!.items} fallback={<tr><td colspan="3" class="muted">No apps yet.</td></tr>}>
              {(a) => (
                <tr>
                  <td>
                    <a href={`#/apps/${enc(a.name)}`}>{a.name}</a>
                  </td>
                  <td class="muted">{a.created}</td>
                  <td class="right">
                    <button class="danger ghost" onClick={() => remove(a.name)}>
                      Delete
                    </button>
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </Show>
    </section>
  );
}
