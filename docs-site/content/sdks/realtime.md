---
title: Realtime
description: Live queries, single-record listeners and change events.
group: Use the SDKs
order: 50
---

Realtime pushes changes over Server-Sent Events. A live query gives you the current result first and a new snapshot after each change, so a screen stays current without polling.

## Listen to a query

:::tabs
```ts title="TypeScript"
const stop = tasks.onSnapshot(
  (snap) => {
    render(snap.docs);                       // the full, ordered result
    for (const c of snap.changes) console.log(c.type, c.doc.id); // added | modified | removed
  },
  { filter: "done = false", sort: "-created", limit: 50, onError: (e) => console.error(e) },
);
// later
stop();
```
```kotlin title="Kotlin"
// As a Flow (recommended on Android)
tasks.onSnapshot(SnapshotOptions(filter = "done = false", sort = "-created", limit = 50))
    .collect { snap ->
        adapter.submitList(snap.docs)
        snap.changes.forEach { println("${it.type} ${it.doc.id}") }   // ADDED | MODIFIED | REMOVED
    }

// As a callback
val stop = tasks.onSnapshot({ snap -> render(snap.docs) }, SnapshotOptions(filter = "done = false"))
stop()
```
```dart title="Dart"
final sub = tasks
    .onSnapshot(SnapshotOptions(filter: 'done = false', sort: '-created', limit: 50))
    .listen(
      (snap) {
        setState(() => items = snap.docs);
        for (final c in snap.changes) {
          print('${c.type} ${c.doc}');   // added | modified | removed
        }
      },
      onError: (e) => print('realtime error: $e'),
    );
// later
await sub.cancel();
```
```python title="Python"
def on_snap(snap):
    print(len(snap.docs), "docs")
    for c in snap.changes:
        print(c.type, c.doc["id"])      # added | modified | removed

stop = tasks.on_snapshot(on_snap, filter="done = false", sort="-created", limit=50,
                         on_error=lambda e: print("realtime error", e))
# later
stop()
```
```rust title="Rust"
use monolith_client::SnapshotOptions;

let mut sub = mono.records("tasks")
    .on_snapshot(SnapshotOptions::new().filter("done = false").sort("-created").limit(50));
while let Some(snap) = sub.recv().await {
    let snap = snap?;
    println!("{} docs, {} changes", snap.docs.len(), snap.changes.len());
}
```
:::

The first snapshot reports every document as `added`. After that, `changes` lists what changed (`added`, `modified`, `removed`) and `docs` is the full ordered result. `limit` defaults to 100, server cap 1000.

## Listen to one record

:::tabs
```ts title="TypeScript"
const stop = tasks.onDocSnapshot(id, (doc) => {
  if (doc === null) showDeleted();   // deleted, or no longer visible to you
  else render(doc);
});
```
```kotlin title="Kotlin"
tasks.onDocSnapshot(id).collect { task ->
    if (task == null) showDeleted() else render(task)   // null: deleted or no longer visible
}
```
```dart title="Dart"
tasks.onDocSnapshot(id).listen((task) {
  if (task == null) showDeleted();   // deleted, or no longer visible
  else render(task);
});
```
```python title="Python"
stop = tasks.on_doc_snapshot(record_id, lambda doc: print("gone" if doc is None else doc["title"]))
```
```rust title="Rust"
let mut sub = mono.records("tasks").on_doc_snapshot(&id);
while let Some(doc) = sub.recv().await {
    match doc? { Some(r) => render(r), None => show_deleted() }   // None: deleted or no longer visible
}
```
:::

The callback gets `null` (`None`) once the record is deleted or no longer visible to you.

## Raw change events

:::tabs
```ts title="TypeScript"
const stop = tasks.subscribe((e) => {
  console.log(e.action, e.record.id, e.seq);   // create | update | delete | leave
}, { filter: "done = false" });
```
```kotlin title="Kotlin"
tasks.subscribe(filter = "done = false").collect { e ->
    println("${e.action} ${e.record.id} seq=${e.seq}")   // create | update | delete | leave
}
```
```dart title="Dart"
final stop = tasks.subscribe((e) {
  print('${e.action} ${e.record} seq=${e.seq}');   // create | update | delete | leave
}, RealtimeSubscriptionOptions(filter: 'done = false'));
stop();
```
```python title="Python"
stop = tasks.subscribe(lambda e: print(e.action, e.record["id"], e.seq), filter="done = false")
```
```rust title="Rust"
use monolith_client::SubscribeOptions;
let mut sub = mono.collection::<Task>("tasks").subscribe(SubscribeOptions::new().filter("done = false"));
while let Some(ev) = sub.recv().await {
    let ev = ev?;
    println!("{:?} seq={:?}", ev.action, ev.seq);   // Create | Update | Delete | Leave
}
```
:::

`action` is `create`, `update`, `delete` or `leave`. `leave` means the record stopped matching your filter or rule. It was not deleted.

## How it behaves

- The SDK reconnects with backoff, resumes from the last sequence number, and takes a fresh snapshot when the server asks for it.
- Rules apply: you only receive rows your view rule allows.
- Subscriptions with the same collection and no filter can share a connection (TypeScript, Python). A filter opens its own.

| Language | Stop listening | Notes |
|---|---|---|
| TypeScript | Call the returned function | In raw HTTP use `fetch` streaming: `EventSource` cannot send `Authorization`. |
| Kotlin | Cancel the `Flow` collection, or call the returned function | Collect with `repeatOnLifecycle(STARTED)` on Android. |
| Dart | `cancel()` the stream subscription | `StreamBuilder` works in Flutter. |
| Python | Call the returned function, or `mono.close()` | Callbacks run on a background thread. |
| Rust | Drop the `Subscription` | `on_snapshot` and `on_doc_snapshot` exist on `Collection<Record>` only. Set backoff with `realtime_backoff`. |

Connection limits, replay window and queue size: [Realtime](/build/database/realtime).

## Next steps

- [Read and query](/sdks/query)
- [Rules](/build/database/rules)
