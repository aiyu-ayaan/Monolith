//! Realtime hub (doc 07): one task per app tails the change bus, fetches the changed rows
//! once per batch, encodes each event once per audience (superuser / everyone else) and
//! pushes the shared frame into bounded per-connection queues.
//!
//! Topics: a whole collection, or a collection narrowed by a `filter` and/or record `ids`.
//! A narrowed topic tracks which records the connection has seen (`seen`, bounded) so it can
//! send `leave` when a record stops matching and deliver deletes of records it showed. A
//! connection can start with a `snapshot` (current result set) or replay missed changes with
//! `since` (`_changes` log); live events that arrive meanwhile are buffered behind a gate.
//!
//! Limits (doc 07 "implemented today"): SSE only, no relational view rules or filters for
//! non-superusers, `seen` is capped at [`MAX_SEEN`] (beyond it every delete is delivered),
//! and untracked topics deliver deletes to non-superusers only when the view rule is public
//! (the pre-delete row is not captured yet).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex, RwLock};

use monolith_rules::{Class, Compiled, Mode, compile};
use monolith_store::id::encode as encode_id;
use monolith_store::{AppStore, Change, ChangeBatch, Id, Op, Query, Value};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc};

use crate::app::{App, ListParams, Principal, now_us, row_multi};
use crate::codec;
use crate::error::{CoreError, CoreResult};
use crate::schema::Collection;

/// Frames queued per connection before it is dropped (it reconnects and refetches).
pub const QUEUE: usize = 256;
pub const MAX_CONNS: usize = 10_000;
pub const MAX_COLLS: usize = 32;
/// Records one narrowed topic remembers as delivered.
pub const MAX_SEEN: usize = 50_000;
/// Changes replayed for `since` before the client is told to resync.
pub const MAX_REPLAY: usize = 20_000;
/// Rows a snapshot may carry.
pub const MAX_SNAPSHOT: usize = 1_000;
/// Seconds between heartbeat comments.
pub const HEARTBEAT_S: u64 = 25;
/// Rows fetched per statement; shorter lists are padded up to the next bucket so statements cache.
const BUCKETS: [usize; 4] = [1, 8, 64, 512];

/// One ready-to-write SSE frame, shared by every receiver.
pub type Frame = Arc<str>;

#[derive(Debug, Default)]
pub struct HubStats {
    pub connections: AtomicU64,
    pub events: AtomicU64,
    pub dropped: AtomicU64,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeStats {
    pub connections: u64,
    pub events: u64,
    pub dropped: u64,
}

/// What one connection wants from one collection.
#[derive(Debug, Default, Clone)]
pub struct TopicSpec {
    pub collection: String,
    /// Filter expression (doc 06); only matching records are delivered.
    pub filter: String,
    /// Base32 record ids; only these records are delivered.
    pub ids: Vec<String>,
    /// Sort of the snapshot.
    pub sort: String,
    /// Send the current result set first (at most this many records).
    pub snapshot: Option<usize>,
}

#[derive(Default)]
struct Seen {
    set: Mutex<HashSet<Id>>,
    overflow: AtomicBool,
}

impl Seen {
    fn add(&self, id: Id) {
        let mut g = self.set.lock().unwrap_or_else(|e| e.into_inner());
        if g.len() >= MAX_SEEN {
            self.overflow.store(true, Relaxed);
        } else {
            g.insert(id);
        }
    }

    fn take(&self, id: &Id) -> bool {
        self.set
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id)
    }
}

#[derive(Default)]
struct Topic {
    filter: Option<Compiled>,
    ids: Option<HashSet<Id>>,
    /// Present when the topic can drop records (filter or ids).
    seen: Option<Seen>,
}

struct Conn {
    id: u64,
    tx: mpsc::Sender<Frame>,
    p: Principal,
    topics: HashMap<u32, Topic>,
    /// `Some` while the snapshot or replay is still being sent; live frames wait here.
    gate: Mutex<Option<Vec<(u64, Frame)>>>,
    gated: AtomicBool,
}

