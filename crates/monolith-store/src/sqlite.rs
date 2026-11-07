//! SQLite engine: one writer thread with group commit, a pool of reader threads
//! (doc 03 §3–§5). Blocking SQLite calls never run on a Tokio worker.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossbeam_channel::{
    Receiver, RecvTimeoutError, SendTimeoutError, Sender, TrySendError, bounded,
};
use rusqlite::types::{ToSqlOutput, ValueRef};
use rusqlite::{Connection, OpenFlags, ToSql, params_from_iter};
use tokio::sync::{broadcast, oneshot};

use crate::{
    AppStore, Change, ChangeBatch, Error, Id, MigrationPlan, Op, Query, Result, Row, SnapshotInfo,
    StoreStats, Value, WriteOp, WriteResult,
};

#[derive(Debug, Clone)]
pub struct SqliteConfig {
    pub readers: usize,
    pub writer_queue: usize,
    pub reader_queue: usize,
    /// Max ops per group commit. 1 turns group commit off (bake-off baseline).
    pub max_batch: usize,
    /// `synchronous=FULL` instead of `NORMAL` (open question Q3, doc 20).
    pub durability_full: bool,
    /// Only applies when the file is created.
    pub page_size: u32,
    pub change_bus: usize,
    /// Bounded wait when the reader queue is full or a reader hits a lock (doc 03 §10, doc 06 §8).
    pub read_timeout: Duration,
    /// Idle period before the writer runs `PRAGMA optimize` and requests a checkpoint.
    pub idle_checkpoint: Duration,
}

impl Default for SqliteConfig {
    fn default() -> Self {
        let cores = thread::available_parallelism().map_or(4, |n| n.get());
        Self {
            readers: (cores * 2).min(16),
            writer_queue: 4096,
            reader_queue: 4096,
            max_batch: 256,
            durability_full: false,
            page_size: 4096,
            change_bus: 1024,
            read_timeout: Duration::from_secs(2),
            idle_checkpoint: Duration::from_secs(30),
        }
    }
}

// Doc 04 §5 starting point; tuned by the bake-off.
const SHARED_PRAGMAS: &str = "
PRAGMA temp_store = MEMORY;
PRAGMA cache_size = -16000;
PRAGMA mmap_size = 1073741824;
PRAGMA trusted_schema = OFF;
";

const WRITER_PRAGMAS: &str = "
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 5000;
PRAGMA foreign_keys = ON;
PRAGMA wal_autocheckpoint = 4000;
PRAGMA journal_size_limit = 134217728;
PRAGMA cell_size_check = OFF;
PRAGMA optimize = 0x10002;
";

const CHANGES_DDL: &str = "
CREATE TABLE IF NOT EXISTS _changes (
  seq     INTEGER PRIMARY KEY,
  ts      INTEGER NOT NULL,
  coll    INTEGER NOT NULL,
  rid     BLOB NOT NULL,
  op      INTEGER NOT NULL,
  hlc     INTEGER NOT NULL,
  actor   BLOB,
  fields  INTEGER,
  patch   BLOB
) STRICT;
";

const INSERT_CHANGE: &str = "INSERT INTO _changes (seq, ts, coll, rid, op, hlc, actor, fields) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)";
const SELECT_CHANGES: &str = "SELECT seq, ts, coll, rid, op, hlc, actor, fields FROM _changes WHERE seq > ?1 ORDER BY seq LIMIT ?2";

const STMT_CACHE: usize = 256;

/// A busy writer asks for a background checkpoint at most this often.
const CHECKPOINT_GAP: Duration = Duration::from_millis(100);

enum WriterMsg {
    Write {
        op: WriteOp,
        dry_run: bool,
        tx: oneshot::Sender<Result<WriteResult>>,
    },
    Schema(MigrationPlan, oneshot::Sender<Result<()>>),
}

type ReadJob = Box<dyn FnOnce(&Connection) + Send>;

/// Relaxed atomics: a few ns per op, off the SQLite critical section.
#[derive(Default)]
struct Counters {
    commits: AtomicU64,
    ops: AtomicU64,
    ops_failed: AtomicU64,
    mutations: AtomicU64,
    max_batch: AtomicU64,
    commit_us: AtomicU64,
    commit_us_max: AtomicU64,
    reads: AtomicU64,
    read_us: AtomicU64,
    busy: AtomicU64,
}

