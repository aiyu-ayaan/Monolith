//! Record JSON codec. Input decodes straight into typed `Value`s per field (no
//! `serde_json::Value` tree); output writes column values straight into bytes
//! (doc 11 §4, doc 16 §4).

use std::fmt;

use monolith_rules::parse_datetime_us;
use monolith_store::id::{decode as decode_id, encode as encode_id};
use monolith_store::{Id, Value};
use serde::de::{DeserializeSeed, Deserializer, IgnoredAny, MapAccess, Visitor};
use serde_json::value::RawValue;

use crate::error::{CoreError, CoreResult, FieldError};
use crate::schema::{Collection, FIRST_FIELD_SLOT, FieldDef, FieldType, MAX_TEXT_DEFAULT};

// ---------- output ----------

pub fn write_str(out: &mut Vec<u8>, s: &str) {
    out.push(b'"');
    let mut start = 0;
    for (i, &b) in s.as_bytes().iter().enumerate() {
        let esc: &[u8] = match b {
            b'"' => b"\\\"",
            b'\\' => b"\\\\",
            b'\n' => b"\\n",
            b'\r' => b"\\r",
            b'\t' => b"\\t",
            0..=0x1f => b"",
            _ => continue,
        };
        out.extend_from_slice(&s.as_bytes()[start..i]);
        if esc.is_empty() {
            out.extend_from_slice(format!("\\u{b:04x}").as_bytes());
        } else {
            out.extend_from_slice(esc);
        }
        start = i + 1;
    }
    out.extend_from_slice(&s.as_bytes()[start..]);
    out.push(b'"');
}

/// µs since epoch → `YYYY-MM-DDTHH:MM:SS.ffffffZ`.
pub fn format_us(us: i64) -> String {
    let secs = us.div_euclid(1_000_000);
    let frac = us.rem_euclid(1_000_000);
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{frac:06}Z",
        sod / 3600,
        sod / 60 % 60,
        sod % 60
    )
}

fn write_value(out: &mut Vec<u8>, kind: Option<FieldType>, v: &Value) {
    use std::io::Write;
    match (kind, v) {
        (_, Value::Null) => out.extend_from_slice(b"null"),
        (Some(FieldType::Bool), Value::Int(i)) => {
            out.extend_from_slice(if *i != 0 { b"true" } else { b"false" })
        }
        (Some(FieldType::Date), Value::Int(us)) => write_str(out, &format_us(*us)),
        (Some(FieldType::Json), Value::Text(s)) => out.extend_from_slice(s.as_bytes()),
        (_, Value::Int(i)) => {
            let _ = write!(out, "{i}");
        }
        (_, Value::Real(f)) if f.is_finite() => {
            let _ = write!(out, "{f}");
        }
        (_, Value::Real(_)) => out.extend_from_slice(b"null"),
        (_, Value::Text(s)) => write_str(out, s),
        (_, Value::Blob(b)) => match <&Id>::try_from(&b[..]) {
            Ok(id) => {
                out.push(b'"');
                out.extend_from_slice(&encode_id(id));
                out.push(b'"');
            }
            Err(_) => out.extend_from_slice(b"null"),
        },
    }
}

/// Writes one record object. Hidden fields only for superusers; `only` projects.
pub fn write_record(
    out: &mut Vec<u8>,
    c: &Collection,
    row: &[Value],
    su: bool,
    only: Option<&[usize]>,
) {
    write_record_ext(out, c, row, su, only, None, &[], &[]);
}

