---
title: Read and query
description: List, filter, sort, page and count records in any SDK.
group: Use the SDKs
order: 30
---

A list call returns one page of records and a cursor for the next. Every record has `id`, `created`, `updated` and `_v` next to your own fields. Pick your language in any code block: the choice is remembered on every page.

## Get a collection

:::tabs
```ts title="TypeScript"
type Task = { title: string; done: boolean };
const tasks = mono.collection<Task>("tasks");   // typed; omit <Task> for RecordModel
```
```kotlin title="Kotlin"
@Serializable
data class Task(val id: String = "", val title: String, val done: Boolean = false)

val tasks = mono.collection<Task>("tasks")   // typed with kotlinx.serialization
val raw = mono.collection("tasks")           // RecordModel: id, created, updated, _v, data
```
```dart title="Dart"
final tasks = mono.collection<RecordModel>('tasks');                 // RecordModel: id, created, updated, v, data
final typed = mono.collection<Task>('tasks', Task.fromJson);         // your own decoder
```
```python title="Python"
tasks = mono.collection("tasks")   # records are plain dicts
```
```rust title="Rust"
use monolith_client::{ListOptions, Record};

let tasks = mono.records("tasks");                 // Collection<Record>: id, created, updated, version, fields
let typed = mono.collection::<Task>("tasks");      // your own serde type
```
:::

Getting a collection handle is cached and makes no network request. Typed models are optional. Without one you get a generic record: a `dict` in Python, `RecordModel` in Kotlin and Dart, `Record` in Rust.

## List records

:::tabs
```ts title="TypeScript"
const page = await tasks.list({ limit: 20 });
for (const t of page.items) console.log(t.id, t.title);
```
```kotlin title="Kotlin"
val page = tasks.list(ListOptions(limit = 20))
page.items.forEach { println("${it.id} ${it.title}") }
```
```dart title="Dart"
final page = await tasks.list(limit: 20);
for (final t in page.items) {
  print('${t.id} ${t['title']}');
}
```
```python title="Python"
page = tasks.list(limit=20)
for t in page.items:
    print(t["id"], t["title"])
```
```rust title="Rust"
let page = tasks.list(None).await?;
for t in &page.items {
    println!("{} {:?}", t.id, t.fields.get("title"));
}
```
:::

## Filter and sort

:::tabs
```ts title="TypeScript"
const open = await tasks.list({
  filter: "done = false && created > '2026-01-01'",
  sort: "-created,title",
});
```
```kotlin title="Kotlin"
val open = tasks.list(
    ListOptions(filter = "done = false && created > '2026-01-01'", sort = "-created,title")
)
```
```dart title="Dart"
final open = await tasks.list(
  filter: "done = false && created > '2026-01-01'",
  sort: '-created,title',
);
```
```python title="Python"
open_tasks = tasks.list(filter="done = false && created > '2026-01-01'", sort="-created,title")
```
```rust title="Rust"
let opts = ListOptions::new()
    .filter("done = false && created > '2026-01-01'")
    .sort("-created,title");
let open = tasks.list(Some(&opts)).await?;
```
:::

`filter` uses the [rules syntax](/reference/rules-syntax). Put user input inside quotes you control and strip quotes from the value, or a user can change the meaning of your filter. `sort` takes comma-separated fields, `-` for descending.

## Page through results

