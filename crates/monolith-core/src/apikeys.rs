//! Per-app API keys (doc 08 §4): `mk_<app prefix>_<secret>`, shown once, stored as SHA-256.
//!
//! Live keys sit in memory keyed by that hash, so verifying a request costs one hash and one
//! map lookup, no DB read. Per key: optional rate limit (GCRA, lock-free), CIDR allowlist and a
//! last-used time kept in memory and flushed in the background. Not yet: publishable keys,
//! `@request.key.*` in rules, per-collection `records:admin`.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering::Relaxed};

use monolith_auth::{new_api_key, sha256};
use monolith_store::id::{encode_string, new_id};
use monolith_store::{AppStore, Id, Query, Value};
use serde::{Deserialize, Serialize};

use crate::app::now_us;
use crate::codec::format_us;
use crate::error::{CoreError, CoreResult, FieldError};
use crate::schema::valid_name;
use crate::system::{Registry, sys_delete, sys_write};

const MAX_SCOPES: usize = 32;
const MAX_NAME: usize = 100;
const MAX_DAYS: u32 = 3650;
const MAX_RATE: u32 = 1_000_000;
const MAX_CIDRS: usize = 32;
const MINUTE_US: i64 = 60_000_000;
const DAY_US: i64 = 86_400 * 1_000_000;

/// Scope bases and whether they take a `:target` (collection or bucket).
const SCOPES: [(&str, bool); 6] = [
    ("records:read", true),
    ("records:write", true),
    ("records:admin", false),
    ("realtime:subscribe", false),
    ("files:read", true),
    ("files:write", true),
];

/// A live key. The secret itself is never kept.
#[derive(Debug)]
pub struct ApiKey {
    pub id: Id,
    pub app: String,
    pub name: String,
    /// Display form of the secret: the first 12 characters.
    pub prefix: String,
    pub scopes: Vec<String>,
    pub created_us: i64,
    pub expires_us: Option<i64>,
    /// Requests per minute (burst of the same size); `None` is unlimited.
    pub rate_per_min: Option<u32>,
    /// Allowed client networks as written (`10.0.0.0/8`, `203.0.113.7`); empty allows any address.
    pub allowed_ips: Vec<String>,
    nets: Vec<Net>,
    /// Latest use, µs; 0 = never. Memory only until [`Registry::flush_key_usage`].
    last_used_us: AtomicI64,
    flushed_us: AtomicI64,
    /// GCRA theoretical arrival time, µs.
    tat_us: AtomicI64,
}

/// An IP network in canonical form (IPv4-mapped IPv6 counts as IPv4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Net {
    v4: bool,
    bits: u128,
    prefix: u8,
}

fn ip_bits(ip: IpAddr) -> (bool, u128) {
    match ip.to_canonical() {
        IpAddr::V4(a) => (true, u128::from(u32::from(a))),
        IpAddr::V6(a) => (false, u128::from(a)),
    }
}

impl Net {
    fn width(v4: bool) -> u8 {
        if v4 { 32 } else { 128 }
    }

    fn mask(v4: bool, prefix: u8) -> u128 {
        let w = Self::width(v4);
        if prefix == 0 {
            0
        } else {
            (u128::MAX << (128 - prefix)) >> (128 - w)
        }
    }

    fn parse(s: &str) -> Option<Net> {
        let (a, p) = s.split_once('/').map_or((s, None), |(a, p)| (a, Some(p)));
        let (v4, bits) = ip_bits(a.parse().ok()?);
        let prefix = match p {
            None => Self::width(v4),
            Some(p) => p.parse::<u8>().ok().filter(|p| *p <= Self::width(v4))?,
        };
        Some(Net {
            v4,
            bits: bits & Self::mask(v4, prefix),
            prefix,
        })
    }

    fn contains(&self, ip: IpAddr) -> bool {
        let (v4, bits) = ip_bits(ip);
        v4 == self.v4 && bits & Self::mask(v4, self.prefix) == self.bits
    }
}

impl ApiKey {
    /// Whether `ip` may use this key. An unknown address fails a non-empty allowlist.
    pub fn ip_allowed(&self, ip: Option<IpAddr>) -> bool {
        self.nets.is_empty() || ip.is_some_and(|ip| self.nets.iter().any(|n| n.contains(ip)))
    }

