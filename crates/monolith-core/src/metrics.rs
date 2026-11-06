//! Server-side traffic metrics (doc 10 §1, doc 16 §5): request counters fed by the HTTP
//! layer, record op counters fed by the change bus, kept in memory as per-minute
//! (last hour) and per-hour (last 48 h) rings. Recording is a handful of relaxed atomic
//! adds; no locks, no allocation.
//!
//! shortcut: in memory only, reset on restart; persisted rollups arrive with the
//! analytics store (Phase 4, doc 10 §3).

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, RwLock};

use monolith_store::{AppStore, ChangeBatch, Op, Query, StoreStats, Value};
use serde::Serialize;
use tokio::sync::broadcast;

use crate::app::{App, Principal, TotalMode, now_us};
use crate::codec::format_us;
use crate::error::{CoreError, CoreResult};
use crate::system::Registry;

const REQ: usize = 0;
const READ: usize = 1;
const WRITE: usize = 2;
const CREATE: usize = 3;
const UPDATE: usize = 4;
const DELETE: usize = 5;
const E4: usize = 6;
const E5: usize = 7;
const BYTES_IN: usize = 8;
const BYTES_OUT: usize = 9;
const DUR: usize = 10;
const DUR_MAX: usize = 11;
/// Latency histogram: bucket `i` holds durations below `2^(i+6)` µs (64 µs .. ~8 s).
const HIST: usize = 12;
const HIST_N: usize = 18;
const N: usize = HIST + HIST_N;

const MINUTES: usize = 60;
const HOURS: usize = 48;
const MIN_US: i64 = 60_000_000;
const HOUR_US: i64 = 3_600_000_000;

/// What a request did, as the HTTP layer classifies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReqKind {
    /// GET/HEAD on a records route.
    Read,
    /// Any other method on a records route, or a batch.
    Write,
    Other,
}

type Counts = [u64; N];

struct Slot {
    /// Bucket number (minute or hour since epoch); `u64::MAX` for the all-time slot.
    epoch: AtomicU64,
    c: [AtomicU64; N],
}