/// Threads stop when the store is dropped and their queues drain.
pub struct SqliteStore {
    writer: Sender<WriterMsg>,
    readers: Sender<ReadJob>,
    bus: broadcast::Sender<ChangeBatch>,
    read_timeout: Duration,
    n_readers: usize,
    stats: Arc<Counters>,
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        match e.sqlite_error_code() {
            Some(rusqlite::ErrorCode::ConstraintViolation) => Error::Constraint(e.to_string()),
            _ => Error::Engine(e.to_string()),
        }
    }
}

impl ToSql for Value {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(match self {
            Value::Null => ValueRef::Null,
            Value::Int(v) => ValueRef::Integer(*v),
            Value::Real(v) => ValueRef::Real(*v),
            Value::Text(v) => ValueRef::Text(v.as_bytes()),
            Value::Blob(v) => ValueRef::Blob(v),
        }))
    }
}

fn to_value(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::Int(i),
        ValueRef::Real(f) => Value::Real(f),
        ValueRef::Text(t) => Value::Text(String::from_utf8_lossy(t).into()),
        ValueRef::Blob(b) => Value::Blob(b.into()),
    }
}

fn now_us() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_micros() as i64)
}

/// HLC: 48-bit physical ms + 16-bit counter (doc 09), strictly increasing.
fn next_hlc(last: u64, now_us: i64) -> u64 {
    (((now_us / 1000) as u64) << 16).max(last + 1)
}

fn blob_id(v: Option<Vec<u8>>) -> Option<Id> {
    v.and_then(|b| b.try_into().ok())
}

impl SqliteStore {
    /// Opens or creates the app database and starts its threads. Blocking: call at
    /// startup or from `spawn_blocking`.
    pub fn open(path: &Path, cfg: &SqliteConfig) -> Result<Self> {
        if cfg.readers == 0 || cfg.max_batch == 0 {
            return Err(Error::Invalid("readers and max_batch must be > 0"));
        }
        let conn = Connection::open(path)?;
        // page_size must be set before the first table exists; runs once per open, not per request.
        conn.execute_batch(&format!("PRAGMA page_size = {};", cfg.page_size))?;
        conn.execute_batch(WRITER_PRAGMAS)?;
        conn.execute_batch(SHARED_PRAGMAS)?;
        conn.execute_batch(if cfg.durability_full {
            "PRAGMA synchronous = FULL;"
        } else {
            "PRAGMA synchronous = NORMAL;"
        })?;
        conn.execute_batch(CHANGES_DDL)?;
        conn.set_prepared_statement_cache_capacity(STMT_CACHE);
        let (last_seq, last_hlc): (i64, i64) = conn.query_row(
            "SELECT coalesce(max(seq), 0), coalesce(max(hlc), 0) FROM _changes",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;

        let mut reader_conns = Vec::with_capacity(cfg.readers);
        for _ in 0..cfg.readers {
            let c = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            c.execute_batch(SHARED_PRAGMAS)?;
            c.busy_timeout(cfg.read_timeout)?;
            c.set_prepared_statement_cache_capacity(STMT_CACHE);
            reader_conns.push(c);
        }

        let (bus, _) = broadcast::channel(cfg.change_bus);
        let (writer, writer_rx) = bounded(cfg.writer_queue);
        let (readers, reader_rx) = bounded::<ReadJob>(cfg.reader_queue);

        let stats = Arc::new(Counters::default());
        let state = Writer {
            stats: stats.clone(),
            conn,
            bus: bus.clone(),
            max_batch: cfg.max_batch,
            idle_checkpoint: cfg.idle_checkpoint,
            ckpt: Arc::new(Checkpoint {
                path: path.to_owned(),
                pending: AtomicBool::new(false),
            }),
            last_ckpt: Instant::now(),
            next_seq: last_seq as u64 + 1,
            hlc: last_hlc as u64,
        };
        spawn("monolith-writer", move || state.run(writer_rx))?;
        for (i, c) in reader_conns.into_iter().enumerate() {
            let rx = reader_rx.clone();
            spawn(&format!("monolith-reader-{i}"), move || reader_loop(c, rx))?;
        }
        Ok(Self {
            writer,
            readers,
            bus,
            read_timeout: cfg.read_timeout,
            n_readers: cfg.readers,
            stats,
        })
    }

    /// Counter snapshot plus current queue depths.
    pub fn stats(&self) -> StoreStats {
        let c = &*self.stats;
        StoreStats {
            commits: c.commits.load(Relaxed),
            ops: c.ops.load(Relaxed),
            ops_failed: c.ops_failed.load(Relaxed),
            mutations: c.mutations.load(Relaxed),
            max_batch: c.max_batch.load(Relaxed),
            commit_us_total: c.commit_us.load(Relaxed),
            commit_us_max: c.commit_us_max.load(Relaxed),
            reads: c.reads.load(Relaxed),
            read_us_total: c.read_us.load(Relaxed),
            busy: c.busy.load(Relaxed),
            writer_queue: self.writer.len() as u64,
            reader_queue: self.readers.len() as u64,
            readers: self.n_readers as u64,
        }
    }

    fn count_busy<T>(&self, r: Result<T>) -> Result<T> {
        if matches!(r, Err(Error::Busy)) {
            self.stats.busy.fetch_add(1, Relaxed);
        }
        r
    }

    async fn read<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        let stats = self.stats.clone();
        let job: ReadJob = Box::new(move |c| {
            let t = Instant::now();
            let r = f(c);
            stats.reads.fetch_add(1, Relaxed);
            stats
                .read_us
                .fetch_add(t.elapsed().as_micros() as u64, Relaxed);
            let _ = tx.send(r);
        });
        let sent = self.count_busy(self.enqueue_read(job).await);
        sent?;
        rx.await.map_err(|_| Error::Closed)?
    }