    /// GCRA: `Err(retry after µs)` when the key is over its rate. Lock-free.
    pub fn take(&self, now: i64) -> Result<(), i64> {
        let Some(limit) = self.rate_per_min else {
            return Ok(());
        };
        let t = (MINUTE_US / i64::from(limit)).max(1);
        let tau = MINUTE_US - t; // a burst of `limit` requests
        let mut tat = self.tat_us.load(Relaxed);
        loop {
            if now < tat - tau {
                return Err(tat - tau - now);
            }
            match self
                .tat_us
                .compare_exchange_weak(tat, tat.max(now) + t, Relaxed, Relaxed)
            {
                Ok(_) => return Ok(()),
                Err(cur) => tat = cur,
            }
        }
    }

    pub fn last_used(&self) -> Option<i64> {
        Some(self.last_used_us.load(Relaxed)).filter(|t| *t > 0)
    }

    /// True when a scope grants `perm` (e.g. `records:read`) on `target` (a collection or bucket).
    /// `records:admin` grants every `records:*`; a scope without target covers every target.
    pub fn allows(&self, perm: &str, target: Option<&str>) -> bool {
        self.scopes.iter().any(|s| {
            if s == "records:admin" {
                return perm.starts_with("records:");
            }
            match s.strip_prefix(perm) {
                Some("") => true,
                Some(rest) => rest.strip_prefix(':').is_some_and(|t| Some(t) == target),
                None => false,
            }
        })
    }

    /// Bypasses collection rules, like a superuser limited to this app.
    pub fn admin(&self) -> bool {
        self.scopes.iter().any(|s| s == "records:admin")
    }

    pub fn expired(&self, now: i64) -> bool {
        self.expires_us.is_some_and(|e| e <= now)
    }

