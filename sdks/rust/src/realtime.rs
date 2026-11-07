//! Realtime over SSE: live events, filtered subscriptions and Firestore-style snapshots.
//!
//! Every call spawns a Tokio task that owns one dedicated SSE connection and returns a
//! [`Subscription`], a [`futures_util::Stream`]. Dropping the subscription stops the task and
//! closes the connection. The task reconnects with exponential backoff and jitter; plain
//! subscriptions resume with `since=<last seq>`, snapshot subscriptions re-snapshot instead.
//!
//! Unlike the TypeScript and Python SDKs, plain subscriptions do **not** share a connection:
//! each [`Collection::subscribe`] opens its own. A Tokio runtime must be running when you call
//! these methods.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{Stream, StreamExt};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::client::Client;
use crate::collections::{Collection, Record};
use crate::errors::{Error, Result};

/// What happened to a record. `Leave`: it stopped matching the filter or rule (not deleted).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RealtimeAction {
    Create,
    Update,
    Delete,
    Leave,
}

/// One live change.
#[derive(Debug, Clone, PartialEq)]
pub struct RealtimeEvent<T = Record> {
    pub action: RealtimeAction,
    pub record: T,
    pub collection: String,
    /// Change-log sequence number; the resume point after a reconnect.
    pub seq: Option<u64>,
}

/// Narrowing for [`Collection::subscribe`].
#[derive(Debug, Clone, Default)]
pub struct SubscribeOptions {
    /// Filter expression (same syntax as list `filter`); only matching records arrive.
    pub filter: Option<String>,
    /// Only these record ids.
    pub ids: Vec<String>,
}

impl SubscribeOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn filter(mut self, filter: impl Into<String>) -> Self {
        self.filter = Some(filter.into());
        self
    }

    pub fn ids<I, S>(mut self, ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.ids = ids.into_iter().map(Into::into).collect();
        self
    }
}

/// Options for [`Collection::on_snapshot`].
#[derive(Debug, Clone, Default)]
pub struct SnapshotOptions {
    pub filter: Option<String>,
    /// Sort expression such as `-created,title`. Also applied client-side to live changes.
    pub sort: Option<String>,
    /// Records in the initial result (server cap 1000). Default 100.
    pub limit: Option<usize>,
}