    async fn enqueue_read(&self, job: ReadJob) -> Result<()> {
        match self.readers.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(job)) if !self.read_timeout.is_zero() => {
                let readers = self.readers.clone();
                let timeout = self.read_timeout;
                tokio::task::spawn_blocking(move || match readers.send_timeout(job, timeout) {
                    Ok(()) => Ok(()),
                    Err(SendTimeoutError::Timeout(_)) => Err(Error::Busy),
                    Err(SendTimeoutError::Disconnected(_)) => Err(Error::Closed),
                })
                .await
                .map_err(|_| Error::Closed)??;
            }
            Err(e) => return Err(send_err(e)),
        }
        Ok(())
    }

    async fn send_write<T>(
        &self,
        msg: impl FnOnce(oneshot::Sender<Result<T>>) -> WriterMsg,
    ) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        self.count_busy(self.writer.try_send(msg(tx)).map_err(send_err))?;
        rx.await.map_err(|_| Error::Closed)?
    }
}

fn send_err<T>(e: TrySendError<T>) -> Error {
    match e {
        TrySendError::Full(_) => Error::Busy,
        TrySendError::Disconnected(_) => Error::Closed,
    }
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) -> Result<()> {
    thread::Builder::new()
        .name(name.into())
        .spawn(f)
        .map(drop)
        .map_err(|e| Error::Engine(e.to_string()))
}

fn reader_loop(conn: Connection, rx: Receiver<ReadJob>) {
    while let Ok(job) = rx.recv() {
        // A panicking job drops its reply (caller sees Closed); the reader stays up.
        let _ = catch_unwind(AssertUnwindSafe(|| job(&conn)));
    }
}

/// One store's checkpoint request; `pending` keeps at most one in the queue.
struct Checkpoint {
    path: PathBuf,
    pending: AtomicBool,
}

/// Background WAL checkpoints (doc 04 §5) on one thread shared by every store. A checkpoint
/// fsyncs the WAL and the database file; on the writer thread that stalled every queued write
/// (~1 s seen under load). PASSIVE never blocks the writer or readers. The writer keeps its
/// own `wal_autocheckpoint` as the backstop that lets the WAL restart under nonstop writes;
/// by then the background pass has copied most frames. A short-lived connection per pass
/// costs well under the fsync and keeps idle apps from holding one more connection.
fn request_checkpoint(c: &Arc<Checkpoint>) {
    static WORKER: OnceLock<Option<Sender<Arc<Checkpoint>>>> = OnceLock::new();
    let worker = WORKER.get_or_init(|| {
        let (tx, rx) = bounded::<Arc<Checkpoint>>(1024);
        spawn("monolith-ckpt", move || {
            while let Ok(c) = rx.recv() {
                c.pending.store(false, Relaxed);
                // No CREATE flag: an app deleted meanwhile must not come back as an empty file.
                let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
                if let Ok(conn) = Connection::open_with_flags(&c.path, flags) {
                    let _ = conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);");
                }
            }
        })
        .ok()
        .map(|()| tx)
    });
    if let Some(tx) = worker
        && !c.pending.swap(true, Relaxed)
        && tx.try_send(c.clone()).is_err()
    {
        c.pending.store(false, Relaxed);
    }
}

