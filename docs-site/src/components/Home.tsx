import { createSignal, For } from "solid-js";
import { A } from "@solidjs/router";
import { AREAS, firstPage, PAGES } from "../lib/content";

const FEATURES = [
  { t: "Database and rules", d: "Typed collections with relations, keyset paging and access rules written next to the data.", to: "/build/database/overview" },
  { t: "Authentication", d: "Email and password plus OAuth and OIDC providers, configured per app.", to: "/build/auth/overview" },
  { t: "Realtime", d: "Live queries over Server-Sent Events, filtered per collection, with resume after a disconnect.", to: "/build/database/realtime" },
  { t: "File storage", d: "Buckets on local disk or S3, stored once per content, with on-demand thumbnails.", to: "/build/storage/overview" },
  { t: "API keys", d: "Scoped keys for servers and scripts, stored only as hashes.", to: "/build/auth/api-keys" },
  { t: "Webhooks", d: "Signed calls to your server when records change, retried with backoff.", to: "/build/extend/webhooks" },
  { t: "WASM plugins", d: "Sandboxed handlers that run after a commit, with fuel and memory limits.", to: "/build/extend/plugins" },
  { t: "Backups", d: "Consistent copies of an app taken while it keeps serving, restorable into a new app.", to: "/build/operate/backups" },
];

const SHOWCASE = [
  {
    img: "records", title: "Browse and edit your data",
    text: "The admin console shows every collection as a table. Filter with the same expressions your app uses, sort, and edit records in place.",
    points: ["Typed fields: text, number, bool, dates, relations", "Filter and sort with the rules syntax", "Cursor paging that costs the same on page 1 and page 1,000"],
    to: "/build/database/query", cta: "Query records",
  },
  {
    img: "rules", title: "Decide who can do what",
    text: "Every collection has five rules: list, view, create, update and delete. A rule is a filter expression, public, or locked to superusers.",
    points: ["Compiled once per schema version", "Check a rule with the explain endpoint", "Rules can use the caller, the request body and date macros"],
    to: "/build/database/rules", cta: "Read about rules",
  },
  {
    img: "auth", title: "Sign users in with the methods you pick",
    text: "Turn on email and password and connect as many providers as you need. Sign-in screens in the SDKs read this configuration, so a new provider appears on its own.",
    points: ["Short access tokens, rotating refresh tokens", "Allowed redirect URLs per app", "Ready-made screens for web, Compose and Flutter"],
    to: "/build/auth/overview", cta: "Set up sign-in",
  },
  {
    img: "storage", title: "Keep files next to your data",
    text: "Create buckets, set size and type limits and serve images at the sizes you declare. Identical files are stored once.",
    points: ["Public or private buckets", "Range requests and ETag revalidation", "Local disk or any S3-compatible store"],
    to: "/build/storage/overview", cta: "Use storage",
  },
];

const PLATFORMS = [
  { t: "Web", d: "TypeScript and React", to: "/get-started/quickstart-web" },
  { t: "Android", d: "Kotlin and Compose", to: "/get-started/quickstart-android" },
  { t: "Flutter", d: "Dart and widgets", to: "/get-started/quickstart-flutter" },
  { t: "Servers and scripts", d: "curl, Python and Rust", to: "/get-started/quickstart-server" },
];

const CODE: Record<string, string> = {
  TypeScript: `import { Monolith } from "@monolith/client";

const mono = Monolith.init({ url: "https://api.example.com", app: "shop" });
await mono.auth.signIn({ email: "ada@example.com", password: "correct horse" });

const products = mono.collection("products");
const page = await products.list({ filter: "in_stock = true", sort: "-created", limit: 20 });

products.onSnapshot((snap) => render(snap.docs), { filter: "in_stock = true" });`,
  Kotlin: `val mono = Monolith.init(context, url = "https://api.example.com", app = "shop")
mono.auth.signIn("ada@example.com", "correct horse")

val products = mono.collection<Product>("products")
val page = products.list(ListOptions(filter = "in_stock = true", sort = "-created", limit = 20))

products.onSnapshot(SnapshotOptions(filter = "in_stock = true"))
    .collect { snap -> adapter.submitList(snap.docs) }`,
  Dart: `final mono = await Monolith.init(url: 'https://api.example.com', app: 'shop');
await mono.auth().signIn('ada@example.com', 'correct horse');

final products = mono.collection<RecordModel>('products');
final page = await products.list(filter: 'in_stock = true', sort: '-created', limit: 20);

products.onSnapshot(SnapshotOptions(filter: 'in_stock = true'))
    .listen((snap) => setState(() => items = snap.docs));`,
  Python: `import monolith

mono = monolith.init(url="https://api.example.com", app="shop")
mono.auth().sign_in("ada@example.com", "correct horse")

products = mono.collection("products")
page = products.list(filter="in_stock = true", sort="-created", limit=20)

products.on_snapshot(lambda snap: print(len(snap.docs)), filter="in_stock = true")`,
  Rust: `let mono = Monolith::init("https://api.example.com", "shop")?;
mono.auth().sign_in("ada@example.com", "correct horse").await?;

let products = mono.records("products");
let opts = ListOptions::new().filter("in_stock = true").sort("-created").limit(20);
let page = products.list(Some(&opts)).await?;`,
};