    pub fn info(&self) -> ApiKeyInfo {
        ApiKeyInfo {
            id: encode_string(&self.id),
            app: self.app.clone(),
            name: self.name.clone(),
            prefix: self.prefix.clone(),
            scopes: self.scopes.clone(),
            created: format_us(self.created_us),
            expires: self.expires_us.map(format_us),
            last_used: self.last_used().map(format_us),
            rate_limit_per_min: self.rate_per_min,
            allowed_ips: self.allowed_ips.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyInfo {
    pub id: String,
    pub app: String,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<String>,
    pub created: String,
    pub expires: Option<String>,
    /// Updated in memory on every use, saved about once a minute.
    pub last_used: Option<String>,
    pub rate_limit_per_min: Option<u32>,
    pub allowed_ips: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiKeyIn {
    pub name: String,
    pub scopes: Vec<String>,
    /// Omit for a key that never expires.
    #[serde(default)]
    pub expires_in_days: Option<u32>,
    /// Omit for no limit.
    #[serde(default)]
    pub rate_limit_per_min: Option<u32>,
    /// IPs or CIDR networks the key may be used from; omit or empty for anywhere.
    #[serde(default)]
    pub allowed_ips: Vec<String>,
}

fn invalid(field: &str, code: &'static str, msg: impl Into<String>) -> CoreError {
    CoreError::Validation(vec![FieldError::new(field, code, msg)])
}

fn validate(input: &ApiKeyIn) -> CoreResult<()> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(invalid(
            "name",
            "out_of_range",
            format!("1 to {MAX_NAME} characters"),
        ));
    }
    if input.scopes.is_empty() || input.scopes.len() > MAX_SCOPES {
        return Err(invalid(
            "scopes",
            "out_of_range",
            format!("1 to {MAX_SCOPES} scopes"),
        ));
    }
    for s in &input.scopes {
        let ok = SCOPES
            .iter()
            .any(|(base, targeted)| match s.strip_prefix(base) {
                Some("") => true,
                Some(t) => *targeted && t.strip_prefix(':').is_some_and(valid_name),
                None => false,
            });
        if !ok {
            return Err(invalid(
                "scopes",
                "unknown_scope",
                format!(
                    "'{s}' is not a scope; use records:read|write[:collection], records:admin, \
                     realtime:subscribe, files:read|write[:bucket]"
                ),
            ));
        }
    }
    if input
        .expires_in_days
        .is_some_and(|d| !(1..=MAX_DAYS).contains(&d))
    {
        return Err(invalid(
            "expiresInDays",
            "out_of_range",
            format!("1 to {MAX_DAYS}"),
        ));
    }
    if input
        .rate_limit_per_min
        .is_some_and(|r| !(1..=MAX_RATE).contains(&r))
    {
        return Err(invalid(
            "rateLimitPerMin",
            "out_of_range",
            format!("1 to {MAX_RATE}"),
        ));
    }
    if input.allowed_ips.len() > MAX_CIDRS
        || input.allowed_ips.iter().any(|c| Net::parse(c).is_none())
    {
        return Err(invalid(
            "allowedIps",
            "invalid_format",
            format!("up to {MAX_CIDRS} addresses like 203.0.113.7, 10.0.0.0/8 or 2001:db8::/32"),
        ));
    }
    Ok(())
}

struct Parts {
    id: Id,
    app: String,
    name: String,
    prefix: String,
    scopes: Vec<String>,
    created_us: i64,
    expires_us: Option<i64>,
    rate_per_min: Option<u32>,
    allowed_ips: Vec<String>,
    last_used_us: i64,
}

impl ApiKey {
    fn build(p: Parts) -> ApiKey {
        ApiKey {
            nets: p.allowed_ips.iter().filter_map(|c| Net::parse(c)).collect(),
            id: p.id,
            app: p.app,
            name: p.name,
            prefix: p.prefix,
            scopes: p.scopes,
            created_us: p.created_us,
            expires_us: p.expires_us,
            rate_per_min: p.rate_per_min,
            allowed_ips: p.allowed_ips,
            last_used_us: AtomicI64::new(p.last_used_us),
            flushed_us: AtomicI64::new(p.last_used_us),
            tat_us: AtomicI64::new(0),
        }
    }
}

const COLS: &str =
    "id, app, name, prefix, hash, scopes, created, expires, last_used, rate_limit, allowed_ips";

fn from_row(r: &[Value]) -> Option<(ApiKey, [u8; 32])> {
    let (Value::Blob(id), Value::Text(app), Value::Text(name), Value::Text(prefix)) =
        (&r[0], &r[1], &r[2], &r[3])
    else {
        return None;
    };
    let (Value::Blob(hash), Value::Text(scopes), Value::Int(created)) = (&r[4], &r[5], &r[6])
    else {
        return None;
    };
    let opt = |v: &Value| match v {
        Value::Int(e) => Some(*e),
        _ => None,
    };
    let ips = match &r[10] {
        Value::Text(t) => serde_json::from_str(t).ok()?,
        _ => Vec::new(),
    };
    Some((
        ApiKey::build(Parts {
            id: <Id>::try_from(&id[..]).ok()?,
            app: app.to_string(),
            name: name.to_string(),
            prefix: prefix.to_string(),
            scopes: serde_json::from_str(scopes).ok()?,
            created_us: *created,
            expires_us: opt(&r[7]),
            rate_per_min: opt(&r[9]).and_then(|r| u32::try_from(r).ok()),
            allowed_ips: ips,
            last_used_us: opt(&r[8]).unwrap_or(0),
        }),
        <[u8; 32]>::try_from(&hash[..]).ok()?,
    ))
}

/// Adds the columns newer than the first `api_keys` layout (existing data directories).
pub(crate) async fn migrate(sys: &monolith_store::SqliteStore) -> CoreResult<()> {
    crate::system::ensure_columns(
        sys,
        "api_keys",
        &[
            ("last_used", "INTEGER"),
            ("rate_limit", "INTEGER"),
            ("allowed_ips", "TEXT"),
        ],
    )
    .await
    .map(drop)
}

pub(crate) async fn load(
    sys: &monolith_store::SqliteStore,
) -> CoreResult<HashMap<[u8; 32], Arc<ApiKey>>> {
    migrate(sys).await?;
    let rows = sys
        .query(Query {
            sql: format!("SELECT {COLS} FROM api_keys").into(),
            params: vec![],
        })
        .await?;
    Ok(rows
        .iter()
        .filter_map(|r| from_row(r))
        .map(|(k, h)| (h, Arc::new(k)))
        .collect())
}

fn ips_json(ips: &[String]) -> CoreResult<Value> {
    serde_json::to_string(ips)
        .map(|j| Value::Text(j.into()))
        .map_err(|e| CoreError::Internal(e.to_string()))
}

impl Registry {
    /// Creates a key; the returned secret is the only time it is visible.
    pub async fn create_api_key(
        &self,
        app: &str,
        input: ApiKeyIn,
    ) -> CoreResult<(ApiKeyInfo, String)> {
        self.app_info(app).await?;
        validate(&input)?;
        let tag: String = app.chars().take(4).collect();
        let secret = new_api_key(&tag).map_err(|e| CoreError::Internal(e.to_string()))?;
        let now = now_us();
        let key = ApiKey::build(Parts {
            id: new_id(),
            app: app.to_owned(),
            name: input.name.trim().to_owned(),
            prefix: secret.chars().take(12).collect(),
            scopes: input.scopes,
            created_us: now,
            expires_us: input.expires_in_days.map(|d| now + i64::from(d) * DAY_US),
            rate_per_min: input.rate_limit_per_min,
            allowed_ips: input.allowed_ips,
            last_used_us: 0,
        });
        let hash = sha256(secret.as_bytes());
        let scopes =
            serde_json::to_string(&key.scopes).map_err(|e| CoreError::Internal(e.to_string()))?;
        self.sys
            .write(sys_write(
                "INSERT INTO api_keys (id, app, name, prefix, hash, scopes, created, expires, \
                 rate_limit, allowed_ips) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                key.id,
                vec![
                    Value::Blob(key.id.into()),
                    Value::Text(key.app.as_str().into()),
                    Value::Text(key.name.as_str().into()),
                    Value::Text(key.prefix.as_str().into()),
                    Value::Blob(hash.into()),
                    Value::Text(scopes.into()),
                    Value::Int(now),
                    key.expires_us.map_or(Value::Null, Value::Int),
                    key.rate_per_min
                        .map_or(Value::Null, |r| Value::Int(i64::from(r))),
                    ips_json(&key.allowed_ips)?,
                ],
            ))
            .await?;
        let info = key.info();
        self.keys
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(hash, Arc::new(key));
        Ok((info, secret))
    }

    /// Replaces a key's name, scopes, expiry (counted from now), rate limit and allowlist. The
    /// secret stays valid, so rotate by creating a new key and revoking this one.
    pub async fn update_api_key(
        &self,
        app: &str,
        id: &str,
        input: ApiKeyIn,
    ) -> CoreResult<ApiKeyInfo> {
        let id = monolith_store::id::decode(id).ok_or(CoreError::NotFound("api key"))?;
        validate(&input)?;
        let (hash, old) = self
            .keys
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|(_, k)| k.id == id && k.app == app)
            .map(|(h, k)| (*h, k.clone()))
            .ok_or(CoreError::NotFound("api key"))?;
        let now = now_us();
        let key = ApiKey::build(Parts {
            id,
            app: app.to_owned(),
            name: input.name.trim().to_owned(),
            prefix: old.prefix.clone(),
            scopes: input.scopes,
            created_us: old.created_us,
            expires_us: input.expires_in_days.map(|d| now + i64::from(d) * DAY_US),
            rate_per_min: input.rate_limit_per_min,
            allowed_ips: input.allowed_ips,
            last_used_us: old.last_used_us.load(Relaxed),
        });
        let scopes =
            serde_json::to_string(&key.scopes).map_err(|e| CoreError::Internal(e.to_string()))?;
        self.sys
            .write(sys_write(
                "UPDATE api_keys SET name = ?, scopes = ?, expires = ?, rate_limit = ?, \
                 allowed_ips = ? WHERE id = ? AND app = ?",
                id,
                vec![
                    Value::Text(key.name.as_str().into()),
                    Value::Text(scopes.into()),
                    key.expires_us.map_or(Value::Null, Value::Int),
                    key.rate_per_min
                        .map_or(Value::Null, |r| Value::Int(i64::from(r))),
                    ips_json(&key.allowed_ips)?,
                    Value::Blob(id.into()),
                    Value::Text(app.into()),
                ],
            ))
            .await
            .map_err(|e| match e {
                monolith_store::Error::NotFound => CoreError::NotFound("api key"),
                e => e.into(),
            })?;
        let info = key.info();
        self.keys
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(hash, Arc::new(key));
        Ok(info)
    }

    pub async fn list_api_keys(&self, app: &str) -> CoreResult<Vec<ApiKeyInfo>> {
        self.app_info(app).await?;
        // The map holds the live usage numbers; the table only has the last flushed ones.
        let mut keys: Vec<_> = self
            .keys
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter(|k| k.app == app)
            .map(|k| k.info())
            .collect();
        keys.sort_by(|a, b| b.created.cmp(&a.created));
        Ok(keys)
    }

    /// Takes effect at once: the key leaves the in-memory map before the call returns.
    pub async fn revoke_api_key(&self, app: &str, id: &str) -> CoreResult<()> {
        let id = monolith_store::id::decode(id).ok_or(CoreError::NotFound("api key"))?;
        let removed = {
            let mut g = self.keys.write().unwrap_or_else(|e| e.into_inner());
            let hash = g
                .iter()
                .find(|(_, k)| k.id == id && k.app == app)
                .map(|(h, _)| *h);
            hash.and_then(|h| g.remove(&h))
        };
        if removed.is_none() {
            return Err(CoreError::NotFound("api key"));
        }
        self.sys
            .write(sys_delete(
                "DELETE FROM api_keys WHERE id = ? AND app = ?",
                vec![Value::Blob(id.into()), Value::Text(app.into())],
            ))
            .await
            .map_err(|e| match e {
                monolith_store::Error::NotFound => CoreError::NotFound("api key"),
                e => e.into(),
            })?;
        Ok(())
    }

    /// The live, unexpired key for a bearer secret, without side effects (no rate limit, no
    /// last-used update): for logging who called. No DB access.
    pub fn lookup_api_key(&self, secret: &str) -> Option<Arc<ApiKey>> {
        if !secret.starts_with("mk_") {
            return None;
        }
        let k = self
            .keys
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&sha256(secret.as_bytes()))?
            .clone();
        (!k.expired(now_us())).then_some(k)
    }

