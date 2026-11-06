//! Rules and filter language (doc 06).
//!
//! `compile` turns rule text into a typed IR once per schema version. The IR yields
//! a parameterized SQL `WHERE` fragment (stable text, so `prepare_cached` always hits)
//! and an in-memory predicate that must agree with SQLite on every row (differential
//! tests in `tests/differential.rs`).

mod compile;
mod eval;
pub mod parse;
mod time;

pub use compile::{Compiled, compile};
pub use monolith_store::{Id, Value};
pub use time::parse_datetime_us;

/// Value types the language knows. Storage: Text=TEXT, Number=INTEGER|REAL,
/// Bool=INTEGER 0/1, Date=INTEGER µs UTC, Id=BLOB(16) (doc 05 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Text,
    Number,
    Bool,
    Date,
    Id,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    Scalar(Ty),
    /// Single relation (`multi = false`, Id column) or multi-relation (`multi = true`,
    /// `_rel_<collection>_<field>` junction table) pointing at `target.id`.
    Relation {
        target: String,
        multi: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInfo {
    /// Index of this field in the row slice given to the predicate.
    pub slot: usize,
    pub kind: FieldKind,
    /// Superuser-only; client filters may not reference it.
    pub hidden: bool,
}

/// Schema lookup. Names are validated identifiers, so they are safe to quote in SQL.
pub trait SchemaView {
    fn field(&self, collection: &str, name: &str) -> Option<FieldInfo>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Admin-authored rule: hidden fields allowed.
    Rule,
    /// Client `filter` parameter.
    Filter,
}

/// Static class (doc 06 §4), ordered by evaluation cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Const,
    AuthOnly,
    RowLocal,
    Relational,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Auth {
    pub id: Id,
    pub collection: Box<str>,
    /// Cached auth fields carried in the token (doc 08 §2).
    pub claims: Vec<(Box<str>, Value)>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Ctx<'a> {
    pub auth: Option<&'a Auth>,
    /// `@request.body.*` values by field slot (write rules).
    pub body: Option<&'a [Value]>,
    pub now_us: i64,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RuleError {
    #[error("syntax error at {pos}: {msg}")]
    Syntax { pos: usize, msg: String },
    #[error("unknown field '{0}'")]
    UnknownField(String),
    #[error("type error: {0}")]
    Type(String),
    #[error("not supported: {0}")]
    Unsupported(&'static str),
    #[error("limit exceeded: {0}")]
    Limit(&'static str),
}
