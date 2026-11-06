import { createResource, createSignal, For, Show } from "solid-js";
import { api, enc, type ApiError, type AppInfo } from "../api";
import { fmtDate, Icon, toast } from "../ui";
import ErrorBox from "./ErrorBox";

export default function Apps() {
  const [apps, { refetch }] = createResource(() => api<{ items: AppInfo[] }>("GET", "/api/v1/system/apps"));
  const [name, setName] = createSignal("");
  const [error, setError] = createSignal<ApiError | null>(null);
  const [busy, setBusy] = createSignal(false);

  const run = async (f: () => Promise<unknown>, done: string) => {
    setError(null);
    setBusy(true);
    try {
      await f();
      refetch();
      toast(done);
    } catch (e) {
      setError(e as ApiError);
    } finally {
      setBusy(false);
    }
  };

  const create = (e: SubmitEvent) => {
    e.preventDefault();
    const n = name();
    run(async () => {
      await api("POST", "/api/v1/system/apps", { name: n });
      setName("");
    }, `App "${n}" created`);
  };

  const remove = (app: string) => {
    if (!confirm(`Delete app "${app}" and all its data? This cannot be undone.`)) return;
    run(() => api("DELETE", `/api/v1/system/apps/${enc(app)}`), `App "${app}" deleted`);
  };

  return (
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Apps</h1>
          <p class="muted">Each app has its own database, collections and access rules.</p>
        </div>
      </header>

      <form class="create-bar" onSubmit={create}>
        <label for="app-name" class="sr-only">
          New app name
        </label>
        <input
          id="app-name"
          class="mono"
          placeholder="new_app_name"
          pattern="[a-z][a-z0-9_]{0,62}"
          aria-describedby="app-name-help"
          required
          value={name()}
          onInput={(e) => setName(e.currentTarget.value)}
        />
        <button class="btn" type="submit" disabled={busy()}>
          <Icon name="plus" />
          Create app
        </button>
      </form>
      <p id="app-name-help" class="help">
        Lowercase letters, digits and <code>_</code>, starting with a letter.
      </p>

      <ErrorBox error={error() ?? (apps.error as ApiError | undefined)} />

      <Show when={apps()} fallback={<div class="skeleton" aria-busy="true" aria-label="Loading apps" />}>
        <Show
          when={apps()!.items.length}
          fallback={
            <div class="empty">
              <Icon name="database" size={32} />
              <h2>No apps yet</h2>
              <p class="muted">Create one above, then add collections to it.</p>
            </div>
          }
        >
          <ul class="app-grid">
            <For each={apps()!.items}>
              {(a) => (
                <li class="app-card">
                  <a class="app-link" href={`#/apps/${enc(a.name)}`}>
                    <span class="app-icon">
                      <Icon name="database" />
                    </span>
                    <span class="app-meta">
                      <strong>{a.name}</strong>
                      <span class="muted small">Created {fmtDate(a.created)}</span>
                    </span>
                    <Icon name="chevronRight" />
                  </a>
                  <button class="icon-btn danger" aria-label={`Delete app ${a.name}`} title="Delete app" onClick={() => remove(a.name)}>
                    <Icon name="trash" />
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Show>
    </div>
  );
}
