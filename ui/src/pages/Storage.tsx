import { createResource, createSignal, For, Show } from "solid-js";
import { useParams } from "@solidjs/router";
import { api, enc, ready, token, type ApiError, type Bucket, type StorageObject } from "../api";
import { fmtBytes } from "../charts";
import { Drawer, fmtDate, Icon, toast } from "../ui";
import ErrorBox from "./ErrorBox";

const PAGE = 100;
/** Object keys keep their slashes; every other character is escaped. */
const keyPath = (k: string) => k.split("/").map(enc).join("/");

/** Storage buckets of one app: create and delete buckets, browse, upload, download and delete objects. */
export default function Storage() {
  const params = useParams();
  const base = () => `/api/v1/apps/${enc(params.app!)}/buckets`;
  const [buckets, { refetch: refetchBuckets }] = createResource(base, (b) => api<{ items: Bucket[] }>("GET", b));
  const [error, setError] = createSignal<ApiError | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [selected, setSelected] = createSignal<string | null>(null);
  const [prefix, setPrefix] = createSignal("");
  const [extra, setExtra] = createSignal<StorageObject[]>([]);
  const [cursor, setCursor] = createSignal<string | null>(null);
  const [creating, setCreating] = createSignal(false);
  const [f, setF] = createSignal({ name: "", public: false, maxMb: "100", quotaMb: "", mime: "", thumbs: "" });

  const q = () => (selected() ? { b: selected()!, p: prefix() } : null);
  const [page, { refetch: refetchObjects }] = createResource(q, async (k) => {
    setExtra([]);
    const r = await api<{ items: StorageObject[]; nextCursor: string | null }>(
      "GET",
      `${base()}/${enc(k.b)}/objects?limit=${PAGE}${k.p ? `&prefix=${enc(k.p)}` : ""}`,
    );
    setCursor(r.nextCursor);
    return r.items;
  });
  const objects = () => [...(ready(page) ?? []), ...extra()];
  const bucket = () => ready(buckets)?.items.find((b) => b.name === selected());

  const run = async (fn: () => Promise<unknown>, done?: string) => {
    setError(null);
    setBusy(true);
    try {
      await fn();
      if (done) toast(done);
    } catch (e) {
      setError(e as ApiError);
    } finally {
      setBusy(false);
    }
  };

  const more = () =>
    run(async () => {
      const r = await api<{ items: StorageObject[]; nextCursor: string | null }>(
        "GET",
        `${base()}/${enc(selected()!)}/objects?limit=${PAGE}&cursor=${enc(cursor()!)}${prefix() ? `&prefix=${enc(prefix())}` : ""}`,
      );
      setExtra([...extra(), ...r.items]);
      setCursor(r.nextCursor);
    });

  const create = async (e: SubmitEvent) => {
    e.preventDefault();
    const v = f();
    await run(async () => {
      await api("POST", base(), {
        name: v.name,
        public: v.public,
        maxSize: Math.round(Number(v.maxMb) * 1024 * 1024),
        ...(v.quotaMb ? { maxBytes: Math.round(Number(v.quotaMb) * 1024 * 1024) } : {}),
        allowedMime: v.mime.split(",").map((s) => s.trim().toLowerCase()).filter(Boolean),
        thumbSizes: v.thumbs.split(",").map((s) => s.trim()).filter(Boolean),
      });
      setCreating(false);
      setSelected(v.name);
      refetchBuckets();
    }, "Bucket created");
  };

  const removeBucket = (b: Bucket) => {
    const msg = b.objects ? `Delete bucket "${b.name}" and its ${b.objects} objects? This cannot be undone.` : `Delete bucket "${b.name}"?`;
    if (!confirm(msg)) return;
    run(async () => {
      await api("DELETE", `${base()}/${enc(b.name)}${b.objects ? "?force=true" : ""}`);
      if (selected() === b.name) setSelected(null);
      refetchBuckets();
    }, "Bucket deleted");
  };

  const upload = (files: FileList | null) => {
    if (!files?.length) return;
    run(async () => {
      for (const file of Array.from(files)) {
        const key = `${prefix()}${file.name}`;
        const res = await fetch(`${base()}/${enc(selected()!)}/objects/${keyPath(key)}`, {
          method: "PUT",
          headers: { authorization: `Bearer ${token()}`, "content-type": file.type || "application/octet-stream" },
          body: file,
        });
        if (!res.ok) throw { problem: (await res.json().catch(() => null)) ?? { type: "http", title: `HTTP ${res.status}`, status: res.status } };
      }
      refetchObjects();
      refetchBuckets();
    }, "Uploaded");
  };

  // shortcut: the object is buffered in the browser (the request needs the auth header, so a plain link will not do).
  const download = (o: StorageObject) =>
    run(async () => {
      const res = await fetch(`${base()}/${enc(selected()!)}/objects/${keyPath(o.key)}`, { headers: { authorization: `Bearer ${token()}` } });
      if (!res.ok) throw { problem: (await res.json().catch(() => null)) ?? { type: "http", title: `HTTP ${res.status}`, status: res.status } };
      const url = URL.createObjectURL(await res.blob());
      Object.assign(document.createElement("a"), { href: url, download: o.key.split("/").pop() }).click();
      URL.revokeObjectURL(url);
    });

  const removeObject = (o: StorageObject) => {
    if (!confirm(`Delete ${o.key}?`)) return;
    run(async () => {
      await api("DELETE", `${base()}/${enc(selected()!)}/objects/${keyPath(o.key)}`);
      refetchObjects();
      refetchBuckets();
    }, "Object deleted");
  };

  return (
    <div class="page">
      <header class="page-head">
        <div class="min0">
          <p class="crumbs">
            <a href={`#/apps/${enc(params.app!)}`}>{params.app}</a>
          </p>
          <h1>Storage</h1>
          <p class="muted">Buckets of files for this app. Upload with <code>PUT {base()}/&lt;bucket&gt;/objects/&lt;key&gt;</code> and a superuser token or an API key with <code>files:write</code>.</p>
        </div>
        <button class="btn" onClick={() => { setF({ name: "", public: false, maxMb: "100", quotaMb: "", mime: "", thumbs: "" }); setError(null); setCreating(true); }}>
          <Icon name="storage" />
          New bucket
        </button>
      </header>
      <ErrorBox error={creating() ? null : (error() ?? (buckets.error as ApiError | undefined))} />
      <p class="notice">
        <Icon name="alert" size={16} />
        <span>
          Local disk only. Identical files are stored once, and image thumbnails of the sizes you declare are made on demand. Public buckets let anyone who knows a key read it; listing and writes always need a credential. Per-user access rules and S3 are planned.
        </span>
      </p>
      <div class="table-wrap">
        <table class="grid">
          <thead>
            <tr>
              <th scope="col">Bucket</th>
              <th scope="col">Access</th>
              <th scope="col" class="num">Objects</th>
              <th scope="col" class="num">Size / quota</th>
              <th scope="col" class="num">Max object</th>
              <th scope="col"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            <For each={ready(buckets)?.items ?? []} fallback={<tr><td colspan="6" class="empty-row">{buckets.loading ? "Loading…" : "No buckets yet."}</td></tr>}>
              {(b) => (
                <tr classList={{ active: selected() === b.name }}>
                  <td>
                    <button class="link mono" onClick={() => { setPrefix(""); setSelected(b.name); }}>{b.name}</button>
                  </td>
                  <td class="small">{b.public ? "Public read" : "Private"}</td>
                  <td class="num">{b.objects}</td>
                  <td class="num">{fmtBytes(b.bytes)}{b.maxBytes ? ` / ${fmtBytes(b.maxBytes)}` : ""}</td>
                  <td class="num">{fmtBytes(b.maxSize)}</td>
                  <td class="num">
                    <button class="icon-btn danger" aria-label={`Delete bucket ${b.name}`} title="Delete bucket" disabled={busy()} onClick={() => removeBucket(b)}>
                      <Icon name="trash" />
                    </button>
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>

      <Show when={bucket()}>
        {(b) => (
          <section aria-label={`Objects in ${b().name}`}>
            <header class="page-head">
              <h2 class="mono">{b().name}</h2>
              <div class="row">
                <input aria-label="Key prefix" class="mono" placeholder="Key prefix, e.g. photos/" value={prefix()} onChange={(e) => setPrefix(e.currentTarget.value)} />
                <label class="btn" classList={{ disabled: busy() }}>
                  <Icon name="archive" />
                  Upload
                  <input type="file" multiple hidden disabled={busy()} onChange={(e) => { upload(e.currentTarget.files); e.currentTarget.value = ""; }} />
                </label>
              </div>
            </header>
            <p class="muted small">Uploads are named after the file and placed under the prefix.{b().allowedMime.length ? ` Allowed types: ${b().allowedMime.join(", ")}.` : ""}</p>
            <div class="table-wrap">
              <table class="grid">
                <thead>
                  <tr>
                    <th scope="col">Key</th>
                    <th scope="col">Type</th>
                    <th scope="col" class="num">Size</th>
                    <th scope="col">Updated</th>
                    <th scope="col"><span class="sr-only">Actions</span></th>
                  </tr>
                </thead>
                <tbody>
                  <For each={objects()} fallback={<tr><td colspan="5" class="empty-row">{page.loading ? "Loading…" : "No objects."}</td></tr>}>
                    {(o) => (
                      <tr>
                        <td class="mono small">{o.key}</td>
                        <td class="small">{o.mime}</td>
                        <td class="num">{fmtBytes(o.size)}</td>
                        <td class="small">{fmtDate(o.updated)}</td>
                        <td class="num nowrap">
                          <button class="btn secondary sm" disabled={busy()} onClick={() => download(o)}>
                            Download
                          </button>{" "}
                          <button class="icon-btn danger" aria-label={`Delete ${o.key}`} title="Delete" disabled={busy()} onClick={() => removeObject(o)}>
                            <Icon name="trash" />
                          </button>
                        </td>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
            </div>
            <Show when={cursor()}>
              <p>
                <button class="btn secondary" disabled={busy()} onClick={more}>
                  Load more
                </button>
              </p>
            </Show>
          </section>
        )}
      </Show>

      <Show when={creating()}>
        <Drawer
          title="New bucket"
          onClose={() => setCreating(false)}
          onSubmit={create}
          footer={
            <>
              <button type="button" class="btn secondary" onClick={() => setCreating(false)}>
                Cancel
              </button>
              <button class="btn" type="submit" disabled={busy()}>
                Create
              </button>
            </>
          }
        >
          <ErrorBox error={error()} />
          <div class="field">
            <label for="bname">Name</label>
            <input id="bname" class="mono" required pattern="[a-z][a-z0-9_]{0,62}" value={f().name} onInput={(e) => setF({ ...f(), name: e.currentTarget.value })} />
            <span class="help">Lowercase letters, digits and _, starting with a letter.</span>
          </div>
          <div class="field">
            <label for="bmax">Largest object (MB)</label>
            <input id="bmax" type="number" min="1" max="10240" required value={f().maxMb} onInput={(e) => setF({ ...f(), maxMb: e.currentTarget.value })} />
          </div>
          <div class="field">
            <label for="bquota">Bucket quota (MB)</label>
            <input id="bquota" type="number" min="1" value={f().quotaMb} onInput={(e) => setF({ ...f(), quotaMb: e.currentTarget.value })} />
            <span class="help">Total size the bucket may hold, counted per object (identical files still count each time). Leave empty for no limit.</span>
          </div>
          <div class="field">
            <label for="bmime">Allowed types</label>
            <input id="bmime" class="mono" placeholder="image/*, application/pdf" value={f().mime} onInput={(e) => setF({ ...f(), mime: e.currentTarget.value })} />
            <span class="help">Comma separated. Leave empty to allow every type. The type is detected from the file contents when it can be, so a renamed file is judged by what it really is.</span>
          </div>
          <div class="field">
            <label for="bthumbs">Thumbnail sizes</label>
            <input id="bthumbs" class="mono" placeholder="100x100, 0x300, 400x400f" value={f().thumbs} onInput={(e) => setF({ ...f(), thumbs: e.currentTarget.value })} />
            <span class="help">Request one with <code>?thumb=100x100</code> on a png, jpeg, gif or webp object. <code>WxH</code> crops to fill, <code>Wx0</code> / <code>0xH</code> keep the aspect ratio, <code>WxHf</code> fits inside. Only listed sizes are made. Leave empty for none.</span>
          </div>
          <div class="field">
            <label>
              <input type="checkbox" checked={f().public} onChange={(e) => setF({ ...f(), public: e.currentTarget.checked })} /> Public read
            </label>
            <span class="help">Anyone with an object's URL can read it. Listing and writes still need a credential.</span>
          </div>
        </Drawer>
      </Show>
    </div>
  );
}
