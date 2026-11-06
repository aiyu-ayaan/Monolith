//! List queries: sort parsing, keyset pagination and opaque cursors (doc 11 §3).

use std::fmt::Write;
use std::sync::Arc;

use monolith_rules::{Compiled, Mode, compile};
use monolith_store::Value;

use crate::error::{CoreError, CoreResult};
use crate::schema::{Collection, FieldType, Schema};

pub const MAX_SORT_KEYS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortKey {
    pub slot: usize,
    pub column: String,
    pub desc: bool,
}

fn bad(msg: impl Into<String>) -> CoreError {
    CoreError::BadRequest(msg.into())
}

/// `-created,title` → keys, with `id` appended as the final tie-breaker.
pub fn parse_sort(c: &Collection, s: &str, su: bool) -> CoreResult<Vec<SortKey>> {
    let mut keys: Vec<SortKey> = Vec::new();
    for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (desc, name) = match part.as_bytes()[0] {
            b'-' => (true, &part[1..]),
            b'+' => (false, &part[1..]),
            _ => (false, part),
        };
        let slot = c
            .slot(name)
            .filter(|&s| s != 3)
            .ok_or_else(|| bad(format!("cannot sort by '{name}'")))?;
        if let Some(f) = c.field_at(slot)
            && (f.kind == FieldType::Json || (f.hidden && !su))
        {
            return Err(bad(format!("cannot sort by '{name}'")));
        }
        if keys.iter().any(|k| k.slot == slot) {
            return Err(bad(format!("duplicate sort field '{name}'")));
        }
        keys.push(SortKey {
            slot,
            column: name.to_owned(),
            desc,
        });
    }
    if !keys.iter().any(|k| k.slot == 0) {
        keys.push(SortKey {
            slot: 0,
            column: "id".into(),
            desc: false,
        });
    }
    if keys.len() > MAX_SORT_KEYS {
        return Err(bad(format!("at most {} sort fields", MAX_SORT_KEYS - 1)));
    }
    Ok(keys)
}

/// SQL for one list shape; cached per (schema version, collection, filter, sort, cursor nulls).
#[derive(Debug)]
pub struct ListPlan {
    pub sql: Arc<str>,
    pub filter: Option<Compiled>,
    pub sort: Vec<SortKey>,
    /// Cursor value index for each keyset `?`, in SQL order.
    pub keyset_binds: Vec<usize>,
}

/// `nulls`: `None` without a cursor, else bit i set when cursor value i is NULL.
pub fn plan(
    schema: &Schema,
    c: &Collection,
    filter: &str,
    sort: &str,
    nulls: Option<u32>,
    su: bool,
) -> CoreResult<ListPlan> {
    let rule = if su {
        "1".to_owned()
    } else {
        c.rules
            .list
            .as_ref()
            .ok_or(CoreError::Forbidden(
                "only superusers can list this collection",
            ))?
            .sql
            .clone()
    };
    let filter = if filter.trim().is_empty() {
        None
    } else {
        Some(compile(
            schema,
            &c.def.name,
            filter,
            if su { Mode::Rule } else { Mode::Filter },
        )?)
    };
    let sort = parse_sort(c, sort, su)?;
    let mut keyset_binds = Vec::new();
    let keyset = match nulls {
        None => "1".to_owned(),
        Some(nulls) => {
            let mut ors = Vec::new();
            for (i, k) in sort.iter().enumerate() {
                let null_i = nulls & (1 << i) != 0;
                let col = format!("t.\"{}\"", k.column);
                // SQLite sorts NULL first ascending and last descending.
                let after = match (k.desc, null_i) {
                    (false, true) => format!("{col} IS NOT NULL"),
                    (false, false) => format!("{col} > ?"),
                    (true, true) => continue,
                    (true, false) => format!("({col} < ? OR {col} IS NULL)"),
                };
                let mut parts = Vec::new();
                let mut binds = Vec::new();
                for (j, prev) in sort[..i].iter().enumerate() {
                    if nulls & (1 << j) != 0 {
                        parts.push(format!("t.\"{}\" IS NULL", prev.column));
                    } else {
                        parts.push(format!("t.\"{}\" = ?", prev.column));
                        binds.push(j);
                    }
                }
                if !null_i {
                    binds.push(i);
                }
                parts.push(after);
                keyset_binds.extend(binds);
                ors.push(format!("({})", parts.join(" AND ")));
            }
            if ors.is_empty() {
                "0".to_owned()
            } else {
                ors.join(" OR ")
            }
        }
    };
    let order = sort
        .iter()
        .map(|k| format!("t.\"{}\"{}", k.column, if k.desc { " DESC" } else { "" }))
        .collect::<Vec<_>>()
        .join(", ");
    let mut sql = String::new();
    let _ = write!(
        sql,
        "SELECT {} FROM \"{}\" AS t WHERE ({rule}) AND ({}) AND ({keyset}) ORDER BY {order} LIMIT ?",
        c.sql.cols,
        c.def.name,
        filter.as_ref().map_or("1", |f| f.sql.as_str()),
    );
    Ok(ListPlan {
        sql: sql.into(),
        filter,
        sort,
        keyset_binds,
    })
}