enum Verdict {
    Send,
    Leave,
    Skip,
}

/// Decides what connection `conn` learns from change `ch`; `row` is `None` for deletes.
fn decide(
    conn: &Conn,
    t: &Topic,
    c: &Collection,
    ch: &Change,
    row: Option<&[Value]>,
    public_delete: bool,
    now: i64,
) -> Verdict {
    if t.ids.as_ref().is_some_and(|ids| !ids.contains(&ch.rid)) {
        return Verdict::Skip;
    }
    let Some(row) = row else {
        let blind_ok = t.filter.is_none() && (conn.p.superuser || public_delete);
        let known = t
            .seen
            .as_ref()
            .is_some_and(|s| s.take(&ch.rid) || s.overflow.load(Relaxed));
        return if blind_ok || known {
            Verdict::Send
        } else {
            Verdict::Skip
        };
    };
    let ctx = conn.p.ctx(None, now);
    let view_ok = conn.p.superuser
        || c.rules
            .view
            .as_ref()
            .is_some_and(|r| r.eval(row, &r.bind(&ctx)) == Some(true));
    let filter_ok = t
        .filter
        .as_ref()
        .is_none_or(|f| f.eval(row, &f.bind(&ctx)) == Some(true));
    if view_ok && filter_ok {
        if let Some(s) = &t.seen {
            s.add(ch.rid);
        }
        Verdict::Send
    } else if t.seen.as_ref().is_some_and(|s| s.take(&ch.rid)) {
        Verdict::Leave
    } else {
        Verdict::Skip
    }
}

#[derive(Default)]
pub struct Hub {
    by_coll: RwLock<HashMap<u32, Vec<Arc<Conn>>>>,
    next: AtomicU64,
    started: AtomicBool,
    stats: HubStats,
    /// `(collection id, schema version, bucket)` → `SELECT … WHERE id IN (?, …)`.
    sql: Mutex<HashMap<(u32, u64, usize), Arc<str>>>,
}

impl Hub {
    pub fn stats(&self) -> RealtimeStats {
        RealtimeStats {
            connections: self.stats.connections.load(Relaxed),
            events: self.stats.events.load(Relaxed),
            dropped: self.stats.dropped.load(Relaxed),
        }
    }

    /// Ends every stream (server shutdown): dropping the senders closes the receivers.
    pub fn close(&self) {
        self.by_coll
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.stats.connections.store(0, Relaxed);
    }

    fn remove(&self, id: u64, colls: &[u32]) {
        let mut g = self.by_coll.write().unwrap_or_else(|e| e.into_inner());
        for c in colls {
            if let Some(v) = g.get_mut(c) {
                if let Some(i) = v.iter().position(|x| x.id == id) {
                    v.swap_remove(i);
                    self.stats.connections.fetch_sub(1, Relaxed);
                }
                if v.is_empty() {
                    g.remove(c);
                }
            }
        }
    }

    fn broadcast(&self, frame: &Frame) {
        let g = self.by_coll.read().unwrap_or_else(|e| e.into_inner());
        for c in g.values().flatten() {
            let _ = c.tx.try_send(frame.clone());
        }
    }

    /// Hands a live frame to `conn`: buffered while its gate is open, else queued.
    fn push_live(&self, conn: &Conn, seq: u64, frame: Frame, dead: &mut Vec<u64>) {
        if conn.gated.load(Relaxed) {
            let mut g = conn.gate.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(buf) = g.as_mut() {
                if buf.len() >= QUEUE {
                    self.stats.dropped.fetch_add(1, Relaxed);
                    dead.push(conn.id);
                } else {
                    buf.push((seq, frame));
                }
                return;
            }
        }
        match conn.tx.try_send(frame) {
            Ok(()) => {
                self.stats.events.fetch_add(1, Relaxed);
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.stats.dropped.fetch_add(1, Relaxed);
                dead.push(conn.id);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => dead.push(conn.id),
        }
    }
}

/// Receiving side of one connection. Dropping it unregisters the connection.
pub struct Subscription {
    pub rx: mpsc::Receiver<Frame>,
    app: Arc<App>,
    id: u64,
    colls: Vec<u32>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.app.hub.remove(self.id, &self.colls);
    }
}

