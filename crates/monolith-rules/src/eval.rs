//! In-memory predicate (doc 06 §6). Must match SQLite on every input; the
//! differential suite in `tests/differential.rs` enforces it.

use std::cmp::Ordering;

use crate::compile::{Compiled, Node, Term};
use crate::parse::CmpOp;
use crate::{Class, Value};

impl Compiled {
    /// Evaluates against a row (values by field slot) and the slice from `bind()`.
    /// `None` for `Relational` rules, which need SQL.
    // shortcut: walks the IR; doc 06 §6 allows closures or bytecode later, profile first.
    pub fn eval(&self, row: &[Value], bound: &[Value]) -> Option<bool> {
        if self.class == Class::Relational {
            return None;
        }
        Some(node(&self.node, row, bound))
    }
}

// The language has no NOT, so SQL's NULL (unknown) can collapse to false at every
// leaf without changing AND/OR results.
fn node(n: &Node, row: &[Value], bound: &[Value]) -> bool {
    match n {
        Node::True => true,
        Node::And(a, b) => node(a, row, bound) && node(b, row, bound),
        Node::Or(a, b) => node(a, row, bound) || node(b, row, bound),
        Node::Cmp { l, op, r, .. } => {
            let (l, r) = (value(l, row, bound), value(r, row, bound));
            let (l, r) = (l.as_ref(), r.as_ref());
            match op {
                CmpOp::Eq => is(l, r),
                CmpOp::Ne => !is(l, r),
                CmpOp::Gt => cmp(l, r) == Some(Ordering::Greater),
                CmpOp::Ge => matches!(cmp(l, r), Some(Ordering::Greater | Ordering::Equal)),
                CmpOp::Lt => cmp(l, r) == Some(Ordering::Less),
                CmpOp::Le => matches!(cmp(l, r), Some(Ordering::Less | Ordering::Equal)),
                CmpOp::Like => like(l, r) == Some(true),
                CmpOp::NotLike => like(l, r) == Some(false),
            }
        }
        Node::In { l, list, .. } => {
            let l = value(l, row, bound);
            list.iter()
                .any(|t| cmp(l.as_ref(), value(t, row, bound).as_ref()) == Some(Ordering::Equal))
        }
    }
}

fn value<'a>(t: &Term, row: &'a [Value], bound: &'a [Value]) -> Cow<'a> {
    let (v, lower) = match t {
        Term::Col { slot, lower, .. } => (row.get(*slot).unwrap_or(&Value::Null), *lower),
        Term::Param { idx, lower } => (&bound[*idx], *lower),
    };
    match v {
        // SQLite's built-in lower() folds ASCII only.
        Value::Text(s) if lower => Cow::Owned(Value::Text(s.to_ascii_lowercase().into())),
        v => Cow::Borrowed(v),
    }
}

enum Cow<'a> {
    Borrowed(&'a Value),
    Owned(Value),
}

impl Cow<'_> {
    fn as_ref(&self) -> &Value {
        match self {
            Cow::Borrowed(v) => v,
            Cow::Owned(v) => v,
        }
    }
}

/// SQL `IS`: NULL-safe equality.
fn is(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Null, _) | (_, Value::Null) => false,
        _ => cmp(a, b) == Some(Ordering::Equal),
    }
}

/// SQLite comparison with BINARY collation; `None` when either side is NULL.
fn cmp(a: &Value, b: &Value) -> Option<Ordering> {
    let rank = |v: &Value| match v {
        Value::Null => 0,
        Value::Int(_) | Value::Real(_) => 1,
        Value::Text(_) => 2,
        Value::Blob(_) => 3,
    };
    Some(match (a, b) {
        (Value::Null, _) | (_, Value::Null) => return None,
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Int(x), Value::Real(y)) => (*x as f64).partial_cmp(y)?,
        (Value::Real(x), Value::Int(y)) => x.partial_cmp(&(*y as f64))?,
        (Value::Real(x), Value::Real(y)) => x.partial_cmp(y)?,
        (Value::Text(x), Value::Text(y)) => x.as_bytes().cmp(y.as_bytes()),
        (Value::Blob(x), Value::Blob(y)) => x.cmp(y),
        _ => rank(a).cmp(&rank(b)),
    })
}

/// `hay LIKE '%needle%' ESCAPE '\'`, the only pattern shape the compiler emits.
/// SQLite LIKE folds ASCII case only.
fn like(hay: &Value, pat: &Value) -> Option<bool> {
    let (Value::Text(h), Value::Text(p)) = (hay, pat) else {
        return None;
    };
    let inner = p.strip_prefix('%')?.strip_suffix('%')?;
    let mut needle = String::with_capacity(inner.len());
    let mut esc = false;
    for c in inner.chars() {
        if c == '\\' && !esc {
            esc = true;
            continue;
        }
        esc = false;
        needle.push(c);
    }
    Some(
        h.to_ascii_lowercase()
            .contains(&needle.to_ascii_lowercase()),
    )
}