// ---------- cursor ----------

/// FNV-1a: binds a cursor to the collection, sort and filter it came from.
pub fn shape_hash(parts: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.bytes().chain([0xff]) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

fn hex(b: &[u8]) -> String {
    b.iter()
        .fold(String::with_capacity(b.len() * 2), |mut s, x| {
            let _ = write!(s, "{x:02x}");
            s
        })
}

pub fn encode_cursor(hash: u64, values: &[&Value]) -> String {
    let mut b = hash.to_be_bytes().to_vec();
    for v in values {
        match v {
            Value::Null => b.push(0),
            Value::Int(i) => {
                b.push(1);
                b.extend_from_slice(&i.to_be_bytes());
            }
            Value::Real(f) => {
                b.push(2);
                b.extend_from_slice(&f.to_be_bytes());
            }
            Value::Text(s) => {
                b.push(3);
                b.extend_from_slice(&(s.len() as u32).to_be_bytes());
                b.extend_from_slice(s.as_bytes());
            }
            Value::Blob(x) => {
                b.push(4);
                b.extend_from_slice(&(x.len() as u32).to_be_bytes());
                b.extend_from_slice(x);
            }
        }
    }
    hex(&b)
}

/// Untrusted input: every malformed cursor is a 400, values are only ever bound.
pub fn decode_cursor(s: &str, hash: u64, n: usize) -> CoreResult<Vec<Value>> {
    let err = || bad("invalid cursor (it must come from the same filter and sort)");
    if !s.len().is_multiple_of(2) || s.len() > 8192 {
        return Err(err());
    }
    let bytes: Vec<u8> = (0..s.len())
        .step_by(2)
        .map(|i| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()))
        .collect::<Option<_>>()
        .ok_or_else(err)?;
    let mut r = bytes.as_slice();
    let mut take = |n: usize| -> CoreResult<&[u8]> {
        if r.len() < n {
            return Err(err());
        }
        let (a, b) = r.split_at(n);
        r = b;
        Ok(a)
    };
    if u64::from_be_bytes(take(8)?.try_into().map_err(|_| err())?) != hash {
        return Err(err());
    }
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let tag = take(1)?[0];
        out.push(match tag {
            0 => Value::Null,
            1 => Value::Int(i64::from_be_bytes(take(8)?.try_into().map_err(|_| err())?)),
            2 => Value::Real(f64::from_be_bytes(take(8)?.try_into().map_err(|_| err())?)),
            3 | 4 => {
                let len = u32::from_be_bytes(take(4)?.try_into().map_err(|_| err())?) as usize;
                let data = take(len)?;
                if tag == 3 {
                    Value::Text(std::str::from_utf8(data).map_err(|_| err())?.into())
                } else {
                    Value::Blob(data.into())
                }
            }
            _ => return Err(err()),
        });
    }
    if !r.is_empty() {
        return Err(err());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trip_and_rejects() {
        let vals = [
            Value::Null,
            Value::Int(-3),
            Value::Real(1.5),
            Value::Text("é".into()),
            Value::Blob([1u8; 16].into()),
        ];
        let refs: Vec<&Value> = vals.iter().collect();
        let c = encode_cursor(7, &refs);
        assert_eq!(decode_cursor(&c, 7, 5).unwrap(), vals);
        assert!(decode_cursor(&c, 8, 5).is_err(), "other shape");
        assert!(decode_cursor(&c, 7, 4).is_err(), "trailing bytes");
        assert!(decode_cursor(&c[..c.len() - 2], 7, 5).is_err(), "truncated");
        assert!(decode_cursor("zz", 7, 0).is_err());
    }
}
