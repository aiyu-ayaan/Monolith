//! SQLite engine: one writer thread with group commit, a pool of reader threads
//! (doc 03 §3–§5). Blocking SQLite calls never run on a Tokio worker.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use rusqlite::types::{ToSqlOutput, ValueRef};
use rusqlite::{Connection, OpenFlags, ToSql, params_from_iter};
use tokio::sync::{broadcast, oneshot};

use crate::{
    AppStore, Change, ChangeBatch, Error, Id, MigrationPlan, Op, Query, Result, Row, SnapshotInfo,
    Value, WriteOp, WriteResult,
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

enum WriterMsg {
    Write(WriteOp, oneshot::Sender<Result<WriteResult>>),
    Schema(MigrationPlan, oneshot::Sender<Result<()>>),
}

type ReadJob = Box<dyn FnOnce(&Connection) + Send>;

/// Threads stop when the store is dropped and their queues drain.
pub struct SqliteStore {
    writer: Sender<WriterMsg>,
    readers: Sender<ReadJob>,
    bus: broadcast::Sender<ChangeBatch>,
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Engine(e.to_string())
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
            c.set_prepared_statement_cache_capacity(STMT_CACHE);
            reader_conns.push(c);
        }

        let (bus, _) = broadcast::channel(cfg.change_bus);
        let (writer, writer_rx) = bounded(cfg.writer_queue);
        let (readers, reader_rx) = bounded::<ReadJob>(cfg.reader_queue);

        let state = Writer {
            conn,
            bus: bus.clone(),
            max_batch: cfg.max_batch,
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
        })
    }

    async fn read<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        let job: ReadJob = Box::new(move |c| {
            let _ = tx.send(f(c));
        });
        // shortcut: fails fast when the pool queue is full; doc 03 §10 wants a bounded wait (read_timeout) first.
        self.readers.try_send(job).map_err(send_err)?;
        rx.await.map_err(|_| Error::Closed)?
    }

    async fn send_write<T>(
        &self,
        msg: impl FnOnce(oneshot::Sender<Result<T>>) -> WriterMsg,
    ) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        self.writer.try_send(msg(tx)).map_err(send_err)?;
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
            WriterMsg::Write(_, tx) => Reply::Write(tx, Err(Error::Closed)),
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
    conn: Connection,
    bus: broadcast::Sender<ChangeBatch>,
    max_batch: usize,
    next_seq: u64,
    hlc: u64,
}

impl Writer {
    // shortcut: a panic here ends the writer and later writes get Closed; doc 03 §10 restarts it.
    fn run(mut self, rx: Receiver<WriterMsg>) {
        let mut batch = Vec::with_capacity(self.max_batch);
        let mut replies = Vec::with_capacity(self.max_batch);
        let mut changes = Vec::new();
        while let Ok(first) = rx.recv() {
            batch.push(first);
            while batch.len() < self.max_batch {
                match rx.try_recv() {
                    Ok(m) => batch.push(m),
                    Err(_) => break,
                }
            }
            self.commit_batch(&mut batch, &mut replies, &mut changes);
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
            let reply = match m {
                WriterMsg::Write(op, tx) => {
                    let r = sp
                        .map_err(Error::from)
                        .and_then(|()| self.apply(&op, ts, changes));
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
            let end = if ok {
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
        self.next_seq = start_seq;
        self.hlc = start_hlc;
        changes.clear();
        let msg = e.to_string();
        replies.drain(..).for_each(|r| r.fail(&msg));
    }

    fn apply(&mut self, op: &WriteOp, ts: i64, changes: &mut Vec<Change>) -> Result<WriteResult> {
        let first_seq = self.next_seq;
        for m in &op.mutations {
            let n = self
                .conn
                .prepare_cached(&m.sql)?
                .execute(params_from_iter(m.params.iter()))?;
            if n == 0 && m.op != Op::Insert {
                return Err(Error::NotFound);
            }
            let seq = self.next_seq;
            self.next_seq += 1;
            self.hlc = next_hlc(self.hlc, ts);
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
        })
    }
}

impl AppStore for SqliteStore {
    async fn write(&self, op: WriteOp) -> Result<WriteResult> {
        if op.mutations.is_empty() {
            return Err(Error::Invalid("write has no mutations"));
        }
        self.send_write(|tx| WriterMsg::Write(op, tx)).await
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