impl SnapshotOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn filter(mut self, filter: impl Into<String>) -> Self {
        self.filter = Some(filter.into());
        self
    }

    pub fn sort(mut self, sort: impl Into<String>) -> Self {
        self.sort = Some(sort.into());
        self
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    Added,
    Modified,
    Removed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentChange {
    pub change_type: ChangeType,
    pub doc: Record,
}

/// The current result set plus what changed since the previous snapshot
/// (everything is `Added` in the first one).
#[derive(Debug, Clone, PartialEq)]
pub struct QuerySnapshot {
    pub docs: Vec<Record>,
    pub changes: Vec<DocumentChange>,
}

/// A live stream of items. Dropping it stops the background task and closes the connection.
pub struct Subscription<I> {
    rx: mpsc::Receiver<I>,
    task: JoinHandle<()>,
}

impl<I> Subscription<I> {
    /// Waits for the next item. `None` only after the task ended (it normally never does).
    pub async fn recv(&mut self) -> Option<I> {
        self.rx.recv().await
    }

    /// Stops the subscription now (same as dropping it).
    pub fn close(self) {}
}

impl<I> Stream for Subscription<I> {
    type Item = I;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<I>> {
        self.rx.poll_recv(cx)
    }
}

impl<I> Drop for Subscription<I> {
    fn drop(&mut self) {
        self.task.abort();
    }
}

// ---------------------------------------------------------------- connection engine

#[derive(Deserialize)]
struct SeqOnly {
    seq: Option<u64>,
}

/// Index just past the first blank line (`\n\n`) in `buf`.
fn block_end(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n").map(|p| p + 2)
}

/// Returns `(event, data)`; `None` for comments and blocks without data.
fn parse_block(block: &[u8]) -> Option<(String, String)> {
    let text = String::from_utf8_lossy(block);
    let mut event = "message".to_string();
    let mut data = String::new();
    for raw in text.split('\n') {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(v) = line.strip_prefix("event:") {
            event = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(v.trim());
        }
    }
    (!data.is_empty()).then_some((event, data))
}

fn backoff_delay(base: Duration, max: Duration, attempts: u32) -> Duration {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    // Jitter in [0, 40% of base) without a rand dependency.
    let jitter = base.mul_f64(0.4 * f64::from(nanos % 1000) / 1000.0);
    (base.saturating_mul(1u32 << attempts.min(16)) + jitter).min(max)
}

/// Runs one SSE connection forever (reconnecting) until the receiver is gone.
/// `handler(event, data, out)` turns a raw event into zero or more items.
async fn run<I, H>(
    client: Client,
    params: Vec<(String, String)>,
    resume: bool,
    tx: mpsc::Sender<I>,
    mut handler: H,
) where
    H: FnMut(&str, &str, &mut Vec<I>),
{
    let (base, max) = client.realtime_backoff();
    let app = url::form_urlencoded::byte_serialize(client.app().as_bytes()).collect::<String>();
    let path = format!("/api/v1/apps/{app}/realtime");
    let mut attempts: u32 = 0;
    let mut last_seq: u64 = 0;
    let mut out: Vec<I> = Vec::new();

    loop {
        let mut query = params.clone();
        if resume && last_seq > 0 {
            query.push(("since".into(), last_seq.to_string()));
        }
        let refs: Vec<(&str, &str)> = query
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let mut resynced = false;

        if let Ok(url) = client.build_url(&path, Some(&refs))
            && let Ok(resp) = client.open_sse(url).await
        {
            attempts = 0;
            let mut body = resp.bytes_stream();
            let mut buf: Vec<u8> = Vec::new();
            'read: while let Some(Ok(chunk)) = body.next().await {
                buf.extend_from_slice(&chunk);
                while let Some(end) = block_end(&buf) {
                    let block: Vec<u8> = buf.drain(..end).collect();
                    let Some((event, data)) = parse_block(&block) else {
                        continue;
                    };
                    if let Ok(SeqOnly { seq: Some(seq) }) = serde_json::from_str(&data) {
                        last_seq = last_seq.max(seq);
                    }
                    if event == "resync" {
                        // Missed more than the server keeps: reconnect from scratch.
                        last_seq = 0;
                    }
                    handler(&event, &data, &mut out);
                    for item in out.drain(..) {
                        if tx.send(item).await.is_err() {
                            return;
                        }
                    }
                    if event == "resync" {
                        resynced = true;
                        break 'read;
                    }
                }
            }
        }

        if !resynced {
            tokio::time::sleep(backoff_delay(base, max, attempts)).await;
            attempts = attempts.saturating_add(1);
        }
    }
}

fn spawn<I, H>(
    client: Client,
    params: Vec<(String, String)>,
    resume: bool,
    handler: H,
) -> Subscription<I>
where
    I: Send + 'static,
    H: FnMut(&str, &str, &mut Vec<I>) + Send + 'static,
{
    let (tx, rx) = mpsc::channel(256);
    let task = tokio::spawn(run(client, params, resume, tx, handler));
    Subscription { rx, task }
}

// ---------------------------------------------------------------- events

#[derive(Deserialize)]
struct RecordPayload<T> {
    op: RealtimeAction,
    #[serde(default)]
    collection: String,
    #[serde(default)]
    seq: Option<u64>,
    record: T,
}

#[derive(Deserialize)]
struct SnapshotPayload {
    #[serde(default)]
    items: Vec<Record>,
}

#[derive(Deserialize)]
struct ErrorPayload {
    message: Option<String>,
}

fn error_item(data: &str) -> Error {
    let msg = serde_json::from_str::<ErrorPayload>(data)
        .ok()
        .and_then(|p| p.message)
        .unwrap_or_else(|| "realtime error".into());
    Error::Realtime(msg)
}

