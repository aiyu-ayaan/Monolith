import { createEffect, createResource, createSignal, For, on, Show } from "solid-js";
import { createStore } from "solid-js/store";
import { useParams } from "@solidjs/router";
import { api, enc, type ApiError, type Collection, type Field, type RecordRow } from "../api";
import { Drawer, fmtDate, Icon, toast } from "../ui";
import { useApp } from "./AppLayout";
import ErrorBox from "./ErrorBox";

interface Page {
  items: RecordRow[];
  next: string | null;
}

const cell = (v: unknown) => (v === null || v === undefined ? "" : typeof v === "object" ? JSON.stringify(v) : String(v));

export default function Records() {
  const params = useParams();
  const ctx = useApp();
  const coll = () => ctx.collections()?.items.find((c) => c.name === params.coll);
  const collPath = () => `/api/v1/apps/${enc(params.app!)}/collections/${enc(params.coll!)}`;

  const [filter, setFilter] = createSignal("");
  const [sort, setSort] = createSignal("-created");
  // Cursor history: index 0 is the first page (no cursor).
  const [cursors, setCursors] = createSignal<(string | null)[]>([null]);
  const [query, setQuery] = createSignal({ filter: "", sort: "-created", cursor: null as string | null, n: 0 });

  createEffect(
    on(
      () => params.coll,
      () => {
        setFilter("");
        setSort("-created");
        setCursors([null]);
        setQuery({ filter: "", sort: "-created", cursor: null, n: 0 });
      },
      { defer: true },
    ),
  );

  // Refetches when the collection's schema version changes too.
  const [page, { refetch }] = createResource(
    () => ({ ...query(), path: collPath(), v: coll()?.version }),
    (q) => {
      const sp = new URLSearchParams({ limit: "50" });
      if (q.filter) sp.set("filter", q.filter);
      if (q.sort) sp.set("sort", q.sort);
      if (q.cursor) sp.set("cursor", q.cursor);
      return api<Page>("GET", `${q.path}/records?${sp}`);
    },
  );

  const [editing, setEditing] = createSignal<{ r: RecordRow | null } | null>(null);

  const apply = (e?: SubmitEvent) => {
    e?.preventDefault();
    setCursors([null]);
    setQuery({ filter: filter(), sort: sort(), cursor: null, n: query().n + 1 });
  };

  const next = () => {
    const c = page()?.next;
    if (!c) return;
    setCursors([...cursors(), c]);
    setQuery({ ...query(), cursor: c });
  };

  const prev = () => {
    const cs = cursors().slice(0, -1);
    setCursors(cs);
    setQuery({ ...query(), cursor: cs[cs.length - 1] ?? null });
  };

  const fields = () => coll()?.fields ?? [];

  return (
    <div class="page wide">
      <header class="page-head">
        <div class="min0">
          <p class="crumbs">
            <a href={`#/apps/${enc(params.app!)}`}>{params.app}</a>
            <span aria-hidden="true"> / </span>
            collections
          </p>
          <h1 class="truncate">{params.coll}</h1>
        </div>
        <div class="actions">
          <button class="icon-btn" aria-label="Refresh records" title="Refresh" onClick={() => refetch()}>
            <Icon name="refresh" />
          </button>
          <button class="btn secondary" disabled={!coll()} onClick={() => ctx.editSchema(coll()!)}>
            <Icon name="settings" />
            Edit collection
          </button>
          <button class="btn" disabled={!coll()} onClick={() => setEditing({ r: null })}>
            <Icon name="plus" />
            New record
          </button>
        </div>
      </header>

      <form class="toolbar" onSubmit={apply}>
        <div class="search grow">
          <Icon name="search" size={16} />
          <input
            aria-label="Filter"
            class="mono"
            placeholder="Filter, e.g. status = 'open' && created > '2026-01-01'"
            value={filter()}
            onInput={(e) => setFilter(e.currentTarget.value)}
          />
        </div>
        <input aria-label="Sort" class="mono sort" placeholder="-created" value={sort()} onInput={(e) => setSort(e.currentTarget.value)} />
        <button class="btn secondary" type="submit">
          Apply
        </button>
      </form>

      <ErrorBox error={(page.error ?? ctx.collections.error) as ApiError | undefined} />

      <div class="table-wrap" aria-busy={page.loading}>
        <table class="grid">
          <thead>
            <tr>
              <th scope="col">id</th>
              <For each={fields()}>
                {(f) => (
                  <th scope="col">
                    {f.name}
                    <span class="type-tag">{f.type}</span>
                  </th>
                )}
              </For>
              <th scope="col">updated</th>
            </tr>
          </thead>
          <tbody>
            <For
              each={page()?.items}
              fallback={
                <tr>
                  <td colspan={fields().length + 2} class="empty-row">
                    {page.loading ? "Loading…" : query().filter ? "No records match this filter." : "No records yet."}
                  </td>
                </tr>
              }
            >
              {(r) => (
                <tr class="clickable" onClick={() => setEditing({ r })}>
                  <td>
                    <button class="link mono id" onClick={(e) => (e.stopPropagation(), setEditing({ r }))}>
                      {r.id}
                    </button>
                  </td>
                  <For each={fields()}>
                    {(f) => (
                      <td title={cell(r[f.name])} classList={{ mono: f.type === "json" || f.type === "relation" }}>
                        <Show when={f.type === "bool"} fallback={<span class="truncate cell">{cell(r[f.name])}</span>}>
                          <span class="chip" classList={{ on: !!r[f.name] }}>
                            {r[f.name] ? "true" : "false"}
                          </span>
                        </Show>
                      </td>
                    )}
                  </For>
                  <td class="muted nowrap">{fmtDate(r.updated)}</td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>

      <nav class="pager" aria-label="Pagination">
        <span class="muted small">Page {cursors().length}</span>
        <button class="btn ghost" disabled={cursors().length <= 1} onClick={prev}>
          <Icon name="chevronLeft" />
          Previous
        </button>
        <button class="btn ghost" disabled={!page()?.next} onClick={next}>
          Next
          <Icon name="chevronRight" />
        </button>
      </nav>

      <Show when={editing() && coll()}>
        <RecordForm
          coll={coll()!}
          record={editing()!.r}
          path={collPath()}
          onClose={() => setEditing(null)}
          onDone={(msg) => {
            setEditing(null);
            refetch();
            toast(msg);
          }}
        />
      </Show>
    </div>
  );
}

// ---------- record form ----------

type FormVal = string | boolean;

function toForm(f: Field, v: unknown): FormVal {
  if (f.type === "bool") return !!v;
  if (v === null || v === undefined) return "";
  if (f.type === "json") return JSON.stringify(v, null, 2);
  return String(v);
}

/** Form value → wire value; empty means null. Throws on invalid JSON. */
function fromForm(f: Field, v: FormVal): unknown {
  if (f.type === "bool") return v === true;
  const s = String(v);
  if (s.trim() === "") return null;
  if (f.type === "number") return Number(s);
  if (f.type === "json") return JSON.parse(s);
  return s;
}

const INPUT_TYPE: Partial<Record<Field["type"], string>> = { email: "email", url: "url", number: "number" };

function RecordForm(p: {
  coll: Collection;
  record: RecordRow | null;
  path: string;
  onClose: () => void;
  onDone: (msg: string) => void;
}) {
  const [vals, setVals] = createStore<Record<string, FormVal>>(Object.fromEntries(p.coll.fields.map((f) => [f.name, toForm(f, p.record?.[f.name])])));
  const [error, setError] = createSignal<ApiError | null>(null);
  const [busy, setBusy] = createSignal(false);
  const fieldErr = (name: string) => error()?.problem.errors?.find((e) => e.field === name)?.message;

  const save = async (e: SubmitEvent) => {
    e.preventDefault();
    setError(null);
    const body: Record<string, unknown> = {};
    for (const f of p.coll.fields) {
      try {
        body[f.name] = fromForm(f, vals[f.name]);
      } catch (err) {
        setError({ problem: { type: "json", title: "Invalid JSON", status: 0, errors: [{ field: f.name, code: "json", message: String(err) }] } } as ApiError);
        return;
      }
    }
    setBusy(true);
    try {
      if (p.record) await api("PATCH", `${p.path}/records/${enc(p.record.id)}`, body);
      else await api("POST", `${p.path}/records`, body);
      p.onDone(p.record ? "Record saved" : "Record created");
    } catch (err) {
      setError(err as ApiError);
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    const id = p.record!.id;
    if (!confirm(`Delete record ${id}? This cannot be undone.`)) return;
    try {
      await api("DELETE", `${p.path}/records/${enc(id)}`);
      p.onDone("Record deleted");
    } catch (err) {
      setError(err as ApiError);
    }
  };

  return (
    <Drawer
      title={p.record ? `Edit ${p.coll.name} record` : `New ${p.coll.name} record`}
      onClose={p.onClose}
      onSubmit={save}
      footer={
        <>
          <Show when={p.record}>
            <button type="button" class="btn ghost danger" onClick={remove}>
              <Icon name="trash" />
              Delete
            </button>
          </Show>
          <span class="grow" />
          <button type="button" class="btn ghost" onClick={p.onClose}>
            Cancel
          </button>
          <button type="submit" class="btn" disabled={busy()}>
            {busy() ? "Saving…" : p.record ? "Save changes" : "Create"}
          </button>
        </>
      }
    >
      <Show when={p.record}>
        {(r) => (
          <dl class="meta">
            <dt>id</dt>
            <dd class="mono">{r().id}</dd>
            <dt>created</dt>
            <dd>{fmtDate(r().created)}</dd>
            <dt>updated</dt>
            <dd>{fmtDate(r().updated)}</dd>
          </dl>
        )}
      </Show>
      <div class="stack">
        <For each={p.coll.fields}>
          {(f) => {
            const id = `rec-${f.name}`;
            const err = () => fieldErr(f.name);
            const set = (v: FormVal) => setVals(f.name, v);
            return (
              <div class="field">
                <Show
                  when={f.type !== "bool"}
                  fallback={
                    <label class="check">
                      <input id={id} type="checkbox" checked={vals[f.name] === true} onChange={(e) => set(e.currentTarget.checked)} />
                      {f.name}
                    </label>
                  }
                >
                  <label for={id}>
                    {f.name}
                    <Show when={f.required}>
                      <span class="req" aria-hidden="true">
                        {" "}
                        *
                      </span>
                    </Show>
                    <span class="type-tag">{f.type}</span>
                  </label>
                  <Show
                    when={f.type === "select"}
                    fallback={
                      <Show
                        when={f.type === "json"}
                        fallback={
                          <input
                            id={id}
                            classList={{ mono: f.type === "relation" || f.type === "date" }}
                            type={INPUT_TYPE[f.type] ?? "text"}
                            step={f.type === "number" && !f.onlyInt ? "any" : undefined}
                            required={f.required}
                            placeholder={f.type === "date" ? "2026-01-01T00:00:00Z" : f.type === "relation" ? `${f.collection} id` : undefined}
                            aria-invalid={!!err()}
                            aria-describedby={err() ? `${id}-err` : undefined}
                            value={vals[f.name] as string}
                            onInput={(e) => set(e.currentTarget.value)}
                          />
                        }
                      >
                        <textarea
                          id={id}
                          class="mono"
                          rows="5"
                          aria-invalid={!!err()}
                          aria-describedby={err() ? `${id}-err` : undefined}
                          value={vals[f.name] as string}
                          onInput={(e) => set(e.currentTarget.value)}
                        />
                      </Show>
                    }
                  >
                    <select id={id} required={f.required} value={vals[f.name] as string} onChange={(e) => set(e.currentTarget.value)}>
                      <option value="">—</option>
                      <For each={f.values ?? []}>{(v) => <option value={v}>{v}</option>}</For>
                    </select>
                  </Show>
                </Show>
                <Show when={err()}>
                  <p id={`${id}-err`} class="field-error">
                    {err()}
                  </p>
                </Show>
              </div>
            );
          }}
        </For>
      </div>
      <ErrorBox error={error()} />
    </Drawer>
  );
}