function Shot(p: { name: string; alt: string }) {
  return (
    <figure class="shot">
      <div class="shot-bar" aria-hidden="true"><i /><i /><i /></div>
      <img src={`/screens/${p.name}.png`} alt={p.alt} width="1440" height="860" />
    </figure>
  );
}

export default function Home() {
  const [lang, setLang] = createSignal("TypeScript");
  const count = (id: string) => PAGES.filter((x) => x.area === id).length;
  return (
    <main class="home">
      <section class="hero">
        <p class="eyebrow">Self-hosted backend suite</p>
        <h1>Your backend in one container.</h1>
        <p class="lead">
          A database with access rules, sign-in, realtime, files, webhooks, plugins and backups for every app you run, from one server and one data directory.
        </p>
        <div class="cta">
          <A class="btn primary" href="/get-started/quickstart-server">Run your first request</A>
          <A class="btn" href="/get-started/intro">What is Monolith</A>
          <A class="btn ghost" href="/sdks/overview">Browse the SDKs</A>
        </div>
        <ul class="facts">
          <li>REST with an OpenAPI 3.1 contract</li>
          <li>SQLite, one file per app</li>
          <li>SDKs for TypeScript, Kotlin, Dart, Python, Rust</li>
        </ul>
        <Shot name="dashboard" alt="The Monolith admin console showing the overview of an app" />
        <p class="caption">The admin console, served by the same binary at <code>/_/</code>.</p>
      </section>

      <section class="band">
        <h2>Everything one app needs</h2>
        <p class="sub">Each piece works over plain HTTP. The SDKs are a convenience.</p>
        <div class="features">
          <For each={FEATURES}>
            {(f) => (
              <A class="feature" href={f.to}>
                <h3>{f.t}</h3>
                <p>{f.d}</p>
              </A>
            )}
          </For>
        </div>
      </section>

      <For each={SHOWCASE}>
        {(s, i) => (
          <section class="showcase" classList={{ flip: i() % 2 === 1 }}>
            <div class="show-text">
              <h2>{s.title}</h2>
              <p>{s.text}</p>
              <ul>
                <For each={s.points}>{(x) => <li>{x}</li>}</For>
              </ul>
              <A class="more" href={s.to}>{s.cta} →</A>
            </div>
            <Shot name={s.img} alt={s.title} />
          </section>
        )}
      </For>

      <section class="band">
        <h2>From zero to a live query</h2>
        <p class="sub">The same few lines in every SDK. Pick your language.</p>
        <div class="codebox">
          <div class="tabs-row" role="tablist">
            <For each={Object.keys(CODE)}>
              {(l) => (
                <button type="button" role="tab" aria-selected={lang() === l} classList={{ on: lang() === l }} onClick={() => setLang(l)}>
                  {l}
                </button>
              )}
            </For>
          </div>
          <pre><code>{CODE[lang()]}</code></pre>
        </div>
        <p class="caption">
          Full reference: <A href="/sdks/setup">Set up</A>, <A href="/sdks/query">Read and query</A>, <A href="/sdks/realtime">Realtime</A>.
        </p>
      </section>

      <section class="band">
        <h2>Start with your platform</h2>
        <div class="features four">
          <For each={PLATFORMS}>
            {(f) => (
              <A class="feature" href={f.to}>
                <h3>{f.t}</h3>
                <p>{f.d}</p>
              </A>
            )}
          </For>
        </div>
      </section>

      <section class="band status">
        <div>
          <h2>Where it stands</h2>
          <p>
            The core data plane is nearly finished: apps, collections, rules, realtime, sign-in, storage, API keys, webhooks, plugins and backups work in a first version.
            Offline sync, WebSocket and a published Docker image are not there yet, and the docs say so on every page that is affected.
          </p>
        </div>
        <A class="btn" href="/get-started/intro#what-works-today">See what works today</A>
      </section>

      <section class="band">
        <h2>Browse the documentation</h2>
        <div class="cards">
          <For each={AREAS}>
            {(a) => (
              <A class="card" href={`/${firstPage(a.id)?.path ?? a.id}`}>
                <h2>{a.title}</h2>
                <p>{{ "get-started": "What Monolith is, the concepts, and your first app in a few minutes.", build: "Database, authentication, storage, extending the server and running it.", sdks: "One page per topic, with the code in every language.", reference: "Exact APIs, protocols, limits and configuration." }[a.id]}</p>
                <small>{count(a.id)} pages</small>
              </A>
            )}
          </For>
        </div>
      </section>
    </main>
  );
}