/// Writes one record object with optional multi-relation values (`(slot, ids)`) and
/// pre-encoded `expand` entries (`(field_name, json_bytes)`).
/// When `projected` is `Some(slots)`, each element in `row` corresponds to `slots[i]`.
#[allow(clippy::too_many_arguments)]
pub fn write_record_ext(
    out: &mut Vec<u8>,
    c: &Collection,
    row: &[Value],
    su: bool,
    only: Option<&[usize]>,
    projected: Option<&[usize]>,
    multi_rels: &[(usize, &[Id])],
    expand: &[(&str, &[u8])],
) {
    out.push(b'{');
    let mut first = true;
    let mut emit_slot = |out: &mut Vec<u8>, slot: usize, v: &Value| {
        let f = c.field_at(slot);
        if f.is_some_and(|f| f.hidden && !su)
            || only.is_some_and(|o| slot != 0 && !o.contains(&slot))
        {
            return;
        }
        if !first {
            out.push(b',');
        }
        first = false;
        out.extend_from_slice(c.json_keys[slot].as_bytes());
        if f.is_some_and(|f| f.is_multi_rel()) {
            let ids = multi_rels
                .iter()
                .find(|(s, _)| *s == slot)
                .map_or(&[][..], |(_, ids)| *ids);
            out.push(b'[');
            for (i, id) in ids.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                out.push(b'"');
                out.extend_from_slice(&encode_id(id));
                out.push(b'"');
            }
            out.push(b']');
            return;
        }
        let kind = match slot {
            0 => Some(FieldType::Relation), // ids encode like relations
            1 | 2 => Some(FieldType::Date),
            _ => f.map(|f| f.kind),
        };
        write_value(out, kind, v);
    };

    match projected {
        Some(slots) => {
            for (&slot, v) in slots.iter().zip(row.iter()) {
                emit_slot(out, slot, v);
            }
        }
        None => {
            for (slot, v) in row.iter().enumerate() {
                emit_slot(out, slot, v);
            }
        }
    }

    if !expand.is_empty() {
        if !first {
            out.push(b',');
        }
        out.extend_from_slice(b"\"expand\":{");
        for (i, (k, bytes)) in expand.iter().enumerate() {
            if i > 0 {
                out.push(b',');
            }
            write_str(out, k);
            out.push(b':');
            out.extend_from_slice(bytes);
        }
        out.push(b'}');
    }
    out.push(b'}');
}

// ---------- input ----------

/// A decoded request body. `values[i]` is user field `i` (`None` = not sent);
/// `multi[i]` holds target IDs for multi-relation user field `i` (`None` = not sent).
#[derive(Debug, Default)]
pub struct Body {
    pub id: Option<Id>,
    pub created: Option<i64>,
    pub updated: Option<i64>,
    pub values: Vec<Option<Value>>,
    pub multi: Vec<Option<Vec<Id>>>,
}

impl Body {
    /// `@request.body` / sample row slice for rules: values by slot, NULL where not sent.
    pub fn rule_slots(&self) -> Vec<Value> {
        let mut v = vec![
            self.id.map_or(Value::Null, |id| Value::Blob(id.into())),
            self.created.map_or(Value::Null, Value::Int),
            self.updated.map_or(Value::Null, Value::Int),
            Value::Null,
        ];
        v.extend(self.values.iter().map(|x| x.clone().unwrap_or(Value::Null)));
        v
    }

    /// Bitmask of slots explicitly present in the decoded JSON object.
    pub fn isset_mask(&self) -> u64 {
        let mut m = (u64::from(self.id.is_some()))
            | (u64::from(self.created.is_some()) << 1)
            | (u64::from(self.updated.is_some()) << 2);
        for (i, (v, mr)) in self.values.iter().zip(&self.multi).enumerate() {
            if v.is_some() || mr.is_some() {
                m |= 1u64 << (FIRST_FIELD_SLOT + i);
            }
        }
        m
    }
}

struct BodySeed<'a> {
    c: &'a Collection,
    allow_id: bool,
    allow_system: bool,
    errors: &'a mut Vec<FieldError>,
}

