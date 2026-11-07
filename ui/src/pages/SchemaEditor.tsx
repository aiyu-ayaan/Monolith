import { createSignal, For, Show } from "solid-js";
import { createStore, produce, unwrap } from "solid-js/store";
import { api, enc, FIELD_TYPES, RULE_KEYS, type ApiError, type Collection, type Field, type RuleKey } from "../api";
import { Drawer, Icon } from "../ui";
import ErrorBox from "./ErrorBox";

interface Draft {
  c: Collection;
  indexText: string;
}

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

const SIZED = ["number", "text", "email", "url"];

/** Drops empty optional keys so the server's strict schema accepts the field. */
function cleanField(f: Field): Field {
  const out: Field = { name: f.name.trim(), type: f.type };
  if (f.required) out.required = true;
  if (f.hidden) out.hidden = true;
  if (f.type === "number" && f.onlyInt) out.onlyInt = true;
  if (SIZED.includes(f.type) && f.min !== undefined && !Number.isNaN(f.min)) out.min = f.min;
  if (SIZED.includes(f.type) && f.max !== undefined && !Number.isNaN(f.max)) out.max = f.max;
  if (f.type === "select") out.values = (f.values ?? []).map((v) => v.trim()).filter(Boolean);
  if (f.type === "relation") {
    out.collection = f.collection;
    if (f.onDelete && f.onDelete !== "restrict") out.onDelete = f.onDelete;
    if (f.maxSelect !== undefined && !Number.isNaN(f.maxSelect) && f.maxSelect > 1) out.maxSelect = Math.trunc(f.maxSelect);
  }
  return out;
}

const num = (s: string) => (s.trim() === "" ? undefined : Number(s));

export type Tab = "fields" | "rules" | "indexes";

