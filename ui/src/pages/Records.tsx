import { createResource, createSignal, For, Show } from "solid-js";
import { useParams } from "@solidjs/router";
import { api, enc, type ApiError, type Collection, type RecordRow } from "../api";
import ErrorBox from "./ErrorBox";

interface Page {
  items: RecordRow[];
  next: string | null;
}

const cell = (v: unknown) => (v === null || v === undefined ? "" : typeof v === "object" ? JSON.stringify(v) : String(v));

export default function Records() {
  const params = useParams();
  const collPath = () => `/api/v1/apps/${enc(params.app!)}/collections/${enc(params.coll!)}`;
  const [coll] = createResource(() => params.coll, () => api<Collection>("GET", collPath()));

  const [filter, setFilter] = createSignal("");
  const [sort, setSort] = createSignal("-created");
  // Cursor history: index 0 is the first page (no cursor).
  const [cursors, setCursors] = createSignal<(string | null)[]>([null]);
  const [query, setQuery] = createSignal({ filter: "", sort: "-created", cursor: null as string | null, n: 0 });

  const [page, { refetch }] = createResource(query, (q) => {
    const sp = new URLSearchParams({ limit: "50" });
    if (q.filter) sp.set("filter", q.filter);
    if (q.sort) sp.set("sort", q.sort);
    if (q.cursor) sp.set("cursor", q.cursor);
    return api<Page>("GET", `${collPath()}/records?${sp}`);
  });

  const [editing, setEditing] = createSignal<{ id: string | null; text: string } | null>(null);
  const [error, setError] = createSignal<ApiError | null>(null);

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

  const fieldNames = () => coll()?.fields.map((f) => f.name) ?? [];

  const edit = (r: RecordRow | null) => {
    setError(null);
    const body: Record<string, unknown> = {};
    for (const f of coll()?.fields ?? []) body[f.name] = r ? (r[f.name] ?? null) : null;
    setEditing({ id: r?.id ?? null, text: JSON.stringify(body, null, 2) });
  };

  const saveEdit = async () => {
    const ed = editing();
    if (!ed) return;
    setError(null);
    let body: unknown;
    try {
      body = JSON.parse(ed.text);
    } catch (e) {
      setError({ problem: { type: "json", title: "Invalid JSON", status: 0, detail: String(e) } } as ApiError);
      return;
    }
    try {
      if (ed.id) await api("PATCH", `${collPath()}/records/${enc(ed.id)}`, body);
      else await api("POST", `${collPath()}/records`, body);
      setEditing(null);
      refetch();
    } catch (e) {
      setError(e as ApiError);
    }
  };

  const remove = async (id: string) => {
    if (!confirm(`Delete record ${id}?`)) return;
    try {
      await api("DELETE", `${collPath()}/records/${enc(id)}`);
      refetch();
    } catch (e) {
      setError(e as ApiError);
    }
  };

  return (
    <section>
      <p class="crumbs">
        <a href="#/">Apps</a> / <a href={`#/apps/${enc(params.app!)}`}>{params.app}</a> / {params.coll}
      </p>
      <form class="row" onSubmit={apply}>
        <input class="mono grow" placeholder="filter, e.g. status = 'open' && created > '2026-01-01'" value={filter()} onInput={(e) => setFilter(e.currentTarget.value)} />
        <input class="mono" placeholder="sort, e.g. -created" value={sort()} onInput={(e) => setSort(e.currentTarget.value)} />
        <button type="submit">Apply</button>
        <button type="button" onClick={() => edit(null)}>
          New record
        </button>
      </form>

      <ErrorBox error={error() ?? ((page.error ?? coll.error) as ApiError | undefined)} />

      <Show when={editing()}>
        {(ed) => (
          <div class="card">
            <h4>{ed().id ? `Edit ${ed().id}` : "New record"}</h4>
            <textarea class="mono" rows="10" value={ed().text} onInput={(e) => setEditing({ ...ed(), text: e.currentTarget.value })} />
            <div class="row">
              <button onClick={saveEdit}>Save</button>
              <button class="ghost" onClick={() => setEditing(null)}>
                Cancel
              </button>
            </div>
          </div>
        )}
      </Show>

      <div class="scroll">
        <table>
          <thead>
            <tr>
              <th>id</th>
              <For each={fieldNames()}>{(n) => <th>{n}</th>}</For>
              <th>updated</th>
              <th />
            </tr>
          </thead>
          <tbody>
            <For each={page()?.items} fallback={<tr><td colspan="99" class="muted">{page.loading ? "Loading…" : "No records."}</td></tr>}>
              {(r) => (
                <tr>
                  <td class="mono">{r.id}</td>
                  <For each={fieldNames()}>{(n) => <td>{cell(r[n])}</td>}</For>
                  <td class="muted">{r.updated}</td>
                  <td class="right nowrap">
                    <button class="ghost" onClick={() => edit(r)}>
                      Edit
                    </button>
                    <button class="ghost danger" onClick={() => remove(r.id)}>
                      Delete
                    </button>
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>
      <div class="row">
        <button class="ghost" disabled={cursors().length <= 1} onClick={prev}>
          Previous
        </button>
        <button class="ghost" disabled={!page()?.next} onClick={next}>
          Next
        </button>
      </div>
    </section>
  );
}
