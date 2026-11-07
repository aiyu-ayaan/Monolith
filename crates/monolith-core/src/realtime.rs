//! Realtime hub (doc 07): one task per app tails the change bus, fetches the changed rows
//! once per batch, encodes each event once per audience (superuser / everyone else) and
//! pushes the shared frame into bounded per-connection queues.
//!
//! v1 limits (doc 07 "implemented today"): whole-collection topics only (no per-record or
//! filtered topics), no `since` replay (clients refetch on reconnect), SSE only, no
//! relational view rules for non-superusers, and delete events go to non-superusers only
//! when the view rule is public (the pre-delete row is not captured yet).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex, RwLock};

use monolith_rules::Class;
use monolith_store::id::encode as encode_id;
use monolith_store::{AppStore, Change, ChangeBatch, Id, Op, Query, Value};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc};

use crate::app::{App, Principal, now_us, row_multi};
use crate::codec;
use crate::error::{CoreError, CoreResult};
use crate::schema::Collection;

/// Frames queued per connection before it is dropped (it reconnects and refetches).
pub const QUEUE: usize = 256;
pub const MAX_CONNS: usize = 10_000;
pub const MAX_COLLS: usize = 32;
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

struct Conn {
    id: u64,
    tx: mpsc::Sender<Frame>,
    p: Principal,
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
        if names.is_empty() || names.len() > MAX_COLLS {
            return Err(CoreError::BadRequest(format!(
                "collections must list 1..={MAX_COLLS} collection names"
            )));
        }
        let schema = self.schema();
        let mut colls = Vec::with_capacity(names.len());
        for n in names {
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
            if !colls.contains(&c.cid) {
                colls.push(c.cid);
            }
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
        Ok(Subscription {
            rx,
            app: self.clone(),
            id,
            colls,
        })
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
                self.rt_fetch(c, schema.version, &ids).await
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
            let mut dead = Vec::new();
            for ch in changes {
                let row = by_id.get(&ch.rid).map(|r| r.as_slice());
                if ch.op != Op::Delete && row.is_none() {
                    continue; // already deleted again; its own delete event follows
                }
                let (mut su_frame, mut other_frame): (Option<Frame>, Option<Frame>) = (None, None);
                for conn in &conns {
                    let allowed = conn.p.superuser
                        || match (row, &c.rules.view) {
                            (Some(row), Some(rule)) => {
                                let bound = rule.bind(&conn.p.ctx(None, now));
                                rule.eval(row, &bound) == Some(true)
                            }
                            (None, _) => public_delete,
                            _ => false,
                        };
                    if !allowed {
                        continue;
                    }
                    let slot = if conn.p.superuser {
                        &mut su_frame
                    } else {
                        &mut other_frame
                    };
                    let frame = slot
                        .get_or_insert_with(|| encode(c, ch, row, &multi, conn.p.superuser))
                        .clone();
                    match conn.tx.try_send(frame) {
                        Ok(()) => {
                            self.hub.stats.events.fetch_add(1, Relaxed);
                        }
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            self.hub.stats.dropped.fetch_add(1, Relaxed);
                            dead.push(conn.id);
                        }
                        Err(mpsc::error::TrySendError::Closed(_)) => dead.push(conn.id),
                    }
                }
            }
            for id in dead {
                self.hub.remove(id, &[cid]);
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
) -> Frame {
    let op = match ch.op {
        Op::Insert => "create",
        Op::Update => "update",
        Op::Delete => "delete",
    };
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