fn bucket(n: usize) -> usize {
    BUCKETS.into_iter().find(|b| *b >= n).unwrap_or(BUCKETS[3])
}

impl App {
    /// Subscribes `p` to every change of the named collections.
    pub fn subscribe(
        self: &Arc<Self>,
        p: &Principal,
        names: &[String],
    ) -> CoreResult<Subscription> {
        let specs: Vec<TopicSpec> = names
            .iter()
            .map(|n| TopicSpec {
                collection: n.clone(),
                ..Default::default()
            })
            .collect();
        Ok(self.register(p, &specs, false)?.0)
    }

    /// Subscribes with filters, record ids, a snapshot, or a replay from `since`
    /// (a snapshot wins over `since`: it already reflects every change).
    pub fn subscribe_with(
        self: &Arc<Self>,
        p: &Principal,
        specs: &[TopicSpec],
        since: Option<u64>,
    ) -> CoreResult<Subscription> {
        let snap = specs.iter().any(|s| s.snapshot.is_some());
        let (sub, conn) = self.register(p, specs, snap || since.is_some())?;
        if snap || since.is_some() {
            let app = self.clone();
            let specs = specs.to_vec();
            tokio::spawn(async move {
                app.prime(&conn, &specs, if snap { None } else { since })
                    .await;
            });
        }
        Ok(sub)
    }

