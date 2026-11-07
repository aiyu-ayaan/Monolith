//! Per-app API keys (doc 08 §4): `mk_<app prefix>_<secret>`, shown once, stored as SHA-256.
//!
//! Live keys sit in memory keyed by that hash, so verifying a request costs one hash and one
//! map lookup, no DB read. Not yet: IP allowlist, per-key rate limit, publishable keys,
//! last-used time, `@request.key.*` in rules, per-collection `records:admin`.

use std::collections::HashMap;
use std::sync::Arc;

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
}

impl ApiKey {
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
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiKeyIn {
    pub name: String,
    pub scopes: Vec<String>,
    /// Omit for a key that never expires.
    #[serde(default)]
    pub expires_in_days: Option<u32>,
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
    Ok(())
}

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
    let expires_us = match &r[7] {
        Value::Int(e) => Some(*e),
        _ => None,
    };
    Some((
        ApiKey {
            id: <Id>::try_from(&id[..]).ok()?,
            app: app.to_string(),
            name: name.to_string(),
            prefix: prefix.to_string(),
            scopes: serde_json::from_str(scopes).ok()?,
            created_us: *created,
            expires_us,
        },
        <[u8; 32]>::try_from(&hash[..]).ok()?,
    ))
}

const COLS: &str = "id, app, name, prefix, hash, scopes, created, expires";

pub(crate) async fn load(
    sys: &monolith_store::SqliteStore,
) -> CoreResult<HashMap<[u8; 32], Arc<ApiKey>>> {
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
        let key = ApiKey {
            id: new_id(),
            app: app.to_owned(),
            name: input.name.trim().to_owned(),
            prefix: secret.chars().take(12).collect(),
            scopes: input.scopes,
            created_us: now,
            expires_us: input.expires_in_days.map(|d| now + i64::from(d) * DAY_US),
        };
        let hash = sha256(secret.as_bytes());
        let scopes =
            serde_json::to_string(&key.scopes).map_err(|e| CoreError::Internal(e.to_string()))?;
        self.sys
            .write(sys_write(
                "INSERT INTO api_keys (id, app, name, prefix, hash, scopes, created, expires) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
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

    pub async fn list_api_keys(&self, app: &str) -> CoreResult<Vec<ApiKeyInfo>> {
        self.app_info(app).await?;
        let rows = self
            .sys
            .query(Query {
                sql: format!("SELECT {COLS} FROM api_keys WHERE app = ? ORDER BY created DESC")
                    .into(),
                params: vec![Value::Text(app.into())],
            })
            .await?;
        Ok(rows
            .iter()
            .filter_map(|r| from_row(r))
            .map(|(k, _)| k.info())
            .collect())
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

    /// The live, unexpired key for a bearer secret. No DB access.
    pub fn verify_api_key(&self, secret: &str) -> Option<Arc<ApiKey>> {
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
        ApiKey {
            id: [0; 16],
            app: "a".into(),
            name: "k".into(),
            prefix: String::new(),
            scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
            created_us: 0,
            expires_us: None,
        }
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
