import { createResource, createSignal, For, Show } from "solid-js";
import { createStore, produce, unwrap } from "solid-js/store";
import { useParams } from "@solidjs/router";
import { api, enc, FIELD_TYPES, RULE_KEYS, type ApiError, type Collection, type Field, type RuleKey } from "../api";
import ErrorBox from "./ErrorBox";

interface Draft {
  isNew: boolean;
  c: Collection;
  indexText: string;
}

const blank = (): Draft => ({
  isNew: true,
  c: { name: "", fields: [], rules: {}, indexes: [] },
  indexText: "",
});

// One index per line: `title` or `unique email` or `owner,created`.
const indexesToText = (ix: Collection["indexes"]) =>
  ix.map((i) => `${i.unique ? "unique " : ""}${i.fields.join(",")}`).join("\n");

const textToIndexes = (t: string): Collection["indexes"] =>
  t
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean)
    .map((l) => {
      const unique = l.startsWith("unique ");
      const fields = (unique ? l.slice(7) : l).split(",").map((s) => s.trim()).filter(Boolean);
      return unique ? { fields, unique } : { fields };
    });

/** Drops empty optional keys so the server's strict schema accepts the field. */
function cleanField(f: Field): Field {
  const out: Field = { name: f.name.trim(), type: f.type };
  if (f.required) out.required = true;
  if (f.hidden) out.hidden = true;
  if (f.type === "number" && f.onlyInt) out.onlyInt = true;
  if ((f.type === "number" || f.type === "text" || f.type === "email" || f.type === "url") && f.min !== undefined && !Number.isNaN(f.min)) out.min = f.min;
  if ((f.type === "number" || f.type === "text" || f.type === "email" || f.type === "url") && f.max !== undefined && !Number.isNaN(f.max)) out.max = f.max;
  if (f.type === "select") out.values = (f.values ?? []).map((v) => v.trim()).filter(Boolean);
  if (f.type === "relation") {
    out.collection = f.collection;
    if (f.onDelete && f.onDelete !== "restrict") out.onDelete = f.onDelete;
  }
  return out;
}

const num = (s: string) => (s.trim() === "" ? undefined : Number(s));

