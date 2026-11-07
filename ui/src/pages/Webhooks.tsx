import { createResource, createSignal, For, Show } from "solid-js";
import { useParams } from "@solidjs/router";
import { api, enc, ready, type ApiError, type Webhook, type WebhookDelivery } from "../api";
import { Drawer, fmtDate, Icon, toast } from "../ui";
import ErrorBox from "./ErrorBox";

/** Outbound webhooks of one app: create (secret shown once), edit, test, delivery log. */
export default function Webhooks() {
  const params = useParams();
  const base = () => `/api/v1/apps/${enc(params.app!)}/webhooks`;
  const [list, { refetch }] = createResource(base, (b) => api<{ items: Webhook[] }>("GET", b));
  const [error, setError] = createSignal<ApiError | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [form, setForm] = createSignal(false);
  const [secret, setSecret] = createSignal<string | null>(null);
  const [name, setName] = createSignal("");
  const [url, setUrl] = createSignal("");
  const [events, setEvents] = createSignal("*");
  const [enabled, setEnabled] = createSignal(true);
  const [editing, setEditing] = createSignal<Webhook | null>(null);
  const [logOf, setLogOf] = createSignal<Webhook | null>(null);
  const [status, setStatus] = createSignal("");
  const logUrl = () => {
    const h = logOf();
    return h ? `${base()}/${enc(h.id)}/deliveries?limit=100${status() ? `&status=${status()}` : ""}` : null;
  };
  const [log, { refetch: refetchLog }] = createResource(logUrl, (u) => api<{ items: WebhookDelivery[] }>("GET", u));

  const open = (h?: Webhook) => {
    setEditing(h ?? null);
    setName(h?.name ?? "");
    setUrl(h?.url ?? "");
    setEvents((h?.events ?? ["*"]).join(", "));
    setEnabled(h?.enabled ?? true);
    setError(null);
    setForm(true);
  };

  const save = async (e: SubmitEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const body = {
        name: name(),
        url: url(),
        enabled: enabled(),
        events: events().split(",").map((s) => s.trim()).filter(Boolean),
      };
      const old = editing();
      if (old) {
        await api("PUT", `${base()}/${enc(old.id)}`, body);
        toast("Webhook updated");
      } else {
        setSecret((await api<Webhook & { secret: string }>("POST", base(), body)).secret);
      }
      setForm(false);
      refetch();
    } catch (err) {
      setError(err as ApiError);
    } finally {
      setBusy(false);
    }
  };

  const remove = async (h: Webhook) => {
    if (!confirm(`Delete "${h.name}" and its delivery log?`)) return;
    try {
      await api("DELETE", `${base()}/${enc(h.id)}`);
      toast("Webhook deleted");
      if (logOf()?.id === h.id) setLogOf(null);
      refetch();
    } catch (err) {
      setError(err as ApiError);
    }
  };

  const ping = async (h: Webhook) => {
    try {
      await api("POST", `${base()}/${enc(h.id)}/test`);
      toast("Ping queued");
      if (logOf()?.id === h.id) setTimeout(refetchLog, 800);
      setTimeout(refetch, 800);
    } catch (err) {
      setError(err as ApiError);
    }
  };

  const retry = async (d: WebhookDelivery) => {
    try {
      await api("POST", `${base()}/${enc(logOf()!.id)}/deliveries/${enc(d.id)}/retry`);
      toast("Queued again");
      setTimeout(refetchLog, 800);
    } catch (err) {
      setError(err as ApiError);
    }
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(secret()!);
      toast("Copied");
    } catch {
      toast("Select the secret and copy it manually");
    }
  };

  return (
    <div class="page">
      <header class="page-head">
        <div class="min0">
          <p class="crumbs">
            <a href={`#/apps/${enc(params.app!)}`}>{params.app}</a>
          </p>
          <h1>Webhooks</h1>
          <p class="muted">
            Call your server when records change. Each request is signed (<code>X-Monolith-Signature</code>) and retried with backoff.
          </p>
        </div>
        <button class="btn" onClick={() => open()}>
          <Icon name="plug" />
          Add webhook
        </button>
      </header>
      <ErrorBox error={form() ? null : (error() ?? (list.error as ApiError | undefined))} />
      <Show when={secret()}>
        <div class="notice" role="status">
          <Icon name="check" size={16} />
          <span class="min0">
            Copy the signing secret now; it is shown once.
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
              <th scope="col">URL</th>
              <th scope="col">Events</th>
              <th scope="col">Queue</th>
              <th scope="col">State</th>
              <th scope="col"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            <For each={ready(list)?.items ?? []} fallback={<tr><td colspan="6" class="empty-row">{list.loading ? "Loading…" : "No webhooks yet."}</td></tr>}>
              {(h) => (
                <tr>
                  <td>{h.name}</td>
                  <td class="mono small truncate" style={{ "max-width": "260px" }}>{h.url}</td>
                  <td class="mono small">{h.events.join(", ")}</td>
                  <td class="small">
                    {h.pending} pending
                    <Show when={h.dead > 0}>
                      <br />
                      <strong>{h.dead} dead</strong>
                    </Show>
                  </td>
                  <td class="small">{h.enabled ? "Enabled" : "Paused"}</td>
                  <td class="num nowrap">
                    <button class="btn secondary sm" onClick={() => setLogOf(h)}>Log</button>{" "}
                    <button class="btn secondary sm" onClick={() => ping(h)}>Test</button>{" "}
                    <button class="btn secondary sm" onClick={() => open(h)}>Edit</button>{" "}
                    <button class="icon-btn danger" aria-label={`Delete ${h.name}`} title="Delete" onClick={() => remove(h)}>
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
        {(h) => (
          <section class="stack">
            <div class="page-head sub-head">
              <h2>Deliveries of {h().name}</h2>
              <div>
                <select aria-label="Filter by status" value={status()} onChange={(e) => setStatus(e.currentTarget.value)}>
                  <option value="">All</option>
                  <option value="pending">Pending</option>
                  <option value="ok">Delivered</option>
                  <option value="dead">Dead</option>
                </select>{" "}
                <button class="btn secondary sm" onClick={() => refetchLog()}>Refresh</button>{" "}
                <button class="btn secondary sm" onClick={() => setLogOf(null)}>Close</button>
              </div>
            </div>
            <div class="table-wrap">
              <table class="grid">
                <thead>
                  <tr>
                    <th scope="col">Event</th>
                    <th scope="col">Seq</th>
                    <th scope="col">Status</th>
                    <th scope="col">Attempts</th>
                    <th scope="col">Last result</th>
                    <th scope="col">Created</th>
                    <th scope="col"><span class="sr-only">Actions</span></th>
                  </tr>
                </thead>
                <tbody>
                  <For each={ready(log)?.items ?? []} fallback={<tr><td colspan="7" class="empty-row">{log.loading ? "Loading…" : "No deliveries."}</td></tr>}>
                    {(d) => (
                      <tr>
                        <td class="mono small">{d.event}</td>
                        <td class="num">{d.seq}</td>
                        <td class="small">
                          {d.status}
                          <Show when={d.nextAttempt}>
                            <br />
                            next {fmtDate(d.nextAttempt!)}
                          </Show>
                        </td>
                        <td class="num">{d.attempts}</td>
                        <td class="small">{d.lastError ?? (d.lastStatus ? `HTTP ${d.lastStatus}` : "")}</td>
                        <td class="small">{fmtDate(d.created)}</td>
                        <td class="num">
                          <Show when={d.status !== "pending"}>
                            <button class="btn secondary sm" onClick={() => retry(d)}>Retry</button>
                          </Show>
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

      <Show when={form()}>
        <Drawer
          title={editing() ? "Edit webhook" : "Add webhook"}
          onClose={() => setForm(false)}
          onSubmit={save}
          footer={
            <>
              <button type="button" class="btn secondary" onClick={() => setForm(false)}>Cancel</button>
              <button class="btn" type="submit" disabled={busy()}>{editing() ? "Save" : "Create"}</button>
            </>
          }
        >
          <ErrorBox error={error()} />
          <div class="field">
            <label for="hname">Name</label>
            <input id="hname" required maxlength="100" value={name()} onInput={(e) => setName(e.currentTarget.value)} />
          </div>
          <div class="field">
            <label for="hurl">Endpoint URL</label>
            <input id="hurl" type="url" required class="mono" placeholder="https://example.com/hooks/monolith" value={url()} onInput={(e) => setUrl(e.currentTarget.value)} />
          </div>
          <div class="field">
            <label for="hevents">Events</label>
            <input id="hevents" class="mono" required value={events()} onInput={(e) => setEvents(e.currentTarget.value)} />
            <span class="help">Comma separated <code>collection.op</code> patterns: <code>orders.create</code>, <code>orders.*</code>, <code>*.delete</code>, <code>*</code>.</span>
          </div>
          <div class="field">
            <label>
              <input type="checkbox" checked={enabled()} onChange={(e) => setEnabled(e.currentTarget.checked)} /> Enabled
            </label>
            <span class="help">Changes made while a webhook is paused or before it existed are not sent.</span>
          </div>
        </Drawer>
      </Show>
    </div>
  );
}