export default function SchemaEditor(p: {
  app: string;
  initial: Collection | null;
  tab?: Tab;
  collections: Collection[];
  onClose: () => void;
  onSaved: (c: Collection) => void;
  onDeleted: (name: string) => void;
}) {
  const isNew = !p.initial;
  const start: Collection = p.initial
    ? structuredClone({ ...p.initial, rules: p.initial.rules ?? {}, indexes: p.initial.indexes ?? [] })
    : { name: "", fields: [], rules: {}, indexes: [] };
  const [draft, setDraft] = createStore<Draft>({ c: start, indexText: indexesToText(start.indexes) });
  const [tab, setTab] = createSignal<Tab>(p.tab ?? "fields");
  const [error, setError] = createSignal<ApiError | null>(null);
  const [busy, setBusy] = createSignal(false);
  const base = `/api/v1/apps/${enc(p.app)}/collections`;

  const save = async (e: SubmitEvent) => {
    e.preventDefault();
    setError(null);
    setBusy(true);
    const d = unwrap(draft);
    const body = {
      name: d.c.name.trim(),
      fields: d.c.fields.map(cleanField),
      rules: Object.fromEntries(RULE_KEYS.map((k) => [k, d.c.rules[k] ?? null])),
      indexes: textToIndexes(d.indexText),
    };
    try {
      const c = isNew ? await api<Collection>("POST", base, body) : await api<Collection>("PUT", `${base}/${enc(body.name)}`, body);
      p.onSaved(c);
    } catch (err) {
      setError(err as ApiError);
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    const name = draft.c.name;
    if (!confirm(`Delete collection "${name}" and all its records? This cannot be undone.`)) return;
    try {
      await api("DELETE", `${base}/${enc(name)}`);
      p.onDeleted(name);
    } catch (err) {
      setError(err as ApiError);
    }
  };

  const setField = (i: number, patch: Partial<Field>) => setDraft("c", "fields", i, patch);
  const tabBtn = (t: Tab, label: string) => (
    <button
      type="button"
      role="tab"
      id={`tab-${t}`}
      aria-controls={`panel-${t}`}
      aria-selected={tab() === t}
      classList={{ tab: true, active: tab() === t }}
      onClick={() => setTab(t)}
    >
      {label}
    </button>
  );

  return (
    <Drawer
      wide
      title={isNew ? "New collection" : `Edit ${draft.c.name}`}
      onClose={p.onClose}
      onSubmit={save}
      footer={
        <>
          <Show when={!isNew}>
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
            {busy() ? "Saving…" : isNew ? "Create" : "Save changes"}
          </button>
        </>
      }
    >
      <div class="field">
        <label for="coll-name">Name</label>
        <input
          id="coll-name"
          class="mono"
          required
          disabled={!isNew}
          pattern="[a-z][a-z0-9_]{0,62}"
          placeholder="posts"
          value={draft.c.name}
          onInput={(e) => setDraft("c", "name", e.currentTarget.value)}
        />
        <Show when={!isNew}>
          <p class="help">Renaming is not supported yet.</p>
        </Show>
      </div>

      <div class="tabs" role="tablist" aria-label="Collection settings">
        {tabBtn("fields", "Fields")}
        {tabBtn("rules", "API rules")}
        {tabBtn("indexes", "Indexes")}
      </div>

      <Show when={tab() === "fields"}>
        <div role="tabpanel" id="panel-fields" aria-labelledby="tab-fields" class="stack">
          <div class="chips" aria-label="System fields">
            <For each={["id", "created", "updated", "_v"]}>
              {(n) => (
                <span class="chip">
                  <Icon name="lock" size={12} />
                  {n}
                </span>
              )}
            </For>
          </div>
          <For each={draft.c.fields} fallback={<p class="muted small">No fields yet. System fields are added automatically.</p>}>
            {(f, i) => (
              <div class="field-card">
                <div class="field-main">
                  <input
                    aria-label={`Field ${i() + 1} name`}
                    class="mono"
                    required
                    placeholder="field_name"
                    value={f.name}
                    onInput={(e) => setField(i(), { name: e.currentTarget.value })}
                  />
                  <select aria-label={`Field ${i() + 1} type`} value={f.type} onChange={(e) => setField(i(), { type: e.currentTarget.value as Field["type"] })}>
                    <For each={FIELD_TYPES}>{(t) => <option value={t}>{t}</option>}</For>
                  </select>
                  <button
                    type="button"
                    class="icon-btn danger"
                    aria-label={`Remove field ${f.name || i() + 1}`}
                    onClick={() => setDraft("c", "fields", produce((fs) => fs.splice(i(), 1)))}
                  >
                    <Icon name="trash" />
                  </button>
                </div>
                <div class="field-opts">
                  <label class="check">
                    <input type="checkbox" checked={!!f.required} onChange={(e) => setField(i(), { required: e.currentTarget.checked })} />
                    Required
                  </label>
                  <label class="check">
                    <input type="checkbox" checked={!!f.hidden} onChange={(e) => setField(i(), { hidden: e.currentTarget.checked })} />
                    Hidden
                  </label>
                  <Show when={f.type === "number"}>
                    <label class="check">
                      <input type="checkbox" checked={!!f.onlyInt} onChange={(e) => setField(i(), { onlyInt: e.currentTarget.checked })} />
                      Integer
                    </label>
                  </Show>
                  <Show when={SIZED.includes(f.type)}>
                    <input class="small" aria-label="Min" placeholder="min" inputmode="numeric" value={f.min ?? ""} onInput={(e) => setField(i(), { min: num(e.currentTarget.value) })} />
                    <input class="small" aria-label="Max" placeholder="max" inputmode="numeric" value={f.max ?? ""} onInput={(e) => setField(i(), { max: num(e.currentTarget.value) })} />
                  </Show>
                  <Show when={f.type === "select"}>
                    <input
                      class="grow"
                      aria-label="Select values"
                      placeholder="values, comma separated"
                      value={(f.values ?? []).join(",")}
                      onInput={(e) => setField(i(), { values: e.currentTarget.value.split(",") })}
                    />
                  </Show>
                  <Show when={f.type === "relation"}>
                    <select aria-label="Target collection" value={f.collection ?? ""} onChange={(e) => setField(i(), { collection: e.currentTarget.value })}>
                      <option value="" disabled>
                        target…
                      </option>
                      <For each={p.collections}>{(c) => <option value={c.name}>{c.name}</option>}</For>
                    </select>
                    <input
                      class="small"
                      aria-label="Max select"
                      placeholder="maxSelect (1)"
                      inputmode="numeric"
                      value={f.maxSelect ?? ""}
                      onInput={(e) => setField(i(), { maxSelect: num(e.currentTarget.value) })}
                    />
                    <select aria-label="On delete" value={f.onDelete ?? "restrict"} onChange={(e) => setField(i(), { onDelete: e.currentTarget.value as Field["onDelete"] })}>
                      <option value="restrict">on delete: restrict</option>
                      <option value="cascade">on delete: cascade</option>
                      <option value="setNull" disabled={(f.maxSelect ?? 1) > 1}>
                        on delete: set null
                      </option>
                    </select>
                  </Show>
                </div>
              </div>
            )}
          </For>
          <button type="button" class="btn secondary block" onClick={() => setDraft("c", "fields", (fs) => [...fs, { name: "", type: "text" } as Field])}>
            <Icon name="plus" />
            Add field
          </button>
        </div>
      </Show>

      <Show when={tab() === "rules"}>
        <div role="tabpanel" id="panel-rules" aria-labelledby="tab-rules" class="stack">
          <p class="help">
            Locked = superusers only. Empty = anyone. Otherwise a filter, e.g. <code>owner = @request.auth.id</code>
          </p>
          <For each={RULE_KEYS}>
            {(k: RuleKey) => {
              const locked = () => draft.c.rules[k] == null;
              return (
                <div class="rule">
                  <div class="rule-head">
                    <label for={`rule-${k}`} class="rule-name">
                      {k} rule
                    </label>
                    <label class="check">
                      <input type="checkbox" checked={locked()} onChange={(e) => setDraft("c", "rules", k, e.currentTarget.checked ? null : "")} />
                      <Icon name="lock" size={14} />
                      Superusers only
                    </label>
                  </div>
                  <input
                    id={`rule-${k}`}
                    class="mono"
                    disabled={locked()}
                    placeholder={locked() ? `Only superusers can ${k}` : "Empty = anyone"}
                    value={draft.c.rules[k] ?? ""}
                    onInput={(e) => setDraft("c", "rules", k, e.currentTarget.value)}
                  />
                </div>
              );
            }}
          </For>
        </div>
      </Show>

      <Show when={tab() === "indexes"}>
        <div role="tabpanel" id="panel-indexes" aria-labelledby="tab-indexes" class="field">
          <label for="coll-indexes">Indexes</label>
          <textarea
            id="coll-indexes"
            class="mono"
            rows="6"
            aria-describedby="coll-indexes-help"
            placeholder={"title\nunique email\nowner,created"}
            value={draft.indexText}
            onInput={(e) => setDraft("indexText", e.currentTarget.value)}
          />
          <p id="coll-indexes-help" class="help">
            One per line: comma-separated fields, prefix <code>unique </code> for a unique index.
          </p>
        </div>
      </Show>

      <ErrorBox error={error()} />
    </Drawer>
  );
}
