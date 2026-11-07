import { createResource, createSignal, For, Show } from "solid-js";
import { useNavigate, useParams } from "@solidjs/router";
import { api, enc, ready, token, type ApiError, type Backup } from "../api";
import { fmtBytes } from "../charts";
import { Drawer, fmtDate, Icon, toast } from "../ui";
import ErrorBox from "./ErrorBox";

/** Snapshot backups of one app: take, download, restore into a new app, delete. */
export default function Backups() {
  const params = useParams();
  const navigate = useNavigate();
  const base = () => `/api/v1/apps/${enc(params.app!)}`;
  const [list, { refetch }] = createResource(base, (b) => api<{ items: Backup[] }>("GET", `${b}/backups`));
  const [error, setError] = createSignal<ApiError | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [restoring, setRestoring] = createSignal<Backup | null>(null);
  const [newApp, setNewApp] = createSignal("");

  const run = async (f: () => Promise<unknown>, done: string) => {
    setError(null);
    setBusy(true);
    try {
      await f();
      toast(done);
      refetch();
    } catch (e) {
      setError(e as ApiError);
    } finally {
      setBusy(false);
    }
  };

  // shortcut: the file is buffered in the browser (the request needs the auth header, so a plain link will not do).
  // Fine for admin-sized databases; switch to short-lived signed URLs for very large ones.
  const download = (b: Backup) =>
    run(async () => {
      const res = await fetch(`${base()}/backups/${enc(b.name)}`, { headers: { authorization: `Bearer ${token()}` } });
      if (!res.ok) throw { problem: (await res.json().catch(() => null)) ?? { type: "http", title: `HTTP ${res.status}`, status: res.status } };
      const url = URL.createObjectURL(await res.blob());
      const a = Object.assign(document.createElement("a"), { href: url, download: `${params.app}-${b.name}` });
      a.click();
      URL.revokeObjectURL(url);
    }, "Download started");

  const remove = (b: Backup) => {
    if (!confirm(`Delete backup ${b.name}? This cannot be undone.`)) return;
    run(() => api("DELETE", `${base()}/backups/${enc(b.name)}`), "Backup deleted");
  };

  const restore = async (e: SubmitEvent) => {
    e.preventDefault();
    const b = restoring()!;
    setBusy(true);
    setError(null);
    try {
      await api("POST", `${base()}/backups:restore`, { backup: b.name, newApp: newApp() });
      toast(`Restored into "${newApp()}"`);
      navigate(`/apps/${enc(newApp())}`);
    } catch (err) {
      setError(err as ApiError);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="page">
      <header class="page-head">
        <div class="min0">
          <p class="crumbs">
            <a href={`#/apps/${enc(params.app!)}`}>{params.app}</a>
          </p>
          <h1>Backups</h1>
          <p class="muted">Consistent snapshots of this app's database, taken while it keeps serving. Stored on the server under <code>backups/{params.app}</code>.</p>
        </div>
        <button class="btn" disabled={busy()} onClick={() => run(() => api("POST", `${base()}/backups`), "Backup created")}>
          <Icon name="archive" />
          {busy() ? "Working…" : "Back up now"}
        </button>
      </header>
      <ErrorBox error={restoring() ? null : (error() ?? (list.error as ApiError | undefined))} />
      <p class="notice">
        <Icon name="alert" size={16} />
        <span>
          Manual snapshots only. Storage buckets are included (as links, on the same disk); download gives the database file only. Schedules, retention, S3 targets, encryption and in-place restore are planned. Restore creates a new app, so nothing is overwritten.
        </span>
      </p>
      <div class="table-wrap">
        <table class="grid">
          <thead>
            <tr>
              <th scope="col">Backup</th>
              <th scope="col">Created</th>
              <th scope="col" class="num">Size</th>
              <th scope="col"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            <For each={ready(list)?.items ?? []} fallback={<tr><td colspan="4" class="empty-row">{list.loading ? "Loading…" : "No backups yet."}</td></tr>}>
              {(b) => (
                <tr>
                  <td class="mono small">{b.name}{b.files ? " + files" : ""}</td>
                  <td class="small">{fmtDate(b.created)}</td>
                  <td class="num">{fmtBytes(b.bytes)}</td>
                  <td class="num nowrap">
                    <button class="btn secondary sm" disabled={busy()} onClick={() => download(b)}>
                      Download
                    </button>{" "}
                    <button class="btn secondary sm" disabled={busy()} onClick={() => { setNewApp(`${params.app}_restored`); setError(null); setRestoring(b); }}>
                      Restore…
                    </button>{" "}
                    <button class="icon-btn danger" aria-label={`Delete ${b.name}`} title="Delete" disabled={busy()} onClick={() => remove(b)}>
                      <Icon name="trash" />
                    </button>
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>

      <Show when={restoring()}>
        {(b) => (
          <Drawer
            title="Restore into a new app"
            onClose={() => setRestoring(null)}
            onSubmit={restore}
            footer={
              <>
                <button type="button" class="btn secondary" onClick={() => setRestoring(null)}>
                  Cancel
                </button>
                <button class="btn" type="submit" disabled={busy()}>
                  Restore
                </button>
              </>
            }
          >
            <ErrorBox error={error()} />
            <p class="muted small">
              Creates a new app with the data and schema of <span class="mono">{b().name}</span> ({fmtBytes(b().bytes)}){b().files ? " and its storage buckets" : ""}. <span class="mono">{params.app}</span> is not changed.
            </p>
            <div class="field">
              <label for="newapp">New app name</label>
              <input id="newapp" class="mono" required pattern="[a-z][a-z0-9_]{0,62}" value={newApp()} onInput={(e) => setNewApp(e.currentTarget.value)} />
              <span class="help">Lowercase letters, digits and _, starting with a letter.</span>
            </div>
          </Drawer>
        )}
      </Show>
    </div>
  );
}
