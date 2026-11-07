import { createResource, createSignal, For, Show } from "solid-js";
import { useParams } from "@solidjs/router";
import { ApiError, api, enc, ready, token, type ApiError as ApiErr, type Plugin } from "../api";
import { Drawer, fmtDate, Icon, toast } from "../ui";
import ErrorBox from "./ErrorBox";

/** WASM plugins of one app: upload a module, choose events, watch calls, errors and logs. */
export default function Plugins() {
  const params = useParams();
  const base = () => `/api/v1/apps/${enc(params.app!)}/plugins`;
  const [list, { refetch }] = createResource(base, (b) => api<{ items: Plugin[] }>("GET", b));
  const [error, setError] = createSignal<ApiErr | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [form, setForm] = createSignal(false);
  const [name, setName] = createSignal("");
  const [events, setEvents] = createSignal("*");
  const [fuel, setFuel] = createSignal("");
  const [file, setFile] = createSignal<File | null>(null);
  const [editing, setEditing] = createSignal<Plugin | null>(null);
  const [enabled, setEnabled] = createSignal(true);
  const [logOf, setLogOf] = createSignal<Plugin | null>(null);
  const [logs, { refetch: refetchLogs }] = createResource(
    () => logOf()?.id,
    (id) => api<{ items: string[] }>("GET", `${base()}/${enc(id)}/logs`),
  );

  const open = (p?: Plugin) => {
    setEditing(p ?? null);
    setName(p?.name ?? "");
    setEvents((p?.events ?? ["*"]).join(", "));
    setFuel(p ? String(p.fuel) : "");
    setEnabled(p?.enabled ?? true);
    setFile(null);
    setError(null);
    setForm(true);
  };

  const evList = () => events().split(",").map((s) => s.trim()).filter(Boolean);

  const save = async (e: SubmitEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const old = editing();
      if (old) {
        await api("PUT", `${base()}/${enc(old.id)}`, { name: name(), events: evList(), enabled: enabled(), ...(fuel() ? { fuel: Number(fuel()) } : {}) });
        toast("Plugin updated");
      } else {
        const f = file();
        if (!f) throw new ApiError({ type: "validation", title: "Choose a .wasm file", status: 400 });
        const q = new URLSearchParams({ name: name(), events: evList().join(","), enabled: String(enabled()) });
        if (fuel()) q.set("fuel", fuel());
        const res = await fetch(`${base()}?${q}`, {
          method: "POST",
          headers: { authorization: `Bearer ${token()}`, "content-type": "application/wasm" },
          body: f,
        });
        if (!res.ok) throw new ApiError(await res.json().catch(() => ({ type: "error", title: res.statusText, status: res.status })));
        toast("Plugin installed");
      }
      setForm(false);
      refetch();
    } catch (err) {
      setError(err as ApiErr);
    } finally {
      setBusy(false);
    }
  };

  const remove = async (p: Plugin) => {
    if (!confirm(`Delete plugin "${p.name}"?`)) return;
    try {
      await api("DELETE", `${base()}/${enc(p.id)}`);
      toast("Plugin deleted");
      if (logOf()?.id === p.id) setLogOf(null);
      refetch();
    } catch (err) {
      setError(err as ApiErr);
    }
  };

  return (
    <div class="page">
      <header class="page-head">
        <div class="min0">
          <p class="crumbs">
            <a href={`#/apps/${enc(params.app!)}`}>{params.app}</a>
          </p>
          <h1>Plugins</h1>
          <p class="muted">
            Sandboxed WebAssembly handlers that run after a record changes. No network or database access; each call has a fuel and memory budget.
          </p>
        </div>
        <button class="btn" onClick={() => open()}>
          <Icon name="plug" />
          Install plugin
        </button>
      </header>
      <ErrorBox error={form() ? null : (error() ?? (list.error as ApiErr | undefined))} />
      <div class="table-wrap">
        <table class="grid">
          <thead>
            <tr>
              <th scope="col">Name</th>
              <th scope="col">Events</th>
              <th scope="col">Size</th>
              <th scope="col">Calls</th>
              <th scope="col">Last error</th>
              <th scope="col">State</th>
              <th scope="col"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            <For each={ready(list)?.items ?? []} fallback={<tr><td colspan="7" class="empty-row">{list.loading ? "Loading…" : "No plugins yet."}</td></tr>}>
              {(p) => (
                <tr>
                  <td>{p.name}</td>
                  <td class="mono small">{p.events.join(", ")}</td>
                  <td class="num small">{(p.size / 1024).toFixed(1)} KiB</td>
                  <td class="small">
                    {p.calls} calls, {p.errors} failed
                    <Show when={p.lastRun}>
                      <br />
                      last {fmtDate(p.lastRun!)}
                    </Show>
                  </td>
                  <td class="small">{p.lastError ?? ""}</td>
                  <td class="small">{p.enabled ? "Enabled" : "Paused"}</td>
                  <td class="num nowrap">
                    <button class="btn secondary sm" onClick={() => setLogOf(p)}>Logs</button>{" "}
                    <button class="btn secondary sm" onClick={() => open(p)}>Edit</button>{" "}
                    <button class="icon-btn danger" aria-label={`Delete ${p.name}`} title="Delete" onClick={() => remove(p)}>
                      <Icon name="trash" />
                    </button>
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>

      <Show when={logOf()}>
        {(p) => (
          <section class="stack">
            <div class="page-head sub-head">
              <h2>Logs of {p().name}</h2>
              <div>
                <button class="btn secondary sm" onClick={() => refetchLogs()}>Refresh</button>{" "}
                <button class="btn secondary sm" onClick={() => setLogOf(null)}>Close</button>
              </div>
            </div>
            <pre class="mono small" style={{ "white-space": "pre-wrap" }}>
              {(ready(logs)?.items ?? []).join("\n") || (logs.loading ? "Loading…" : "No log lines since the server started.")}
            </pre>
          </section>
        )}
      </Show>

      <Show when={form()}>
        <Drawer
          title={editing() ? "Edit plugin" : "Install plugin"}
          onClose={() => setForm(false)}
          onSubmit={save}
          footer={
            <>
              <button type="button" class="btn secondary" onClick={() => setForm(false)}>Cancel</button>
              <button class="btn" type="submit" disabled={busy()}>{editing() ? "Save" : "Install"}</button>
            </>
          }
        >
          <ErrorBox error={error()} />
          <div class="field">
            <label for="pname">Name</label>
            <input id="pname" required maxlength="100" value={name()} onInput={(e) => setName(e.currentTarget.value)} />
          </div>
          <Show when={!editing()}>
            <div class="field">
              <label for="pfile">WebAssembly module</label>
              <input id="pfile" type="file" accept=".wasm,application/wasm" required onChange={(e) => setFile(e.currentTarget.files?.[0] ?? null)} />
              <span class="help">Must export <code>memory</code>, <code>alloc</code> and <code>on_event</code>; the only import allowed is <code>monolith.log</code>. Up to 8 MiB.</span>
            </div>
          </Show>
          <div class="field">
            <label for="pevents">Events</label>
            <input id="pevents" class="mono" required value={events()} onInput={(e) => setEvents(e.currentTarget.value)} />
            <span class="help">Comma separated <code>collection.op</code> patterns, e.g. <code>orders.*</code>.</span>
          </div>
          <div class="field">
            <label for="pfuel">Fuel per call</label>
            <input id="pfuel" type="number" min="1000" max="5000000000" placeholder="50000000" value={fuel()} onInput={(e) => setFuel(e.currentTarget.value)} />
            <span class="help">Instruction budget; a call that runs out of fuel is stopped and counted as an error.</span>
          </div>
          <div class="field">
            <label>
              <input type="checkbox" checked={enabled()} onChange={(e) => setEnabled(e.currentTarget.checked)} /> Enabled
            </label>
          </div>
        </Drawer>
      </Show>
    </div>
  );
}