:::tabs
```ts title="TypeScript"
let cursor: string | undefined;
do {
  const page = await tasks.list({ filter: "done = false", sort: "-created", limit: 100, cursor });
  render(page.items);
  cursor = page.nextCursor ?? (page.next as string | undefined);
} while (cursor);
```
```kotlin title="Kotlin"
var cursor: String? = null
do {
    val page = tasks.list(ListOptions(filter = "done = false", sort = "-created", limit = 100, cursor = cursor))
    adapter.add(page.items)
    cursor = page.nextCursor
} while (cursor != null)
```
```dart title="Dart"
String? cursor;
do {
  final page = await tasks.list(filter: 'done = false', sort: '-created', limit: 100, cursor: cursor);
  items.addAll(page.items);
  cursor = page.nextCursor;
} while (cursor != null);
```
```python title="Python"
cursor = None
while True:
    page = tasks.list(filter="done = false", sort="-created", limit=100, cursor=cursor)
    handle(page.items)
    cursor = page.next_cursor
    if not cursor:
        break
```
```rust title="Rust"
let mut cursor: Option<String> = None;
loop {
    let mut opts = ListOptions::new().filter("done = false").sort("-created").limit(100);
    if let Some(c) = &cursor { opts = opts.cursor(c.clone()); }
    let page = tasks.list(Some(&opts)).await?;
    handle(&page.items);
    cursor = page.next;
    if cursor.is_none() { break; }
}
```
:::

Keep passing the cursor back until it is empty. A cursor is only valid with the same `filter` and `sort` that made it. Do not build page numbers and do not assume a total.

:::note
The cursor field is `nextCursor` in Kotlin and Dart, `next_cursor` in Python and `next` in Rust. TypeScript's type says `nextCursor`, but the server sends `next`: the sample reads both.
:::

## Choose fields and expand relations

:::tabs
```ts title="TypeScript"
const rows = await tasks.list({ fields: "id,title", expand: "owner" });
const one = await tasks.getOne(id, { expand: "owner,project.org", fields: "id,title,owner" });
```
```kotlin title="Kotlin"
val rows = tasks.list(ListOptions(fields = "id,title", expand = "owner"))
val one = tasks.getOne(id, expand = "owner,project.org", fields = "id,title,owner")
```
```dart title="Dart"
final rows = await tasks.list(fields: 'id,title', expand: 'owner');
final one = await tasks.getOne(id, expand: 'owner,project.org', fields: 'id,title,owner');
```
```python title="Python"
rows = tasks.list(fields="id,title", expand="owner")
one = tasks.get_one(record_id, expand="owner,project.org", fields="id,title,owner")
```
```rust title="Rust"
let rows = tasks.list(Some(&ListOptions::new().fields("id,title").expand("owner"))).await?;
```
:::

`fields` limits the response to some top-level fields (`id` is always there). `expand` follows relations up to 4 hops and embeds the related records.

## Count

:::tabs
```ts title="TypeScript"
const n = await tasks.count({ filter: "done = false" });
```
```kotlin title="Kotlin"
val open: Long = tasks.count(filter = "done = false")
```
```dart title="Dart"
final open = await tasks.count(filter: 'done = false');
```
```python title="Python"
open_count = tasks.count(filter="done = false")
```
```rust title="Rust"
let open: u64 = tasks.count(Some("done = false")).await?;
```
:::

:::note
The TypeScript `count()` does not match the server's response yet. Use `list({ total: "exact", limit: 1 })` and read `total` instead.
:::

## Options

| Option | Default | Meaning |
|---|---|---|
| `filter` | none | Up to 4,096 characters. It cannot use `hidden` fields. |
| `sort` | `id` | Up to 3 fields. `id` is always added last as a tie-breaker. |
| `limit` | 50 | 1 to 1000 |
| `cursor` | none | The cursor of the previous page |
| `fields` | all | Top-level fields to return |
| `expand` | none | Relation paths, such as `owner,project.org` |
| `total` | `none` | `exact` or `estimate` adds `total` to the result. `exact` runs a `COUNT(*)`. |
| `page`, `perPage` | none | Page-number paging. Prefer cursors. |

In Rust these are builder methods on `ListOptions`; in Kotlin they are fields of `ListOptions`; elsewhere they are named arguments or an options object. TypeScript, Kotlin and Dart retry a `GET` that answers `503`, using `Retry-After`.

## Limits and status

There is no full-text search, and `json` fields cannot be filtered or sorted. Full query reference: [Query records](/build/database/query).

## Next steps

- [Write data](/sdks/write)
- [Realtime](/sdks/realtime)
- [Rules syntax](/reference/rules-syntax)