impl<'de> DeserializeSeed<'de> for BodySeed<'_> {
    type Value = Body;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Body, D::Error> {
        d.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for BodySeed<'_> {
    type Value = Body;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Body, A::Error> {
        let n = self.c.def.fields.len();
        let mut body = Body {
            id: None,
            created: None,
            updated: None,
            values: vec![None; n],
            multi: vec![None; n],
        };
        while let Some(key) = map.next_key::<std::borrow::Cow<'de, str>>()? {
            match self.c.slot(&key) {
                Some(0) if self.allow_id => match map.next_value::<Option<String>>() {
                    Ok(Some(s)) => match decode_id(&s) {
                        Some(id) => body.id = Some(id),
                        None => self.errors.push(FieldError::new(
                            "id",
                            "invalid_format",
                            "not a valid id",
                        )),
                    },
                    _ => self
                        .errors
                        .push(FieldError::new("id", "type", "expected an id string")),
                },
                Some(slot @ (1 | 2)) if self.allow_system => {
                    match map.next_value_seed(ScalarSeed(FieldType::Date))? {
                        Ok(Value::Int(us)) => {
                            if slot == 1 {
                                body.created = Some(us);
                            } else {
                                body.updated = Some(us);
                            }
                        }
                        Ok(Value::Null) => {}
                        _ => self.errors.push(FieldError::new(
                            &key,
                            "type",
                            "expected an RFC 3339 date",
                        )),
                    }
                }
                Some(3) if self.allow_system => {
                    map.next_value::<IgnoredAny>()?;
                }
                Some(slot) if slot >= FIRST_FIELD_SLOT => {
                    let idx = slot - FIRST_FIELD_SLOT;
                    let f = &self.c.def.fields[idx];
                    if f.is_multi_rel() {
                        match map.next_value_seed(MultiRelSeed)? {
                            Ok(ids) => body.multi[idx] = Some(ids),
                            Err(msg) => self.errors.push(FieldError::new(&f.name, "type", msg)),
                        }
                    } else {
                        let v = if f.kind == FieldType::Json {
                            let raw = map.next_value::<Box<RawValue>>()?;
                            Ok(match raw.get() {
                                "null" => Value::Null,
                                s => Value::Text(s.into()),
                            })
                        } else {
                            map.next_value_seed(ScalarSeed(f.kind))?
                        };
                        match v {
                            Ok(v) => body.values[idx] = Some(v),
                            Err(msg) => self.errors.push(FieldError::new(&f.name, "type", msg)),
                        }
                    }
                }
                Some(_) => {
                    map.next_value::<IgnoredAny>()?;
                    self.errors.push(FieldError::new(
                        &key,
                        "read_only",
                        "system fields are read-only",
                    ));
                }
                None => {
                    map.next_value::<IgnoredAny>()?;
                    self.errors
                        .push(FieldError::new(&key, "unknown_field", "no such field"));
                }
            }
        }
        Ok(body)
    }
}

/// Decodes a multi-relation value (`null`, `""`, a single id string, or an array of id strings).
struct MultiRelSeed;

impl<'de> DeserializeSeed<'de> for MultiRelSeed {
    type Value = Result<Vec<Id>, &'static str>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for MultiRelSeed {
    type Value = Result<Vec<Id>, &'static str>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an array of record ids")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Ok(Vec::new()))
    }
    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(Ok(Vec::new()))
    }
    fn visit_str<E>(self, s: &str) -> Result<Self::Value, E> {
        if s.is_empty() {
            return Ok(Ok(Vec::new()));
        }
        Ok(decode_id(s)
            .map(|id| vec![id])
            .ok_or("expected an array of record ids"))
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut ids = Vec::new();
        let mut bad = false;
        while let Some(elem) = seq.next_element_seed(ScalarSeed(FieldType::Relation))? {
            match elem {
                Ok(Value::Blob(b)) => {
                    if let Ok(id) = <&Id>::try_from(&b[..]) {
                        if !ids.contains(id) {
                            ids.push(*id);
                        }
                    } else {
                        bad = true;
                    }
                }
                Ok(Value::Null) => {}
                _ => bad = true,
            }
        }
        if bad {
            Ok(Err("expected an array of record ids"))
        } else {
            Ok(Ok(ids))
        }
    }
    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
        Ok(Err("expected an array of record ids"))
    }
    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
        Ok(Err("expected an array of record ids"))
    }
    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
        Ok(Err("expected an array of record ids"))
    }
    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
        Ok(Err("expected an array of record ids"))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(Err("expected an array of record ids"))
    }
}