impl Slot {
    fn new(epoch: u64) -> Self {
        Self {
            epoch: AtomicU64::new(epoch),
            c: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    fn read(&self) -> Counts {
        std::array::from_fn(|i| self.c[i].load(Relaxed))
    }
}

/// Returns the ring slot for bucket `e`, zeroing it when it last held an older bucket.
// shortcut: an add that races the reset can be lost; fine for dashboards, not billing.
fn slot(ring: &[Slot], e: u64) -> &Slot {
    let s = &ring[e as usize % ring.len()];
    let cur = s.epoch.load(Relaxed);
    if cur < e && s.epoch.compare_exchange(cur, e, Relaxed, Relaxed).is_ok() {
        s.c.iter().for_each(|c| c.store(0, Relaxed));
    }
    s
}

fn hist_bucket(us: u64) -> usize {
    ((u64::BITS - us.leading_zeros()) as usize)
        .saturating_sub(6)
        .min(HIST_N - 1)
}

fn merge(into: &mut Counts, c: &Counts) {
    for i in 0..N {
        if i == DUR_MAX {
            into[i] = into[i].max(c[i]);
        } else {
            into[i] += c[i];
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Traffic {
    pub requests: u64,
    pub reads: u64,
    pub writes: u64,
    pub creates: u64,
    pub updates: u64,
    pub deletes: u64,
    pub errors4xx: u64,
    pub errors5xx: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub avg_us: u64,
    pub max_us: u64,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
}

/// Upper bound of the histogram bucket holding quantile `q`.
fn quantile(c: &Counts, q: f64) -> u64 {
    let n = c[REQ];
    if n == 0 {
        return 0;
    }
    let target = ((n as f64) * q).ceil().max(1.0) as u64;
    let mut seen = 0;
    for i in 0..HIST_N {
        seen += c[HIST + i];
        if seen >= target {
            return (1u64 << (i + 6)).min(c[DUR_MAX].max(1));
        }
    }
    c[DUR_MAX]
}

impl From<&Counts> for Traffic {
    fn from(c: &Counts) -> Self {
        Traffic {
            requests: c[REQ],
            reads: c[READ],
            writes: c[WRITE],
            creates: c[CREATE],
            updates: c[UPDATE],
            deletes: c[DELETE],
            errors4xx: c[E4],
            errors5xx: c[E5],
            bytes_in: c[BYTES_IN],
            bytes_out: c[BYTES_OUT],
            avg_us: c[DUR].checked_div(c[REQ]).unwrap_or(0),
            max_us: c[DUR_MAX],
            p50_us: quantile(c, 0.50),
            p95_us: quantile(c, 0.95),
            p99_us: quantile(c, 0.99),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Point {
    /// Bucket start, unix seconds.
    pub t: i64,
    #[serde(flatten)]
    pub traffic: Traffic,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficSeries {
    pub total: Traffic,
    pub last_hour: Traffic,
    pub minutes: Vec<Point>,
    pub hours: Vec<Point>,
}

/// Per collection: reads (requests), creates, updates, deletes (changes).
type CollCounters = [AtomicU64; 4];

pub struct Metrics {
    since_us: i64,
    total: Slot,
    minutes: Box<[Slot]>,
    hours: Box<[Slot]>,
    colls: RwLock<HashMap<u32, Arc<CollCounters>>>,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            since_us: now_us(),
            total: Slot::new(u64::MAX),
            minutes: (0..MINUTES).map(|_| Slot::new(0)).collect(),
            hours: (0..HOURS).map(|_| Slot::new(0)).collect(),
            colls: RwLock::new(HashMap::new()),
        }
    }

    pub fn since_us(&self) -> i64 {
        self.since_us
    }

    fn slots(&self, now: i64) -> [&Slot; 3] {
        [
            &self.total,
            slot(&self.minutes, (now / MIN_US) as u64),
            slot(&self.hours, (now / HOUR_US) as u64),
        ]
    }

    pub fn record_request(
        &self,
        now: i64,
        kind: ReqKind,
        status: u16,
        dur_us: u64,
        bytes_in: u64,
        bytes_out: u64,
    ) {
        let h = HIST + hist_bucket(dur_us);
        for s in self.slots(now) {
            let c = &s.c;
            c[REQ].fetch_add(1, Relaxed);
            match kind {
                ReqKind::Read => c[READ].fetch_add(1, Relaxed),
                ReqKind::Write => c[WRITE].fetch_add(1, Relaxed),
                ReqKind::Other => 0,
            };
            match status {
                400..=499 => c[E4].fetch_add(1, Relaxed),
                500.. => c[E5].fetch_add(1, Relaxed),
                _ => 0,
            };
            c[BYTES_IN].fetch_add(bytes_in, Relaxed);
            c[BYTES_OUT].fetch_add(bytes_out, Relaxed);
            c[DUR].fetch_add(dur_us, Relaxed);
            c[DUR_MAX].fetch_max(dur_us, Relaxed);
            c[h].fetch_add(1, Relaxed);
        }
    }

    /// Committed record ops `[creates, updates, deletes]`.
    pub fn record_changes(&self, now: i64, ops: [u64; 3]) {
        for s in self.slots(now) {
            for (i, n) in ops.into_iter().enumerate() {
                if n > 0 {
                    s.c[CREATE + i].fetch_add(n, Relaxed);
                }
            }
        }
    }

    fn coll(&self, cid: u32) -> Arc<CollCounters> {
        if let Some(c) = self
            .colls
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&cid)
        {
            return c.clone();
        }
        self.colls
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .entry(cid)
            .or_insert_with(|| Arc::new(std::array::from_fn(|_| AtomicU64::new(0))))
            .clone()
    }

    /// Counts one read request against collection `cid`.
    pub fn record_coll_read(&self, cid: u32) {
        self.coll(cid)[0].fetch_add(1, Relaxed);
    }

    fn coll_counts(&self, cid: u32) -> [u64; 4] {
        self.colls
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&cid)
            .map_or([0; 4], |c| std::array::from_fn(|i| c[i].load(Relaxed)))
    }

    pub fn total(&self) -> Traffic {
        Traffic::from(&self.total.read())
    }

    fn ring(ring: &[Slot], cur: u64, width_us: i64) -> (Vec<Point>, Counts) {
        let mut sum = [0; N];
        let points = (0..ring.len() as u64)
            .map(|i| {
                let e = (cur + 1 + i).saturating_sub(ring.len() as u64);
                let s = &ring[e as usize % ring.len()];
                let c = if s.epoch.load(Relaxed) == e {
                    s.read()
                } else {
                    [0; N]
                };
                merge(&mut sum, &c);
                Point {
                    t: e as i64 * (width_us / 1_000_000),
                    traffic: Traffic::from(&c),
                }
            })
            .collect();
        (points, sum)
    }

    pub fn series(&self, now: i64) -> TrafficSeries {
        let (minutes, last_hour) = Self::ring(&self.minutes, (now / MIN_US) as u64, MIN_US);
        let (hours, _) = Self::ring(&self.hours, (now / HOUR_US) as u64, HOUR_US);
        TrafficSeries {
            total: self.total(),
            last_hour: Traffic::from(&last_hour),
            minutes,
            hours,
        }
    }
}

/// Feeds record op counters from one app's change bus; ends when the store closes.
pub(crate) fn spawn_change_counter(
    mut rx: broadcast::Receiver<ChangeBatch>,
    sinks: [Arc<Metrics>; 2],
) {
    tokio::spawn(async move {
        loop {
            let batch = match rx.recv().await {
                Ok(b) => b,
                // Lagged: those changes go uncounted; the log on disk is still complete.
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            };
            let now = now_us();
            let mut ops = [0u64; 3];
            let app = &sinks[0];
            for ch in batch.iter() {
                let i = match ch.op {
                    Op::Insert => 0,
                    Op::Update => 1,
                    Op::Delete => 2,
                };
                ops[i] += 1;
                app.coll(ch.coll)[1 + i].fetch_add(1, Relaxed);
            }
            sinks.iter().for_each(|m| m.record_changes(now, ops));
        }
    });
}

// ---------- stats views (superuser endpoints) ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreStatsOut {
    pub commits: u64,
    pub ops: u64,
    pub ops_failed: u64,
    pub mutations: u64,
    pub max_batch: u64,
    pub commit_avg_us: u64,
    pub commit_max_us: u64,
    pub reads: u64,
    pub read_avg_us: u64,
    pub busy: u64,
    pub writer_queue: u64,
    pub reader_queue: u64,
    pub readers: u64,
}

impl From<StoreStats> for StoreStatsOut {
    fn from(s: StoreStats) -> Self {
        Self {
            commits: s.commits,
            ops: s.ops,
            ops_failed: s.ops_failed,
            mutations: s.mutations,
            max_batch: s.max_batch,
            commit_avg_us: s.commit_us_total.checked_div(s.commits).unwrap_or(0),
            commit_max_us: s.commit_us_max,
            reads: s.reads,
            read_avg_us: s.read_us_total.checked_div(s.reads).unwrap_or(0),
            busy: s.busy,
            writer_queue: s.writer_queue,
            reader_queue: s.reader_queue,
            readers: s.readers,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionStats {
    pub name: String,
    pub records: Option<u64>,
    pub reads: u64,
    pub creates: u64,
    pub updates: u64,
    pub deletes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStats {
    pub app: String,
    pub since: String,
    pub schema_version: u64,
    pub traffic: TrafficSeries,
    pub store: StoreStatsOut,
    pub db_bytes: u64,
    pub wal_bytes: u64,
    pub collections: Vec<CollectionStats>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSummary {
    pub name: String,
    pub loaded: bool,
    pub db_bytes: u64,
    pub wal_bytes: u64,
    pub traffic: Option<Traffic>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStats {
    pub since: String,
    pub uptime_s: u64,
    pub memory_bytes: Option<u64>,
    pub apps_total: usize,
    pub apps_loaded: usize,
    pub traffic: TrafficSeries,
    pub apps: Vec<AppSummary>,
}

fn db_sizes(db: &Path) -> (u64, u64) {
    let len = |p: PathBuf| std::fs::metadata(p).map_or(0, |m| m.len());
    let mut wal = db.as_os_str().to_owned();
    wal.push("-wal");
    (len(db.to_owned()), len(wal.into()))
}

/// Resident set size from `/proc/self/status` (Linux); `None` elsewhere.
fn rss_bytes() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb = s.lines().find_map(|l| l.strip_prefix("VmRSS:"))?;
    kb.trim()
        .trim_end_matches("kB")
        .trim()
        .parse::<u64>()
        .ok()
        .map(|k| k * 1024)
}

fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> impl Future<Output = CoreResult<T>> {
    let h = tokio::task::spawn_blocking(f);
    async move { h.await.map_err(|e| CoreError::Internal(e.to_string())) }
}

impl App {
    /// Traffic, store internals, file sizes and per-collection counters.
    /// `exact` counts rows with `COUNT(*)`; otherwise `sqlite_stat1` estimates (or null).
    pub async fn stats(&self, exact: bool) -> CoreResult<AppStats> {
        let schema = self.schema();
        let mut names: Vec<_> = schema.collections.values().collect();
        names.sort_by(|a, b| a.def.name.cmp(&b.def.name));
        let mut estimates = HashMap::new();
        if !exact {
            for r in self
                .store()
                .query(Query {
                    sql: "SELECT tbl, CAST(substr(stat, 1, instr(stat || ' ', ' ') - 1) AS INTEGER) FROM sqlite_stat1".into(),
                    params: vec![],
                })
                .await
                // No sqlite_stat1 until the first ANALYZE/optimize: estimates are null.
                .unwrap_or_default()
            {
                if let [Value::Text(t), Value::Int(n)] = &r[..] {
                    estimates.entry(t.to_string()).or_insert(*n as u64);
                }
            }
        }
        let mut collections = Vec::with_capacity(names.len());
        for c in names {
            let records = if exact {
                Some(
                    self.count_records(&Principal::superuser(), &c.def.name, "", TotalMode::Exact)
                        .await?
                        .0,
                )
            } else {
                estimates.get(&c.def.name).copied()
            };
            let [reads, creates, updates, deletes] = self.metrics.coll_counts(c.cid);
            collections.push(CollectionStats {
                name: c.def.name.clone(),
                records,
                reads,
                creates,
                updates,
                deletes,
            });
        }
        let db = self.db_path.clone();
        let (db_bytes, wal_bytes) = blocking(move || db_sizes(&db)).await?;
        Ok(AppStats {
            app: self.name.clone(),
            since: format_us(self.metrics.since_us()),
            schema_version: schema.version,
            traffic: self.metrics.series(now_us()),
            store: self.store().stats().into(),
            db_bytes,
            wal_bytes,
            collections,
        })
    }
}

impl Registry {
    pub async fn system_stats(&self) -> CoreResult<SystemStats> {
        let infos = self.list_apps().await?;
        let dbs: Vec<PathBuf> = infos
            .iter()
            .map(|a| self.app_dir(&a.name).join("data.db"))
            .collect();
        let (sizes, memory_bytes) = blocking(move || {
            (
                dbs.iter().map(|p| db_sizes(p)).collect::<Vec<_>>(),
                rss_bytes(),
            )
        })
        .await?;
        let mut apps_loaded = 0;
        let apps = infos
            .into_iter()
            .zip(sizes)
            .map(|(a, (db_bytes, wal_bytes))| {
                let traffic = self.loaded_app(&a.name).map(|app| app.metrics.total());
                apps_loaded += usize::from(traffic.is_some());
                AppSummary {
                    name: a.name,
                    loaded: traffic.is_some(),
                    db_bytes,
                    wal_bytes,
                    traffic,
                }
            })
            .collect::<Vec<_>>();
        let now = now_us();
        let m = &self.metrics;
        Ok(SystemStats {
            since: format_us(m.since_us()),
            uptime_s: ((now - m.since_us()) / 1_000_000).max(0) as u64,
            memory_bytes,
            apps_total: apps.len(),
            apps_loaded,
            traffic: m.series(now),
            apps,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_and_quantiles() {
        assert_eq!(hist_bucket(0), 0);
        assert_eq!(hist_bucket(63), 0);
        assert_eq!(hist_bucket(64), 1);
        assert_eq!(hist_bucket(u64::MAX), HIST_N - 1);

        let m = Metrics::new();
        let t0 = 1_000 * HOUR_US;
        for i in 0..100 {
            let kind = if i % 2 == 0 {
                ReqKind::Read
            } else {
                ReqKind::Write
            };
            m.record_request(
                t0,
                kind,
                if i < 3 { 500 } else { 200 },
                100 + i * 10,
                10,
                20,
            );
        }
        m.record_changes(t0, [4, 2, 1]);
        let t = m.total();
        assert_eq!(
            (t.requests, t.reads, t.writes, t.errors5xx),
            (100, 50, 50, 3)
        );
        assert_eq!((t.creates, t.updates, t.deletes), (4, 2, 1));
        assert_eq!((t.bytes_in, t.bytes_out, t.max_us), (1000, 2000, 1090));
        assert!(t.p50_us >= 512 && t.p50_us <= 1024, "{t:?}");
        assert!(t.p99_us <= t.max_us);

        // The same minute shows in the series; a later minute rolls the ring.
        let s = m.series(t0);
        assert_eq!(s.minutes.len(), MINUTES);
        assert_eq!(s.hours.len(), HOURS);
        assert_eq!(s.minutes.last().unwrap().traffic.requests, 100);
        assert_eq!(s.minutes.last().unwrap().t, t0 / 1_000_000);
        assert_eq!(s.last_hour.requests, 100);

        let later = t0 + 2 * HOUR_US;
        m.record_request(later, ReqKind::Other, 404, 5, 0, 0);
        let s = m.series(later);
        assert_eq!(s.last_hour.requests, 1);
        assert_eq!(s.last_hour.errors4xx, 1);
        assert_eq!(s.hours.iter().map(|p| p.traffic.requests).sum::<u64>(), 101);
        assert_eq!(s.total.requests, 101);

        // A slot reused 60 minutes later starts from zero.
        let wrap = later + 60 * MIN_US;
        m.record_request(wrap, ReqKind::Read, 200, 5, 0, 0);
        assert_eq!(m.series(wrap).minutes.last().unwrap().traffic.requests, 1);
    }
}
