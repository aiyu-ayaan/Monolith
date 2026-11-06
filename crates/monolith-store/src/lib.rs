//! Storage seam for one app database (doc 03 §6).
//!
//! Everything above [`AppStore`] (HTTP, rules, realtime, sync) stays engine-agnostic.
//! The engine is picked by the Phase 0 bake-off (doc 04); [`sqlite`] is the lead.

pub mod id;
pub mod sqlite;

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::broadcast;

pub use sqlite::{SqliteConfig, SqliteStore};

/// UUIDv7 as 16 raw bytes (doc 05 §2).
pub type Id = [u8; 16];

/// A bound SQL parameter or a column value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(Box<str>),
    Blob(Box<[u8]>),
}

/// Change-log operation codes, stored as-is in `_changes.op`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Op {
    Insert = 1,
    Update = 2,
    Delete = 3,
}

impl Op {
    fn from_code(code: i64) -> Option<Self> {
        match code {
            1 => Some(Self::Insert),
            2 => Some(Self::Update),
            3 => Some(Self::Delete),
            _ => None,
        }
    }
}

/// One record mutation. `sql` is a compiled template (same `Arc` per shape), never
/// built per request; the engine caches its prepared statement by text.
#[derive(Debug, Clone)]
pub struct Mutation {
    pub op: Op,
    pub coll: u32,
    pub rid: Id,
    /// Bitmap of changed field slots; `None` means all fields.
    pub fields: Option<u64>,
    pub sql: Arc<str>,
    pub params: Vec<Value>,
    /// Param index the writer overwrites with this mutation's seq (the `_v` column).
    pub seq_slot: Option<usize>,
    /// Param index the writer overwrites with this mutation's HLC (the `_hlc` column).
    pub hlc_slot: Option<usize>,
    /// Auxiliary statements (e.g. `_rel_*` junction table updates) run in the same
    /// savepoint only after `sql` affects a row, without advancing `seq`.
    pub post_sql: Vec<(Arc<str>, Vec<Value>)>,
}

/// All mutations of one `WriteOp` commit or fail together.
#[derive(Debug, Clone)]
pub struct WriteOp {
    pub actor: Option<Id>,
    pub mutations: Vec<Mutation>,
}

/// The op's mutations got the contiguous seqs `first_seq..=last_seq`.
#[derive(Debug, Clone, PartialEq)]
pub struct WriteResult {
    pub first_seq: u64,
    pub last_seq: u64,
    /// Rows from `RETURNING` clauses, in mutation order.
    pub returned: Vec<Row>,
}

#[derive(Debug, Clone)]
pub struct Query {
    pub sql: Arc<str>,
    pub params: Vec<Value>,
}

// shortcut: rows are materialized; Phase 1 streams column values straight to JSON (doc 11 §4).
pub type Row = Vec<Value>;

/// One committed `_changes` row (doc 05 §8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub seq: u64,
    pub ts: i64,
    pub coll: u32,
    pub rid: Id,
    pub op: Op,
    pub hlc: u64,
    pub actor: Option<Id>,
    pub fields: Option<u64>,
}

// shortcut: raw DDL; typed steps (AddColumn, RebuildTable, ...) arrive with the Phase 1 planner (doc 05 §9).
#[derive(Debug, Clone)]
pub struct MigrationPlan {
    pub statements: Vec<String>,
}

/// Engine counters since open (doc 16 §5). Plain numbers; callers serialize.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StoreStats {
    pub commits: u64,
    pub ops: u64,
    pub ops_failed: u64,
    pub mutations: u64,
    pub max_batch: u64,
    pub commit_us_total: u64,
    pub commit_us_max: u64,
    pub reads: u64,
    pub read_us_total: u64,
    pub busy: u64,
    pub writer_queue: u64,
    pub reader_queue: u64,
    pub readers: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotInfo {
    pub bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Queue full. HTTP maps this to 503 + `Retry-After`.
    #[error("store busy")]
    Busy,
    #[error("store closed")]
    Closed,
    /// The mutation changed no row: missing record or a rule in its WHERE rejected it.
    #[error("record not found")]
    NotFound,
    /// UNIQUE, PRIMARY KEY, NOT NULL, CHECK or FOREIGN KEY violation.
    #[error("constraint violation: {0}")]
    Constraint(String),
    #[error("invalid request: {0}")]
    Invalid(&'static str),
    #[error("engine error: {0}")]
    Engine(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Committed changes of one group commit, in seq order.
pub type ChangeBatch = Arc<[Change]>;

pub trait AppStore: Send + Sync {
    /// Runs through the single writer; batched with concurrent writes (group commit).
    fn write(&self, op: WriteOp) -> impl Future<Output = Result<WriteResult>> + Send;
    /// Runs `op` inside a rolled-back savepoint, returning what `write` would have produced.
    fn write_dry_run(&self, op: WriteOp) -> impl Future<Output = Result<WriteResult>> + Send;
    fn query(&self, q: Query) -> impl Future<Output = Result<Vec<Row>>> + Send;
    /// Changes with `seq > after`, ascending, at most `limit`.
    fn changes_since(
        &self,
        after: u64,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<Change>>> + Send;
    fn apply_schema(&self, plan: MigrationPlan) -> impl Future<Output = Result<()>> + Send;
    /// Consistent copy of the database to `dest`, which must not exist.
    fn snapshot(&self, dest: PathBuf) -> impl Future<Output = Result<SnapshotInfo>> + Send;
    /// Change bus feed. A lagging receiver catches up with `changes_since`.
    fn subscribe(&self) -> broadcast::Receiver<ChangeBatch>;
}