/// Decodes one scalar; a wrong JSON type is an `Err(message)`, not a parse failure.
struct ScalarSeed(FieldType);

impl<'de> DeserializeSeed<'de> for ScalarSeed {
    type Value = Result<Value, &'static str>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ScalarSeed {
    type Value = Result<Value, &'static str>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a field value")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Ok(Value::Null))
    }
    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(Ok(Value::Null))
    }
    fn visit_bool<E>(self, b: bool) -> Result<Self::Value, E> {
        Ok(match self.0 {
            FieldType::Bool => Ok(Value::Int(b as i64)),
            _ => Err("unexpected boolean"),
        })
    }
    fn visit_i64<E>(self, i: i64) -> Result<Self::Value, E> {
        Ok(match self.0 {
            FieldType::Number => Ok(Value::Int(i)),
            _ => Err("unexpected number"),
        })
    }
    fn visit_u64<E>(self, u: u64) -> Result<Self::Value, E> {
        Ok(match (self.0, i64::try_from(u)) {
            (FieldType::Number, Ok(i)) => Ok(Value::Int(i)),
            (FieldType::Number, Err(_)) => Ok(Value::Real(u as f64)),
            _ => Err("unexpected number"),
        })
    }
    fn visit_f64<E>(self, f: f64) -> Result<Self::Value, E> {
        Ok(match self.0 {
            FieldType::Number if f.is_finite() => Ok(Value::Real(f)),
            _ => Err("unexpected number"),
        })
    }
    fn visit_str<E>(self, s: &str) -> Result<Self::Value, E> {
        Ok(match self.0 {
            FieldType::Text | FieldType::Email | FieldType::Url | FieldType::Select => {
                Ok(Value::Text(s.into()))
            }
            FieldType::Date if s.is_empty() => Ok(Value::Null),
            FieldType::Date => parse_datetime_us(s)
                .map(Value::Int)
                .ok_or("expected an RFC 3339 date"),
            FieldType::Relation if s.is_empty() => Ok(Value::Null),
            FieldType::Relation => decode_id(s)
                .map(|id| Value::Blob(id.into()))
                .ok_or("expected a record id"),
            _ => Err("unexpected string"),
        })
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(Err("unexpected object"))
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(Err("unexpected array"))
    }
}

/// Decodes and validates a create (`create = true`) or update body.
pub fn decode_body(c: &Collection, json: &[u8], create: bool) -> CoreResult<Body> {
    let mut errors = Vec::new();
    let mut de = serde_json::Deserializer::from_slice(json);
    let body = BodySeed {
        c,
        allow_id: create,
        allow_system: false,
        errors: &mut errors,
    }
    .deserialize(&mut de)
    .and_then(|b| de.end().map(|()| b))
    .map_err(|e| CoreError::BadRequest(format!("invalid JSON body: {e}")))?;
    for (i, f) in c.def.fields.iter().enumerate() {
        if f.is_multi_rel() {
            check_multi_field(f, body.multi[i].as_deref(), create, &mut errors);
        } else {
            check_field(f, body.values[i].as_ref(), create, &mut errors);
        }
    }
    if errors.is_empty() {
        Ok(body)
    } else {
        Err(CoreError::Validation(errors))
    }
}

/// Decodes a sample record/body for `rules:explain` without requiring fields to be present.
pub fn decode_sample(c: &Collection, json: &[u8]) -> CoreResult<(Vec<Value>, u64)> {
    let mut errors = Vec::new();
    let mut de = serde_json::Deserializer::from_slice(json);
    let body = BodySeed {
        c,
        allow_id: true,
        allow_system: true,
        errors: &mut errors,
    }
    .deserialize(&mut de)
    .and_then(|b| de.end().map(|()| b))
    .map_err(|e| CoreError::BadRequest(format!("invalid sample JSON: {e}")))?;
    if errors.is_empty() {
        Ok((body.rule_slots(), body.isset_mask()))
    } else {
        Err(CoreError::Validation(errors))
    }
}