    /// [`Self::lookup_api_key`] plus the per-key checks, in this order: client address
    /// (403), rate limit (429), then the use is recorded. Costs a hash, a map lookup and a few
    /// atomics; no DB access.
    pub fn authorize_api_key(&self, secret: &str, ip: Option<IpAddr>) -> CoreResult<Arc<ApiKey>> {
        let k = self.lookup_api_key(secret).ok_or(CoreError::Unauthorized)?;
        if !k.ip_allowed(ip) {
            return Err(CoreError::Forbidden(
                "the API key may not be used from this address",
            ));
        }
        let now = now_us();
        k.take(now)
            .map_err(|us| CoreError::RateLimited((us as u64).div_ceil(1_000_000).max(1)))?;
        k.last_used_us.store(now, Relaxed);
        Ok(k)
    }

    /// Persists last-used times that changed since the last flush. Call about once a minute and at
    /// shutdown. A key revoked meanwhile is skipped.
    pub async fn flush_key_usage(&self) -> CoreResult<usize> {
        let dirty: Vec<Arc<ApiKey>> = self
            .keys
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter(|k| k.last_used_us.load(Relaxed) > k.flushed_us.load(Relaxed))
            .cloned()
            .collect();
        let mut n = 0;
        for k in dirty {
            let t = k.last_used_us.load(Relaxed);
            match self
                .sys
                .write(sys_write(
                    "UPDATE api_keys SET last_used = ? WHERE id = ?",
                    k.id,
                    vec![Value::Int(t), Value::Blob(k.id.into())],
                ))
                .await
            {
                Ok(_) => {
                    k.flushed_us.store(t, Relaxed);
                    n += 1;
                }
                Err(monolith_store::Error::NotFound) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(n)
    }

    /// Removes every key of a deleted app.
    pub(crate) async fn drop_api_keys(&self, app: &str) -> CoreResult<()> {
        self.keys
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|_, k| k.app != app);
        match self
            .sys
            .write(sys_delete(
                "DELETE FROM api_keys WHERE app = ?",
                vec![Value::Text(app.into())],
            ))
            .await
        {
            Ok(_) | Err(monolith_store::Error::NotFound) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(scopes: &[&str]) -> ApiKey {
        limited(scopes, None, &[])
    }

    fn limited(scopes: &[&str], rate: Option<u32>, ips: &[&str]) -> ApiKey {
        ApiKey::build(Parts {
            id: [0; 16],
            app: "a".into(),
            name: "k".into(),
            prefix: String::new(),
            scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
            created_us: 0,
            expires_us: None,
            rate_per_min: rate,
            allowed_ips: ips.iter().map(|s| (*s).to_owned()).collect(),
            last_used_us: 0,
        })
    }

    #[test]
    fn cidr_allowlist() {
        let ip = |s: &str| Some(s.parse::<IpAddr>().unwrap());
        assert!(key(&[]).ip_allowed(None), "no list allows anything");
        let k = limited(&[], None, &["10.1.0.0/16", "203.0.113.7", "2001:db8::/32"]);
        assert!(k.ip_allowed(ip("10.1.200.3")));
        assert!(!k.ip_allowed(ip("10.2.0.1")));
        assert!(k.ip_allowed(ip("203.0.113.7")) && !k.ip_allowed(ip("203.0.113.8")));
        assert!(k.ip_allowed(ip("2001:db8:1::1")) && !k.ip_allowed(ip("2001:db9::1")));
        assert!(
            k.ip_allowed(ip("::ffff:10.1.0.9")),
            "mapped IPv4 counts as IPv4"
        );
        assert!(!k.ip_allowed(None), "unknown address fails a list");
        assert!(limited(&[], None, &["0.0.0.0/0"]).ip_allowed(ip("8.8.8.8")));
        for bad in ["10.0.0.0/33", "nope", "1.2.3.4/x", "::1/129"] {
            assert!(Net::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn rate_limit_gcra() {
        let k = limited(&[], Some(60), &[]); // 1 per second, burst 60
        assert!((0..60).all(|_| k.take(1_000_000).is_ok()), "burst of 60");
        let wait = k.take(1_000_000).unwrap_err();
        assert!((900_000..=1_000_000).contains(&wait), "{wait}");
        assert!(k.take(1_000_000 + wait).is_ok(), "one slot after the wait");
        assert!(k.take(1_000_000 + wait).is_err());
        assert!(key(&[]).take(0).is_ok(), "unlimited");
    }

    #[test]
    fn scope_matching() {
        let k = key(&["records:read:tasks", "files:write"]);
        assert!(k.allows("records:read", Some("tasks")));
        assert!(!k.allows("records:read", Some("other")));
        assert!(!k.allows("records:read", None));
        assert!(!k.allows("records:write", Some("tasks")));
        assert!(k.allows("files:write", Some("photos")), "no target = all");
        assert!(!k.allows("files:read", Some("photos")));
        let a = key(&["records:admin"]);
        assert!(a.admin() && a.allows("records:write", Some("x")));
        assert!(!a.allows("files:read", None), "admin is records only");
        assert!(!key(&["records:read"]).admin());
    }

    #[test]
    fn scope_validation() {
        let ok = |s: &[&str]| {
            validate(&ApiKeyIn {
                name: "n".into(),
                scopes: s.iter().map(|s| (*s).to_owned()).collect(),
                expires_in_days: None,
                rate_limit_per_min: None,
                allowed_ips: Vec::new(),
            })
            .is_ok()
        };
        assert!(ok(&[
            "records:read",
            "records:write:tasks",
            "realtime:subscribe"
        ]));
        assert!(!ok(&[]));
        assert!(!ok(&["records:readx"]));
        assert!(!ok(&["records:admin:tasks"]), "admin takes no target");
        assert!(!ok(&["records:read:Bad-Name"]));
        assert!(!ok(&["superuser"]));
    }
}