export default function Collections() {
  const params = useParams();
  const base = () => `/api/v1/apps/${enc(params.app!)}/collections`;
  const [list, { refetch }] = createResource(() => params.app, () => api<{ items: Collection[] }>("GET", base()));
  const [draft, setDraft] = createStore<Draft>(blank());
  const [selected, setSelected] = createSignal<string | null>(null);
  const [error, setError] = createSignal<ApiError | null>(null);
  const [saved, setSaved] = createSignal("");

  const select = (c: Collection) => {
    setSelected(c.name);
    setError(null);
    setSaved("");
    const copy: Collection = structuredClone({ ...c, rules: c.rules ?? {}, indexes: c.indexes ?? [] });
    setDraft({ isNew: false, c: copy, indexText: indexesToText(copy.indexes) });
  };

  const startNew = () => {
    setSelected(null);
    setError(null);
    setSaved("");
    setDraft(blank());
  };

  const save = async (e: SubmitEvent) => {
    e.preventDefault();
    setError(null);
    setSaved("");
    const d = unwrap(draft);
    const body = {
      name: d.c.name.trim(),
      fields: d.c.fields.map(cleanField),
      rules: Object.fromEntries(RULE_KEYS.map((k) => [k, d.c.rules[k] ?? null])),
      indexes: textToIndexes(d.indexText),
    };
    try {
      const c = d.isNew
        ? await api<Collection>("POST", base(), body)
        : await api<Collection>("PUT", `${base()}/${enc(body.name)}`, body);
      await refetch();
      select(c);
      setSaved(`Saved (schema version ${c.version}).`);
    } catch (err) {
      setError(err as ApiError);
    }
  };

  const remove = async () => {
    const name = draft.c.name;
    if (!confirm(`Delete collection "${name}" and all its records? This cannot be undone.`)) return;
    try {
      await api("DELETE", `${base()}/${enc(name)}`);
      await refetch();
      startNew();
    } catch (err) {
      setError(err as ApiError);
    }
  };

  const setField = (i: number, patch: Partial<Field>) => setDraft("c", "fields", i, patch);

  return (
    <section>
      <p class="crumbs">
        <a href="#/">Apps</a> / {params.app}
      </p>
      <div class="split">
        <aside>
          <h3>Collections</h3>
          <ErrorBox error={list.error as ApiError | undefined} />
          <ul class="nav">
            <For each={list()?.items}>
              {(c) => (
                <li classList={{ active: selected() === c.name }}>
                  <button class="link" onClick={() => select(c)}>
                    {c.name}
                  </button>
                  <a class="muted" href={`#/apps/${enc(params.app!)}/c/${enc(c.name)}`}>
                    records
                  </a>
                </li>
              )}
            </For>
          </ul>
          <button onClick={startNew}>New collection</button>
        </aside>

        <form class="editor" onSubmit={save}>
          <h3>{draft.isNew ? "New collection" : draft.c.name}</h3>
          <Show when={draft.isNew}>
            <label>
              Name
              <input required pattern="[a-z][a-z0-9_]{0,62}" value={draft.c.name} onInput={(e) => setDraft("c", "name", e.currentTarget.value)} />
            </label>
          </Show>

          <h4>Fields</h4>
          <p class="muted">System fields id, created, updated and _v are added automatically.</p>
          <table class="fields">
            <thead>
              <tr>
                <th>Name</th>
                <th>Type</th>
                <th>Req.</th>
                <th>Hidden</th>
                <th>Options</th>
                <th />
              </tr>
            </thead>
            <tbody>
              <For each={draft.c.fields}>
                {(f, i) => (
                  <tr>
                    <td>
                      <input required value={f.name} onInput={(e) => setField(i(), { name: e.currentTarget.value })} />
                    </td>
                    <td>
                      <select value={f.type} onChange={(e) => setField(i(), { type: e.currentTarget.value as Field["type"] })}>
                        <For each={FIELD_TYPES}>{(t) => <option value={t}>{t}</option>}</For>
                      </select>
                    </td>
                    <td>
                      <input type="checkbox" checked={!!f.required} onChange={(e) => setField(i(), { required: e.currentTarget.checked })} />
                    </td>
                    <td>
                      <input type="checkbox" checked={!!f.hidden} onChange={(e) => setField(i(), { hidden: e.currentTarget.checked })} />
                    </td>
                    <td class="opts">
                      <Show when={f.type === "select"}>
                        <input
                          placeholder="values, comma separated"
                          value={(f.values ?? []).join(",")}
                          onInput={(e) => setField(i(), { values: e.currentTarget.value.split(",") })}
                        />
                      </Show>
                      <Show when={f.type === "relation"}>
                        <select value={f.collection ?? ""} onChange={(e) => setField(i(), { collection: e.currentTarget.value })}>
                          <option value="" disabled>
                            target…
                          </option>
                          <For each={list()?.items}>{(c) => <option value={c.name}>{c.name}</option>}</For>
                        </select>
                        <select value={f.onDelete ?? "restrict"} onChange={(e) => setField(i(), { onDelete: e.currentTarget.value as Field["onDelete"] })}>
                          <option value="restrict">restrict</option>
                          <option value="cascade">cascade</option>
                          <option value="setNull">set null</option>
                        </select>
                      </Show>
                      <Show when={f.type === "number"}>
                        <label class="inline">
                          <input type="checkbox" checked={!!f.onlyInt} onChange={(e) => setField(i(), { onlyInt: e.currentTarget.checked })} /> int
                        </label>
                      </Show>
                      <Show when={["number", "text", "email", "url"].includes(f.type)}>
                        <input class="small" placeholder="min" value={f.min ?? ""} onInput={(e) => setField(i(), { min: num(e.currentTarget.value) })} />
                        <input class="small" placeholder="max" value={f.max ?? ""} onInput={(e) => setField(i(), { max: num(e.currentTarget.value) })} />
                      </Show>
                    </td>
                    <td>
                      <button type="button" class="ghost danger" onClick={() => setDraft("c", "fields", produce((fs) => fs.splice(i(), 1)))}>
                        Remove
                      </button>
                    </td>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
          <button type="button" onClick={() => setDraft("c", "fields", (fs) => [...fs, { name: "", type: "text" } as Field])}>
            Add field
          </button>

          <h4>Rules</h4>
          <p class="muted">Unchecked = superusers only. Empty = anyone. Otherwise a filter expression, e.g. owner = @request.auth.id</p>
          <For each={RULE_KEYS}>
            {(k: RuleKey) => (
              <div class="rule">
                <label class="inline">
                  <input
                    type="checkbox"
                    checked={draft.c.rules[k] != null}
                    onChange={(e) => setDraft("c", "rules", k, e.currentTarget.checked ? "" : null)}
                  />
                  {k}
                </label>
                <input
                  class="mono"
                  disabled={draft.c.rules[k] == null}
                  placeholder={draft.c.rules[k] == null ? "superusers only" : "anyone"}
                  value={draft.c.rules[k] ?? ""}
                  onInput={(e) => setDraft("c", "rules", k, e.currentTarget.value)}
                />
              </div>
            )}
          </For>

          <h4>Indexes</h4>
          <textarea
            class="mono"
            rows="3"
            placeholder={"one per line: title  |  unique email  |  owner,created"}
            value={draft.indexText}
            onInput={(e) => setDraft("indexText", e.currentTarget.value)}
          />

          <ErrorBox error={error()} />
          <Show when={saved()}>
            <p class="ok">{saved()}</p>
          </Show>
          <div class="row">
            <button type="submit">{draft.isNew ? "Create collection" : "Save changes"}</button>
            <Show when={!draft.isNew}>
              <button type="button" class="danger" onClick={remove}>
                Delete collection
              </button>
            </Show>
          </div>
        </form>
      </div>
    </section>
  );
}