fn check_multi_field(f: &FieldDef, ids: Option<&[Id]>, create: bool, errors: &mut Vec<FieldError>) {
    let empty = ids.is_none_or(<[Id]>::is_empty);
    if f.required && empty && (create || ids.is_some()) {
        errors.push(FieldError::new(&f.name, "required", "value required"));
        return;
    }
    if let (Some(ids), Some(max)) = (ids, f.max_select)
        && ids.len() > max as usize
    {
        errors.push(FieldError::new(
            &f.name,
            "too_many",
            format!("at most {max} relations"),
        ));
    }
}

fn check_field(f: &FieldDef, v: Option<&Value>, create: bool, errors: &mut Vec<FieldError>) {
    let empty = match v {
        None => create,
        Some(Value::Null) => true,
        Some(Value::Text(s)) => s.is_empty() && f.kind != FieldType::Json,
        _ => false,
    };
    if f.required && empty && (create || v.is_some()) {
        errors.push(FieldError::new(&f.name, "required", "value required"));
        return;
    }
    let mut err = |code, msg: String| errors.push(FieldError::new(&f.name, code, msg));
    match (f.kind, v) {
        (FieldType::Text | FieldType::Email | FieldType::Url, Some(Value::Text(s)))
            if !s.is_empty() =>
        {
            let len = s.chars().count();
            let max = f.max.map_or(MAX_TEXT_DEFAULT, |m| m as usize);
            if len > max {
                err("too_long", format!("at most {max} characters"));
            } else if f.min.is_some_and(|m| (len as f64) < m) {
                err(
                    "too_short",
                    format!("at least {} characters", f.min.unwrap_or(0.0)),
                );
            } else if f.kind == FieldType::Email && !valid_email(s) {
                err("invalid_format", "not an email address".into());
            } else if f.kind == FieldType::Url
                && !(s.starts_with("https://") || s.starts_with("http://"))
            {
                err("invalid_format", "not an http(s) URL".into());
            }
        }
        (FieldType::Select, Some(Value::Text(s)))
            if !s.is_empty() && !f.values.iter().any(|x| x == &**s) =>
        {
            err("invalid_option", format!("must be one of {:?}", f.values));
        }
        (FieldType::Number, Some(n @ (Value::Int(_) | Value::Real(_)))) => {
            let x = match n {
                Value::Int(i) => *i as f64,
                Value::Real(r) => *r,
                _ => unreachable!(),
            };
            if f.only_int && matches!(n, Value::Real(r) if r.fract() != 0.0) {
                err("type", "must be an integer".into());
            } else if f.min.is_some_and(|m| x < m) || f.max.is_some_and(|m| x > m) {
                err(
                    "out_of_range",
                    format!("must be within [{:?}, {:?}]", f.min, f.max),
                );
            }
        }
        (FieldType::Json, Some(Value::Text(s))) if s.len() > 1 << 20 => {
            err("too_long", "JSON over 1 MiB".into())
        }
        _ => {}
    }
}

fn valid_email(s: &str) -> bool {
    match s.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !s.contains(char::is_whitespace)
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip() {
        for s in [
            "1970-01-01T00:00:00.000000Z",
            "2026-11-06T13:45:01.250000Z",
            "1969-12-31T23:59:59.999999Z",
        ] {
            let us = parse_datetime_us(s).unwrap();
            assert_eq!(format_us(us), s);
        }
    }

    #[test]
    fn escapes_strings() {
        let mut out = Vec::new();
        write_str(&mut out, "a\"b\\c\n\u{1}é");
        assert_eq!(String::from_utf8(out).unwrap(), r#""a\"b\\c\n\u0001é""#);
    }
}