impl<T: DeserializeOwned + Send + 'static> Collection<T> {
    /// Live events for this collection as a stream.
    ///
    /// Uses a dedicated SSE connection per call. After a reconnect the stream resumes from the
    /// last seen change (`since`), so nothing is missed; on `resync` it restarts from live.
    pub fn subscribe(&self, options: SubscribeOptions) -> Subscription<Result<RealtimeEvent<T>>> {
        let name = self.name.clone();
        let mut params = vec![("collections".to_string(), name.clone())];
        if let Some(f) = options.filter {
            params.push((format!("filter.{name}"), f));
        }
        if !options.ids.is_empty() {
            params.push((format!("ids.{name}"), options.ids.join(",")));
        }
        spawn(
            self.client.clone(),
            params,
            true,
            move |event, data, out| match event {
                "record" => out.push(
                    serde_json::from_str::<RecordPayload<T>>(data)
                        .map(|p| RealtimeEvent {
                            action: p.op,
                            record: p.record,
                            collection: p.collection,
                            seq: p.seq,
                        })
                        .map_err(Error::from),
                ),
                "error" => out.push(Err(error_item(data))),
                _ => {}
            },
        )
    }
}

impl Collection<Record> {
    /// Firestore-style query listener: yields the current result set, then again after every
    /// change that affects it. Reconnects re-snapshot, so the view always converges.
    pub fn on_snapshot(&self, options: SnapshotOptions) -> Subscription<Result<QuerySnapshot>> {
        snapshot_stream(self, options, |s| s)
    }

    /// Listens to one record: yields it, or `None` once deleted or no longer visible.
    pub fn on_doc_snapshot(&self, id: &str) -> Subscription<Result<Option<Record>>> {
        let options = SnapshotOptions::new()
            .filter(format!("id = '{}'", id.replace('\'', "")))
            .limit(1);
        snapshot_stream(self, options, |s| s.docs.into_iter().next())
    }
}

fn snapshot_stream<I, M>(
    coll: &Collection<Record>,
    options: SnapshotOptions,
    map: M,
) -> Subscription<Result<I>>
where
    I: Send + 'static,
    M: Fn(QuerySnapshot) -> I + Send + 'static,
{
    let name = coll.name.clone();
    let mut params = vec![
        ("collections".to_string(), name.clone()),
        (
            "snapshot".to_string(),
            options.limit.unwrap_or(100).to_string(),
        ),
    ];
    if let Some(f) = options.filter {
        params.push((format!("filter.{name}"), f));
    }
    if let Some(s) = &options.sort {
        params.push((format!("sort.{name}"), s.clone()));
    }
    let keys = parse_sort(options.sort.as_deref());
    let mut docs: Vec<Record> = Vec::new();

    spawn(
        coll.client.clone(),
        params,
        false,
        move |event, data, out| {
            let mut changes = Vec::new();
            match event {
                "snapshot" => {
                    let Ok(p) = serde_json::from_str::<SnapshotPayload>(data) else {
                        return;
                    };
                    for d in &p.items {
                        let kind = if docs.iter().any(|o| o.id == d.id) {
                            ChangeType::Modified
                        } else {
                            ChangeType::Added
                        };
                        changes.push(DocumentChange {
                            change_type: kind,
                            doc: d.clone(),
                        });
                    }
                    for old in &docs {
                        if !p.items.iter().any(|d| d.id == old.id) {
                            changes.push(DocumentChange {
                                change_type: ChangeType::Removed,
                                doc: old.clone(),
                            });
                        }
                    }
                    docs = p.items;
                }
                "record" => {
                    let Ok(p) = serde_json::from_str::<RecordPayload<Record>>(data) else {
                        return;
                    };
                    let pos = docs.iter().position(|d| d.id == p.record.id);
                    if matches!(p.op, RealtimeAction::Delete | RealtimeAction::Leave) {
                        let Some(pos) = pos else { return };
                        changes.push(DocumentChange {
                            change_type: ChangeType::Removed,
                            doc: docs.remove(pos),
                        });
                    } else {
                        let kind = match pos {
                            Some(pos) => {
                                docs[pos] = p.record.clone();
                                ChangeType::Modified
                            }
                            None => {
                                docs.push(p.record.clone());
                                ChangeType::Added
                            }
                        };
                        changes.push(DocumentChange {
                            change_type: kind,
                            doc: p.record,
                        });
                    }
                }
                "error" => {
                    out.push(Err(error_item(data)));
                    return;
                }
                _ => return,
            }
            let mut ordered = docs.clone();
            if !keys.is_empty() {
                ordered.sort_by(|a, b| compare(&keys, a, b));
            }
            out.push(Ok(map(QuerySnapshot {
                docs: ordered,
                changes,
            })));
        },
    )
}