fn run_query(conn: &Connection, q: &Query) -> Result<Vec<Row>> {
    let mut stmt = conn.prepare_cached(&q.sql)?;
    let cols = stmt.column_count();
    let mut rows = stmt.query(params_from_iter(q.params.iter()))?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        let mut row = Vec::with_capacity(cols);
        for i in 0..cols {
            row.push(to_value(r.get_ref(i)?));
        }
        out.push(row);
    }
    Ok(out)
}

fn read_changes(conn: &Connection, after: u64, limit: u32) -> Result<Vec<Change>> {
    let mut stmt = conn.prepare_cached(SELECT_CHANGES)?;
    let rows = stmt.query_map((after as i64, limit), |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, u32>(2)?,
            r.get::<_, Vec<u8>>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, Option<Vec<u8>>>(6)?,
            r.get::<_, Option<i64>>(7)?,
        ))
    })?;
    rows.map(|r| {
        let (seq, ts, coll, rid, op, hlc, actor, fields) = r?;
        Ok(Change {
            seq: seq as u64,
            ts,
            coll,
            rid: blob_id(Some(rid)).ok_or(Error::Engine("bad rid in _changes".into()))?,
            op: Op::from_code(op).ok_or(Error::Engine("bad op in _changes".into()))?,
            hlc: hlc as u64,
            actor: blob_id(actor),
            fields: fields.map(|f| f as u64),
        })
    })
    .collect()
}

enum Reply {
    Write(oneshot::Sender<Result<WriteResult>>, Result<WriteResult>),
    Schema(oneshot::Sender<Result<()>>, Result<()>),
}

impl Reply {
    /// Placeholder for an op that never ran; it is always `fail`ed.
    fn unrun(m: WriterMsg) -> Self {
        match m {
            WriterMsg::Write { tx, .. } => Reply::Write(tx, Err(Error::Closed)),
            WriterMsg::Schema(_, tx) => Reply::Schema(tx, Err(Error::Closed)),
        }
    }

    fn send(self) {
        match self {
            Reply::Write(tx, r) => drop(tx.send(r)),
            Reply::Schema(tx, r) => drop(tx.send(r)),
        }
    }

    fn fail(self, msg: &str) {
        match self {
            Reply::Write(tx, _) => drop(tx.send(Err(Error::Engine(msg.into())))),
            Reply::Schema(tx, _) => drop(tx.send(Err(Error::Engine(msg.into())))),
        }
    }
}

struct Writer {
    stats: Arc<Counters>,
    conn: Connection,
    bus: broadcast::Sender<ChangeBatch>,
    max_batch: usize,
    idle_checkpoint: Duration,
    ckpt: Arc<Checkpoint>,
    last_ckpt: Instant,
    next_seq: u64,
    hlc: u64,
}