    fn register(
        self: &Arc<Self>,
        p: &Principal,
        specs: &[TopicSpec],
        gated: bool,
    ) -> CoreResult<(Subscription, Arc<Conn>)> {
        if specs.is_empty() || specs.len() > MAX_COLLS {
            return Err(CoreError::BadRequest(format!(
                "collections must list 1..={MAX_COLLS} collection names"
            )));
        }
        let schema = self.schema();
        let mut colls = Vec::with_capacity(specs.len());
        let mut topics = HashMap::with_capacity(specs.len());
        for s in specs {
            let n = &s.collection;
            let c = schema.get(n)?;
            if !p.superuser {
                let rule = c
                    .rules
                    .view
                    .as_ref()
                    .ok_or(CoreError::Forbidden("the view rule allows superusers only"))?;
                if rule.class == Class::Relational {
                    return Err(CoreError::BadRequest(format!(
                        "realtime does not support relational view rules yet ('{n}')"
                    )));
                }
            }
            if topics.contains_key(&c.cid) {
                return Err(CoreError::BadRequest(format!(
                    "collection '{n}' is listed twice; open one connection per topic"
                )));
            }
            let filter = if s.filter.trim().is_empty() {
                None
            } else {
                let f = compile(
                    &*schema,
                    n,
                    &s.filter,
                    if p.superuser {
                        Mode::Rule
                    } else {
                        Mode::Filter
                    },
                )?;
                if f.class == Class::Relational {
                    return Err(CoreError::BadRequest(
                        "realtime filters cannot use relations yet".into(),
                    ));
                }
                Some(f)
            };
            let ids = if s.ids.is_empty() {
                None
            } else {
                let mut set = HashSet::with_capacity(s.ids.len());
                for i in &s.ids {
                    set.insert(
                        monolith_store::id::decode(i)
                            .ok_or_else(|| CoreError::BadRequest(format!("invalid id '{i}'")))?,
                    );
                }
                Some(set)
            };
            let seen = (filter.is_some() || ids.is_some()).then(Seen::default);
            colls.push(c.cid);
            topics.insert(c.cid, Topic { filter, ids, seen });
        }
        if self.hub.stats.connections.load(Relaxed) as usize >= MAX_CONNS {
            return Err(CoreError::Busy);
        }
        let (tx, rx) = mpsc::channel(QUEUE);
        let id = self.hub.next.fetch_add(1, Relaxed) + 1;
        let hello: Frame =
            format!("event: hello\ndata: {{\"conn\":{id},\"hb\":{HEARTBEAT_S}}}\n\n").into();
        let _ = tx.try_send(hello);
        let conn = Arc::new(Conn {
            id,
            tx,
            p: p.clone(),
            topics,
            gate: Mutex::new(gated.then(Vec::new)),
            gated: AtomicBool::new(gated),
        });
        {
            let mut g = self.hub.by_coll.write().unwrap_or_else(|e| e.into_inner());
            for c in &colls {
                g.entry(*c).or_default().push(conn.clone());
                self.hub.stats.connections.fetch_add(1, Relaxed);
            }
        }
        if !self.hub.started.swap(true, Relaxed) {
            let (rx_bus, weak) = (self.store().subscribe(), Arc::downgrade(self));
            tokio::spawn(async move {
                let mut rx_bus: broadcast::Receiver<ChangeBatch> = rx_bus;
                loop {
                    match rx_bus.recv().await {
                        Ok(b) => match weak.upgrade() {
                            Some(app) => app.fan_out(&b).await,
                            None => break,
                        },
                        Err(broadcast::error::RecvError::Lagged(_)) => match weak.upgrade() {
                            Some(app) => app.hub.broadcast(
                                &"event: resync\ndata: {\"reason\":\"lagged\"}\n\n".into(),
                            ),
                            None => break,
                        },
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
        }
        Ok((
            Subscription {
                rx,
                app: self.clone(),
                id,
                colls,
            },
            conn,
        ))
    }

    /// Sends the snapshot or the replay, then opens the gate and flushes what arrived meanwhile.
    async fn prime(&self, conn: &Arc<Conn>, specs: &[TopicSpec], since: Option<u64>) {
        let mut floor = 0u64;
        if since.is_none() {
            for s in specs.iter().filter(|s| s.snapshot.is_some()) {
                match self.rt_snapshot(conn, s).await {
                    Ok((frame, seq)) => {
                        floor = floor.max(seq);
                        if conn.tx.send(frame).await.is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        let f: Frame = format!(
                            "event: error\ndata: {{\"collection\":\"{}\",\"message\":{}}}\n\n",
                            s.collection,
                            serde_json::Value::String(e.to_string())
                        )
                        .into();
                        if conn.tx.send(f).await.is_err() {
                            return;
                        }
                    }
                }
            }
        } else if let Some(after) = since {
            match self.rt_replay(conn, after).await {
                Some(last) => floor = last,
                None => {
                    let f: Frame = "event: resync\ndata: {\"reason\":\"since\"}\n\n".into();
                    let _ = conn.tx.send(f).await;
                }
            }
        }
        loop {
            let buf = {
                let mut g = conn.gate.lock().unwrap_or_else(|e| e.into_inner());
                let buf = std::mem::take(g.as_mut().expect("gate stays open until here"));
                if buf.is_empty() {
                    *g = None;
                    conn.gated.store(false, Relaxed);
                    return;
                }
                buf
            };
            for (seq, frame) in buf {
                if seq > floor {
                    self.hub.stats.events.fetch_add(1, Relaxed);
                    if conn.tx.send(frame).await.is_err() {
                        return;
                    }
                }
            }
        }
    }

    /// Current result set of one topic as an `event: snapshot` frame, plus the seq it reflects.
    async fn rt_snapshot(&self, conn: &Conn, s: &TopicSpec) -> CoreResult<(Frame, u64)> {
        let seq = match self
            .store()
            .query(Query {
                sql: "SELECT coalesce(max(seq), 0) FROM _changes".into(),
                params: vec![],
            })
            .await?
            .first()
            .and_then(|r| r.first())
        {
            Some(Value::Int(n)) => *n as u64,
            _ => 0,
        };
        let lp = ListParams {
            filter: s.filter.clone(),
            sort: s.sort.clone(),
            limit: Some(s.snapshot.unwrap_or(MAX_SNAPSHOT).clamp(1, MAX_SNAPSHOT)),
            ..Default::default()
        };
        let body = self.list_records(&conn.p, &s.collection, &lp).await?;
        let schema = self.schema();
        if let Some(t) = schema
            .get(&s.collection)
            .ok()
            .and_then(|c| conn.topics.get(&c.cid))
            && let Some(seen) = &t.seen
            && let Ok(j) = serde_json::from_slice::<serde_json::Value>(&body)
        {
            for it in j["items"].as_array().into_iter().flatten() {
                if let Some(id) = it["id"].as_str().and_then(monolith_store::id::decode)
                    && t.ids.as_ref().is_none_or(|ids| ids.contains(&id))
                {
                    seen.add(id);
                }
            }
        }
        let mut out = format!("id: {seq}\nevent: snapshot\ndata: {{\"collection\":").into_bytes();
        codec::write_str(&mut out, &s.collection);
        out.extend_from_slice(format!(",\"seq\":{seq},").as_bytes());
        out.extend_from_slice(&body[1..]);
        out.extend_from_slice(b"\n\n");
        Ok((String::from_utf8_lossy(&out).into_owned().into(), seq))
    }

    /// Queues the changes after `after` for one connection; the last seq read, or `None`
    /// when more than [`MAX_REPLAY`] changes are missing (the client must resync).
    async fn rt_replay(&self, conn: &Arc<Conn>, after: u64) -> Option<u64> {
        let schema = self.schema();
        let mut last = after;
        let mut total = 0usize;
        loop {
            let batch = self.store().changes_since(last, 1_000).await.ok()?;
            let Some(end) = batch.last() else {
                return Some(last);
            };
            last = end.seq;
            total += batch.len();
            if total > MAX_REPLAY {
                return None;
            }
            let mut by_coll: HashMap<u32, Vec<&Change>> = HashMap::new();
            for ch in &batch {
                if conn.topics.contains_key(&ch.coll) {
                    by_coll.entry(ch.coll).or_default().push(ch);
                }
            }
            for (cid, changes) in by_coll {
                let Some(c) = schema.collections.values().find(|c| c.cid == cid) else {
                    continue;
                };
                let mut out = Vec::new();
                self.dispatch(
                    c,
                    schema.version,
                    &changes,
                    std::slice::from_ref(conn),
                    &mut |_, _, f| out.push(f),
                )
                .await;
                for f in out {
                    if conn.tx.send(f).await.is_err() {
                        return Some(last);
                    }
                }
            }
        }
    }

    /// Rows for `ids` (full column list, slot order), in one statement per chunk.
    async fn rt_fetch(&self, c: &Collection, version: u64, ids: &[Id]) -> Vec<Vec<Value>> {
        let mut rows = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(BUCKETS[3]) {
            let b = bucket(chunk.len());
            let sql = self
                .hub
                .sql
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry((c.cid, version, b))
                .or_insert_with(|| {
                    let marks = vec!["?"; b].join(",");
                    format!(
                        "SELECT {} FROM \"{}\" AS t WHERE t.\"id\" IN ({marks})",
                        c.sql.cols, c.def.name
                    )
                    .into()
                })
                .clone();
            let params = (0..b)
                .map(|i| Value::Blob(chunk[i.min(chunk.len() - 1)].into()))
                .collect();
            if let Ok(r) = self.store().query(Query { sql, params }).await {
                rows.extend(r);
            }
        }
        rows
    }

    async fn fan_out(&self, batch: &ChangeBatch) {
        let wanted: HashSet<u32> = {
            let g = self.hub.by_coll.read().unwrap_or_else(|e| e.into_inner());
            batch
                .iter()
                .map(|c| c.coll)
                .filter(|c| g.contains_key(c))
                .collect()
        };
        if wanted.is_empty() {
            return;
        }
        let schema = self.schema();
        for cid in wanted {
            let Some(c) = schema.collections.values().find(|c| c.cid == cid) else {
                continue;
            };
            let conns = match self
                .hub
                .by_coll
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .get(&cid)
            {
                Some(v) => v.clone(),
                None => continue,
            };
            let changes: Vec<&Change> = batch.iter().filter(|ch| ch.coll == cid).collect();
            let mut dead = Vec::new();
            let hub = &self.hub;
            self.dispatch(c, schema.version, &changes, &conns, &mut |conn, seq, f| {
                hub.push_live(conn, seq, f, &mut dead)
            })
            .await;
            for id in dead {
                self.hub.remove(id, &[cid]);
            }
        }
    }

    /// Encodes each change once per audience and calls `emit(conn, seq, frame)` for every
    /// connection that should see it.
    async fn dispatch(
        &self,
        c: &Collection,
        version: u64,
        changes: &[&Change],
        conns: &[Arc<Conn>],
        emit: &mut impl FnMut(&Arc<Conn>, u64, Frame),
    ) {
        let mut ids: Vec<Id> = changes
            .iter()
            .filter(|ch| ch.op != Op::Delete)
            .map(|ch| ch.rid)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        let rows = if ids.is_empty() {
            Vec::new()
        } else {
            self.rt_fetch(c, version, &ids).await
        };
        let multi = self.load_multi_rels(c, &rows).await.unwrap_or_default();
        let by_id: HashMap<Id, &Vec<Value>> = rows
            .iter()
            .filter_map(|r| match r.first() {
                Some(Value::Blob(b)) => <&Id>::try_from(&b[..]).ok().map(|id| (*id, r)),
                _ => None,
            })
            .collect();
        let public_delete = c.def.rules.view.as_deref() == Some("");
        let now = now_us();
        for ch in changes {
            let row = by_id.get(&ch.rid).map(|r| r.as_slice());
            if ch.op != Op::Delete && row.is_none() {
                continue; // already deleted again; its own delete event follows
            }
            let (mut su_frame, mut other_frame, mut leave_frame): (
                Option<Frame>,
                Option<Frame>,
                Option<Frame>,
            ) = (None, None, None);
            for conn in conns {
                let Some(t) = conn.topics.get(&c.cid) else {
                    continue;
                };
                let frame = match decide(conn, t, c, ch, row, public_delete, now) {
                    Verdict::Skip => continue,
                    Verdict::Leave => leave_frame
                        .get_or_insert_with(|| encode(c, ch, None, &multi, true, Some("leave")))
                        .clone(),
                    Verdict::Send => {
                        let slot = if conn.p.superuser {
                            &mut su_frame
                        } else {
                            &mut other_frame
                        };
                        slot.get_or_insert_with(|| {
                            encode(c, ch, row, &multi, conn.p.superuser, None)
                        })
                        .clone()
                    }
                };
                emit(conn, ch.seq, frame);
            }
        }
    }
}

fn encode(
    c: &Collection,
    ch: &Change,
    row: Option<&[Value]>,
    multi: &crate::app::MultiRelMaps,
    su: bool,
    op_override: Option<&'static str>,
) -> Frame {
    let op = op_override.unwrap_or(match ch.op {
        Op::Insert => "create",
        Op::Update => "update",
        Op::Delete => "delete",
    });
    let mut out = Vec::with_capacity(256);
    out.extend_from_slice(
        format!(
            "id: {}\nevent: record\ndata: {{\"op\":\"{op}\",\"collection\":",
            ch.seq
        )
        .as_bytes(),
    );
    codec::write_str(&mut out, &c.def.name);
    out.extend_from_slice(format!(",\"seq\":{},\"record\":", ch.seq).as_bytes());
    match row {
        Some(row) => {
            let rm = row_multi(row, multi);
            codec::write_record_ext(&mut out, c, row, su, None, None, &rm, &[]);
        }
        None => {
            out.extend_from_slice(b"{\"id\":\"");
            out.extend_from_slice(&encode_id(&ch.rid));
            out.extend_from_slice(b"\"}");
        }
    }
    out.extend_from_slice(b"}\n\n");
    // JSON from the codec is valid UTF-8; fall back to lossy rather than panic.
    String::from_utf8(out)
        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
        .into()
}