// ---------------------------------------------------------------- client-side sort

/// Parses `-created,title` into `(field, direction)` pairs (direction is `1` or `-1`).
fn parse_sort(sort: Option<&str>) -> Vec<(String, i8)> {
    sort.unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| match s.strip_prefix('-') {
            Some(f) => (f.to_string(), -1),
            None => (s.trim_start_matches('+').to_string(), 1),
        })
        .collect()
}

fn field<'a>(r: &'a Record, name: &str) -> Option<Cow<'a, Value>> {
    match name {
        "id" => Some(Cow::Owned(Value::String(r.id.clone()))),
        "created" => r.created.as_ref().map(Cow::Borrowed),
        "updated" => r.updated.as_ref().map(Cow::Borrowed),
        "_v" => r.version.map(|v| Cow::Owned(Value::from(v))),
        _ => r.fields.get(name).map(Cow::Borrowed),
    }
}

fn type_rank(v: &Value) -> u8 {
    match v {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
    }
}

fn cmp_values(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x
            .as_f64()
            .partial_cmp(&y.as_f64())
            .unwrap_or(Ordering::Equal),
        (Value::String(x), Value::String(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => type_rank(a).cmp(&type_rank(b)),
    }
}

/// Nulls (and missing fields) sort first ascending, last descending.
fn compare(keys: &[(String, i8)], a: &Record, b: &Record) -> Ordering {
    for (f, dir) in keys {
        let x = field(a, f).filter(|v| !v.is_null());
        let y = field(b, f).filter(|v| !v.is_null());
        let ord = match (x, y) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(x), Some(y)) => cmp_values(&x, &y),
        };
        if ord != Ordering::Equal {
            return if *dir < 0 { ord.reverse() } else { ord };
        }
    }
    Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rec(v: Value) -> Record {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn sort_multi_key_with_nulls() {
        let keys = parse_sort(Some("-created,title"));
        let mut rows = [
            rec(json!({"id":"1","created":1,"title":"b"})),
            rec(json!({"id":"2","created":2,"title":"z"})),
            rec(json!({"id":"3","created":1,"title":"a"})),
            rec(json!({"id":"4","created":null,"title":"q"})),
        ];
        rows.sort_by(|a, b| compare(&keys, a, b));
        let ids: Vec<_> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["2", "3", "1", "4"]);
    }

    #[test]
    fn parses_blocks_and_ignores_comments() {
        assert_eq!(parse_block(b": hb\n\n"), None);
        let b = parse_block(b"id: 3\nevent: record\ndata: {\"a\":1}\n\n").unwrap();
        assert_eq!(b, ("record".to_string(), "{\"a\":1}".to_string()));
        assert_eq!(block_end(b"a\n\nb"), Some(3));
    }

    #[test]
    fn backoff_grows_and_caps() {
        let (b, m) = (Duration::from_millis(500), Duration::from_secs(15));
        assert!(backoff_delay(b, m, 0) >= b && backoff_delay(b, m, 0) < b * 2);
        assert!(backoff_delay(b, m, 2) >= b * 4);
        assert_eq!(backoff_delay(b, m, 30), m);
    }
}