impl Writer {
    fn run(mut self, rx: Receiver<WriterMsg>) {
        let mut batch = Vec::with_capacity(self.max_batch);
        let mut replies = Vec::with_capacity(self.max_batch);
        let mut changes = Vec::new();
        let mut dirty = false;
        loop {
            let first = if dirty && !self.idle_checkpoint.is_zero() {
                match rx.recv_timeout(self.idle_checkpoint) {
                    Ok(m) => m,
                    Err(RecvTimeoutError::Timeout) => {
                        let _ = self.conn.execute_batch("PRAGMA optimize;");
                        request_checkpoint(&self.ckpt);
                        dirty = false;
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            } else {
                match rx.recv() {
                    Ok(m) => m,
                    Err(_) => break,
                }
            };
            batch.push(first);
            while batch.len() < self.max_batch {
                match rx.try_recv() {
                    Ok(m) => batch.push(m),
                    Err(_) => break,
                }
            }
            // Recover from a panicking batch by rolling back and resyncing seq/hlc from disk.
            if catch_unwind(AssertUnwindSafe(|| {
                self.commit_batch(&mut batch, &mut replies, &mut changes);
            }))
            .is_err()
            {
                let _ = self.conn.execute_batch("ROLLBACK");
                if let Ok((s, h)) = self.conn.query_row(
                    "SELECT coalesce(max(seq), 0), coalesce(max(hlc), 0) FROM _changes",
                    [],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
                ) {
                    self.next_seq = s as u64 + 1;
                    self.hlc = h as u64;
                }
                changes.clear();
                batch
                    .drain(..)
                    .for_each(|m| Reply::unrun(m).fail("writer panic recovered"));
                replies
                    .drain(..)
                    .for_each(|r| r.fail("writer panic recovered"));
            } else {
                dirty = true;
            }
        }
    }

    /// One transaction for the batch; each op in its own savepoint so a bad op
    /// never poisons the others (doc 03 §4).
    fn commit_batch(
        &mut self,
        batch: &mut Vec<WriterMsg>,
        replies: &mut Vec<Reply>,
        changes: &mut Vec<Change>,
    ) {
        let start_seq = self.next_seq;
        let start_hlc = self.hlc;
        let ts = now_us();
        let started = Instant::now();
        let n_ops = batch.len() as u64;
        if let Err(e) = self.conn.execute_batch("BEGIN IMMEDIATE") {
            let msg = e.to_string();
            batch.drain(..).for_each(|m| Reply::unrun(m).fail(&msg));
            return;
        }
        let mut broken = None;
        for m in batch.drain(..) {
            if broken.is_some() {
                replies.push(Reply::unrun(m));
                continue;
            }
            let seq = self.next_seq;
            let hlc = self.hlc;
            let n_changes = changes.len();
            let sp = self.conn.execute_batch("SAVEPOINT op");
            let mut dry_run = false;
            let reply = match m {
                WriterMsg::Write {
                    mut op,
                    dry_run: dry,
                    tx,
                } => {
                    dry_run = dry;
                    let r = sp
                        .map_err(Error::from)
                        .and_then(|()| self.apply(&mut op, ts, changes));
                    Reply::Write(tx, r)
                }
                WriterMsg::Schema(plan, tx) => {
                    let r = sp.map_err(Error::from).and_then(|()| {
                        plan.statements
                            .iter()
                            .try_for_each(|s| self.conn.execute_batch(s))
                            .map_err(Error::from)
                    });
                    Reply::Schema(tx, r)
                }
            };
            let ok = matches!(&reply, Reply::Write(_, Ok(_)) | Reply::Schema(_, Ok(())));
            let end = if ok && !dry_run {
                self.conn.execute_batch("RELEASE op")
            } else {
                self.next_seq = seq;
                self.hlc = hlc;
                changes.truncate(n_changes);
                self.conn.execute_batch("ROLLBACK TO op; RELEASE op")
            };
            replies.push(reply);
            // The savepoint stack is broken; abandon the whole transaction.
            broken = end.err();
        }
        if let Some(e) = broken {
            return self.abort(e, start_seq, start_hlc, replies, changes);
        }
        if let Err(e) = self.conn.execute_batch("COMMIT") {
            return self.abort(e, start_seq, start_hlc, replies, changes);
        }
        let s = &*self.stats;
        let us = started.elapsed().as_micros() as u64;
        let failed = replies
            .iter()
            .filter(|r| matches!(r, Reply::Write(_, Err(_)) | Reply::Schema(_, Err(_))))
            .count() as u64;
        s.commits.fetch_add(1, Relaxed);
        s.ops.fetch_add(n_ops, Relaxed);
        s.ops_failed.fetch_add(failed, Relaxed);
        s.mutations.fetch_add(changes.len() as u64, Relaxed);
        s.max_batch.fetch_max(n_ops, Relaxed);
        s.commit_us.fetch_add(us, Relaxed);
        s.commit_us_max.fetch_max(us, Relaxed);
        if self.last_ckpt.elapsed() >= CHECKPOINT_GAP {
            self.last_ckpt = Instant::now();
            request_checkpoint(&self.ckpt);
        }
        replies.drain(..).for_each(Reply::send);
        if !changes.is_empty() {
            // No receivers is fine: consumers can always read the persisted log.
            let _ = self.bus.send(Arc::from(changes.as_slice()));
            changes.clear();
        }
    }

    fn abort(
        &mut self,
        e: rusqlite::Error,
        start_seq: u64,
        start_hlc: u64,
        replies: &mut Vec<Reply>,
        changes: &mut Vec<Change>,
    ) {
        let _ = self.conn.execute_batch("ROLLBACK");
        let n = replies.len() as u64;
        self.stats.ops.fetch_add(n, Relaxed);
        self.stats.ops_failed.fetch_add(n, Relaxed);
        self.next_seq = start_seq;
        self.hlc = start_hlc;
        changes.clear();
        let msg = e.to_string();
        replies.drain(..).for_each(|r| r.fail(&msg));
    }

    fn apply(
        &mut self,
        op: &mut WriteOp,
        ts: i64,
        changes: &mut Vec<Change>,
    ) -> Result<WriteResult> {
        let first_seq = self.next_seq;
        let mut returned = Vec::new();
        for m in &mut op.mutations {
            let seq = self.next_seq;
            let hlc = next_hlc(self.hlc, ts);
            for (slot, v) in [(m.seq_slot, seq as i64), (m.hlc_slot, hlc as i64)] {
                if let Some(p) = slot.and_then(|i| m.params.get_mut(i)) {
                    *p = Value::Int(v);
                }
            }
            {
                let mut stmt = self.conn.prepare_cached(&m.sql)?;
                let cols = stmt.column_count();
                let mut rows = stmt.query(params_from_iter(m.params.iter()))?;
                while let Some(r) = rows.next()? {
                    let mut row = Vec::with_capacity(cols);
                    for i in 0..cols {
                        row.push(to_value(r.get_ref(i)?));
                    }
                    returned.push(row);
                }
            }
            // Zero rows: missing record, or a rule in the statement's WHERE said no.
            if self.conn.changes() == 0 {
                return Err(Error::NotFound);
            }
            for (aux_sql, aux_params) in &m.post_sql {
                self.conn
                    .prepare_cached(aux_sql)?
                    .execute(params_from_iter(aux_params.iter()))?;
            }
            if m.coll == crate::NO_CHANGE_LOG {
                continue;
            }
            self.next_seq += 1;
            self.hlc = hlc;
            self.conn.prepare_cached(INSERT_CHANGE)?.execute((
                seq as i64,
                ts,
                m.coll,
                &m.rid[..],
                m.op as u8,
                self.hlc as i64,
                op.actor.as_ref().map(|a| &a[..]),
                m.fields.map(|f| f as i64),
            ))?;
            changes.push(Change {
                seq,
                ts,
                coll: m.coll,
                rid: m.rid,
                op: m.op,
                hlc: self.hlc,
                actor: op.actor,
                fields: m.fields,
            });
        }
        Ok(WriteResult {
            first_seq,
            last_seq: self.next_seq - 1,
            returned,
        })
    }
}

impl AppStore for SqliteStore {
    async fn write(&self, op: WriteOp) -> Result<WriteResult> {
        if op.mutations.is_empty() {
            return Err(Error::Invalid("write has no mutations"));
        }
        self.send_write(|tx| WriterMsg::Write {
            op,
            dry_run: false,
            tx,
        })
        .await
    }

    async fn write_dry_run(&self, op: WriteOp) -> Result<WriteResult> {
        if op.mutations.is_empty() {
            return Err(Error::Invalid("write has no mutations"));
        }
        self.send_write(|tx| WriterMsg::Write {
            op,
            dry_run: true,
            tx,
        })
        .await
    }

    async fn query(&self, q: Query) -> Result<Vec<Row>> {
        self.read(move |c| run_query(c, &q)).await
    }

    async fn changes_since(&self, after: u64, limit: u32) -> Result<Vec<Change>> {
        self.read(move |c| read_changes(c, after, limit)).await
    }

    async fn apply_schema(&self, plan: MigrationPlan) -> Result<()> {
        self.send_write(|tx| WriterMsg::Schema(plan, tx)).await
    }

    async fn snapshot(&self, dest: PathBuf) -> Result<SnapshotInfo> {
        // VACUUM INTO reads one consistent snapshot on a reader; the writer keeps going.
        self.read(move |c| {
            let to = dest
                .to_str()
                .ok_or(Error::Invalid("snapshot path is not UTF-8"))?;
            c.execute("VACUUM INTO ?1", [to])?;
            let bytes = std::fs::metadata(&dest)
                .map_err(|e| Error::Engine(e.to_string()))?
                .len();
            Ok(SnapshotInfo { bytes })
        })
        .await
    }

    fn subscribe(&self) -> broadcast::Receiver<ChangeBatch> {
        self.bus.subscribe()
    }
}
