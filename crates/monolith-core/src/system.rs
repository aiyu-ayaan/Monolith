//! Control plane (`/data/system.db`): apps, superusers, server secrets (doc 03 §2).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use monolith_auth::{
    Claims, hash_password, hex, random_bytes, sign_token, verify_password, verify_token,
};
use monolith_store::id::new_id;
use monolith_store::{
    AppStore, Error as StoreError, Id, MigrationPlan, Mutation, NO_CHANGE_LOG, Op, Query,
    SqliteConfig, SqliteStore, Value, WriteOp,
};

use crate::app::{App, now_us};
use crate::auth_config::{AuthConfigIn, AuthConfigView, StoredAuth};
use crate::error::{CoreError, CoreResult};
use crate::metrics::Metrics;
use crate::schema::valid_name;

pub const SUPERUSER_TOKEN_TTL_US: i64 = 3600 * 1_000_000;
pub const MIN_PASSWORD: usize = 10;
/// Lifetime of the one-time installer token; a restart issues a fresh one.
pub const INSTALL_TOKEN_TTL_US: i64 = 3600 * 1_000_000;

const SYSTEM_DDL: &str = "
CREATE TABLE IF NOT EXISTS apps (name TEXT PRIMARY KEY, created INTEGER NOT NULL) STRICT;
CREATE TABLE IF NOT EXISTS superusers (
  id BLOB PRIMARY KEY, email TEXT NOT NULL UNIQUE, hash TEXT NOT NULL, created INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value BLOB NOT NULL) STRICT;
CREATE TABLE IF NOT EXISTS api_keys (
  id BLOB PRIMARY KEY, app TEXT NOT NULL, name TEXT NOT NULL, prefix TEXT NOT NULL,
  hash BLOB NOT NULL UNIQUE, scopes TEXT NOT NULL, created INTEGER NOT NULL, expires INTEGER,
  last_used INTEGER, rate_limit INTEGER, allowed_ips TEXT
) STRICT;
CREATE INDEX IF NOT EXISTS api_keys_app ON api_keys (app, created);
CREATE TABLE IF NOT EXISTS webhooks (
  id BLOB PRIMARY KEY, app TEXT NOT NULL, name TEXT NOT NULL, url TEXT NOT NULL,
  secret TEXT NOT NULL, events TEXT NOT NULL, enabled INTEGER NOT NULL, created INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS webhooks_app ON webhooks (app, created);
CREATE TABLE IF NOT EXISTS webhook_deliveries (
  id BLOB PRIMARY KEY, hook BLOB NOT NULL, app TEXT NOT NULL, event TEXT NOT NULL,
  seq INTEGER NOT NULL, payload BLOB NOT NULL, status TEXT NOT NULL, attempts INTEGER NOT NULL,
  next_at INTEGER NOT NULL, last_status INTEGER, last_error TEXT, created INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS wh_deliveries_due ON webhook_deliveries (status, next_at);
CREATE INDEX IF NOT EXISTS wh_deliveries_hook ON webhook_deliveries (hook, created);
CREATE TABLE IF NOT EXISTS app_users (
  id BLOB PRIMARY KEY, app TEXT NOT NULL, email TEXT, hash TEXT, name TEXT NOT NULL,
  created INTEGER NOT NULL, disabled INTEGER NOT NULL, last_login INTEGER,
  UNIQUE (app, email)
) STRICT;
CREATE TABLE IF NOT EXISTS app_identities (
  app TEXT NOT NULL, provider TEXT NOT NULL, subject TEXT NOT NULL, user BLOB NOT NULL,
  PRIMARY KEY (app, provider, subject)
) STRICT;
CREATE INDEX IF NOT EXISTS app_identities_user ON app_identities (user);
CREATE TABLE IF NOT EXISTS app_sessions (
  hash BLOB PRIMARY KEY, app TEXT NOT NULL, user BLOB NOT NULL, exp INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS app_sessions_exp ON app_sessions (exp);
CREATE INDEX IF NOT EXISTS app_sessions_user ON app_sessions (user);
CREATE TABLE IF NOT EXISTS plugins (
  id BLOB PRIMARY KEY, app TEXT NOT NULL, name TEXT NOT NULL, wasm BLOB NOT NULL,
  events TEXT NOT NULL, enabled INTEGER NOT NULL, fuel INTEGER NOT NULL, created INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS plugins_app ON plugins (app, created);
";

/// A console administrator (row of `superusers`, without the hash).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuperuserInfo {
    pub id: Id,
    pub email: String,
    pub created_us: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppInfo {
    pub name: String,
    pub created_us: i64,
}

pub struct Registry {
    dir: PathBuf,
    pub(crate) sys: Arc<SqliteStore>,
    /// Outbound webhooks: per-app hook cache, change cursors and the delivery worker.
    pub(crate) wh: Arc<crate::webhooks::WhHub>,
    apps: RwLock<HashMap<String, Arc<App>>>,
    pub(crate) open_lock: tokio::sync::Mutex<()>,
    pub(crate) token_key: Vec<u8>,
    /// Verified against when the email is unknown, so timing does not reveal accounts.
    pub(crate) dummy_hash: String,
    /// One-time installer token `(token, expires µs)`, memory only (doc 18 §2).
    install: tokio::sync::Mutex<Option<(String, i64)>>,
    /// Storage of apps that used it since start.
    pub(crate) files: RwLock<HashMap<String, Arc<crate::files::Files>>>,
    /// Bounds concurrent thumbnail jobs (half the cores, at least one).
    pub(crate) thumb_pool: Arc<tokio::sync::Semaphore>,
    /// Live API keys by SHA-256 of the secret; verification never touches the DB (doc 08 §2).
    pub(crate) keys: RwLock<HashMap<[u8; 32], Arc<crate::apikeys::ApiKey>>>,
    /// OAuth sign-ins in flight, by `state` (memory only, 10 minutes).
    pub(crate) oauth_pending: std::sync::Mutex<HashMap<String, crate::oauth::Pending>>,
    /// All HTTP traffic plus every app's record ops (in memory, since start).
    pub metrics: Arc<Metrics>,
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn blocking_err(e: tokio::task::JoinError) -> CoreError {
    CoreError::Internal(e.to_string())
}

/// Adds the columns of `wanted` that `table` lacks (data directories older than the column).
/// Returns whether any was added. Table and column names are code constants, never user input.
pub(crate) async fn ensure_columns(
    store: &SqliteStore,
    table: &str,
    wanted: &[(&str, &str)],
) -> CoreResult<bool> {
    let have = store
        .query(Query {
            sql: format!("SELECT name FROM pragma_table_info('{table}')").into(),
            params: vec![],
        })
        .await?;
    let statements: Vec<String> = wanted
        .iter()
        .filter(|(c, _)| {
            !have
                .iter()
                .any(|r| matches!(&r[0], Value::Text(t) if &**t == *c))
        })
        .map(|(c, decl)| format!("ALTER TABLE {table} ADD COLUMN {c} {decl}"))
        .collect();
    if statements.is_empty() {
        return Ok(false);
    }
    store.apply_schema(MigrationPlan { statements }).await?;
    Ok(true)
}

pub(crate) fn sys_delete(sql: &str, params: Vec<Value>) -> WriteOp {
    let mut w = sys_write(sql, [0; 16], params);
    w.mutations[0].op = Op::Delete;
    w
}

pub(crate) fn sys_write(sql: &str, rid: Id, params: Vec<Value>) -> WriteOp {
    WriteOp {
        actor: None,
        mutations: vec![Mutation {
            op: Op::Insert,
            coll: NO_CHANGE_LOG,
            rid,
            fields: None,
            sql: sql.into(),
            params,
            seq_slot: None,
            hlc_slot: None,
            post_sql: Vec::new(),
        }],
    }
}

impl Registry {
    /// Opens the data directory (creating it on first start).
    pub async fn open(dir: &Path) -> CoreResult<Registry> {
        let d = dir.to_owned();
        let sys = tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(d.join("apps"))
                .map_err(|e| CoreError::Internal(e.to_string()))?;
            let cfg = SqliteConfig {
                readers: 2,
                ..SqliteConfig::default()
            };
            SqliteStore::open(&d.join("system.db"), &cfg).map_err(CoreError::from)
        })
        .await
        .map_err(blocking_err)??;
        sys.apply_schema(MigrationPlan {
            statements: vec![SYSTEM_DDL.into()],
        })
        .await?;
        let key = random_bytes::<32>().map_err(|e| CoreError::Internal(e.to_string()))?;
        // Keeps the existing key on restart; NotFound means another start already set it.
        match sys
            .write(sys_write(
                "INSERT INTO settings (key, value) VALUES ('token_key', ?) ON CONFLICT DO NOTHING",
                [0; 16],
                vec![Value::Blob(key.into())],
            ))
            .await
        {
            Ok(_) | Err(StoreError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
        let rows = sys
            .query(Query {
                sql: "SELECT value FROM settings WHERE key = 'token_key'".into(),
                params: vec![],
            })
            .await?;
        let Some(Value::Blob(token_key)) = rows.first().and_then(|r| r.first()) else {
            return Err(CoreError::Internal("missing token_key".into()));
        };
        let dummy_hash = tokio::task::spawn_blocking(|| hash_password("dummy password for timing"))
            .await
            .map_err(blocking_err)?
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        let keys = crate::apikeys::load(&sys).await?;
        let sys = Arc::new(sys);
        let wh = crate::webhooks::WhHub::start(sys.clone()).await?;
        Ok(Registry {
            dir: dir.to_owned(),
            keys: RwLock::new(keys),
            files: RwLock::new(HashMap::new()),
            thumb_pool: Arc::new(tokio::sync::Semaphore::new(
                (std::thread::available_parallelism().map_or(2, |n| n.get()) / 2).max(1),
            )),
            sys,
            wh,
            oauth_pending: std::sync::Mutex::new(HashMap::new()),
            apps: RwLock::new(HashMap::new()),
            open_lock: tokio::sync::Mutex::new(()),
            token_key: token_key.to_vec(),
            dummy_hash,
            install: tokio::sync::Mutex::new(None),
            metrics: Arc::new(Metrics::new()),
        })
    }

    pub(crate) fn data_dir(&self) -> &Path {
        &self.dir
    }

    pub(crate) fn app_dir(&self, name: &str) -> PathBuf {
        self.dir.join("apps").join(name)
    }

    // ---------- apps ----------

    pub async fn list_apps(&self) -> CoreResult<Vec<AppInfo>> {
        let rows = self
            .sys
            .query(Query {
                sql: "SELECT name, created FROM apps ORDER BY name".into(),
                params: vec![],
            })
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| match (&r[0], &r[1]) {
                (Value::Text(n), Value::Int(c)) => Some(AppInfo {
                    name: n.to_string(),
                    created_us: *c,
                }),
                _ => None,
            })
            .collect())
    }

    pub async fn app_info(&self, name: &str) -> CoreResult<AppInfo> {
        let rows = self
            .sys
            .query(Query {
                sql: "SELECT name, created FROM apps WHERE name = ?".into(),
                params: vec![Value::Text(name.into())],
            })
            .await?;
        match rows.first().map(|r| (&r[0], &r[1])) {
            Some((Value::Text(n), Value::Int(c))) => Ok(AppInfo {
                name: n.to_string(),
                created_us: *c,
            }),
            _ => Err(CoreError::NotFound("app")),
        }
    }

    pub async fn create_app(&self, name: &str) -> CoreResult<AppInfo> {
        let info = self.reserve_app(name).await?;
        self.app(name).await?;
        Ok(info)
    }

    /// Registers the name without opening the app (restore copies the data file in first).
    pub(crate) async fn reserve_app(&self, name: &str) -> CoreResult<AppInfo> {
        if !valid_name(name) {
            return Err(CoreError::BadRequest(format!("invalid app name '{name}'")));
        }
        let now = now_us();
        self.sys
            .write(sys_write(
                "INSERT INTO apps (name, created) VALUES (?, ?)",
                new_id(),
                vec![Value::Text(name.into()), Value::Int(now)],
            ))
            .await
            .map_err(|e| match e {
                StoreError::Constraint(_) => CoreError::Conflict(format!("app '{name}' exists")),
                e => e.into(),
            })?;
        Ok(AppInfo {
            name: name.to_owned(),
            created_us: now,
        })
    }

    /// Already-opened app, without touching disk.
    /// Ends every realtime stream so a graceful shutdown does not wait on them.
    pub fn close_realtime(&self) {
        for a in self.apps.read().unwrap_or_else(|e| e.into_inner()).values() {
            a.hub.close();
        }
    }

    pub fn loaded_app(&self, name: &str) -> Option<Arc<App>> {
        self.apps
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
            .cloned()
    }

    /// The running app, opened on first use.
    pub async fn app(&self, name: &str) -> CoreResult<Arc<App>> {
        if let Some(a) = self.loaded_app(name) {
            return Ok(a);
        }
        let _guard = self.open_lock.lock().await;
        if let Some(a) = self
            .apps
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
        {
            return Ok(a.clone());
        }
        if !valid_name(name) {
            return Err(CoreError::NotFound("app"));
        }
        self.app_info(name).await?;
        let dir = self.app_dir(name);
        tokio::task::spawn_blocking({
            let dir = dir.clone();
            move || std::fs::create_dir_all(dir)
        })
        .await
        .map_err(blocking_err)?
        .map_err(|e| CoreError::Internal(e.to_string()))?;
        let app = Arc::new(App::open(name, &dir.join("data.db"), self.metrics.clone()).await?);
        self.apps
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.to_owned(), app.clone());
        crate::webhooks::spawn_dispatcher(self.wh.clone(), &app);
        Ok(app)
    }

    /// Irreversible: closes the app and removes its data directory.
    pub async fn delete_app(&self, name: &str) -> CoreResult<()> {
        let _guard = self.open_lock.lock().await;
        self.app_info(name).await?;
        self.apps
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(name);
        self.sys
            .write(WriteOp {
                actor: None,
                mutations: vec![Mutation {
                    op: Op::Delete,
                    coll: NO_CHANGE_LOG,
                    rid: [0; 16],
                    fields: None,
                    sql: "DELETE FROM apps WHERE name = ?".into(),
                    params: vec![Value::Text(name.into())],
                    seq_slot: None,
                    hlc_slot: None,
                    post_sql: Vec::new(),
                }],
            })
            .await?;
        match self
            .sys
            .write(sys_delete(
                "DELETE FROM settings WHERE key = ?",
                vec![Value::Text(format!("auth:{name}").into())],
            ))
            .await
        {
            Ok(_) | Err(StoreError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
        match self
            .sys
            .write(sys_delete(
                "DELETE FROM settings WHERE key = ?",
                vec![Value::Text(format!("storage:{name}").into())],
            ))
            .await
        {
            Ok(_) | Err(StoreError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
        self.drop_api_keys(name).await?;
        self.drop_webhooks(name).await?;
        self.drop_plugins(name).await?;
        self.drop_app_users(name).await?;
        self.files
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(name);
        let dir = self.app_dir(name);
        tokio::task::spawn_blocking(move || match std::fs::remove_dir_all(&dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        })
        .await
        .map_err(blocking_err)?
        .map_err(|e| CoreError::Internal(e.to_string()))
    }

    // ---------- per-app auth configuration ----------

    pub(crate) async fn stored_auth(&self, app: &str) -> CoreResult<StoredAuth> {
        let rows = self
            .sys
            .query(Query {
                sql: "SELECT value FROM settings WHERE key = ?".into(),
                params: vec![Value::Text(format!("auth:{app}").into())],
            })
            .await?;
        match rows.first().and_then(|r| r.first()) {
            Some(Value::Blob(b)) => serde_json::from_slice(b)
                .map_err(|e| CoreError::Internal(format!("auth config for {app}: {e}"))),
            _ => Ok(StoredAuth::default()),
        }
    }

    pub async fn auth_config(&self, app: &str) -> CoreResult<AuthConfigView> {
        self.app_info(app).await?;
        Ok(self.stored_auth(app).await?.view(app))
    }

    pub async fn put_auth_config(
        &self,
        app: &str,
        input: AuthConfigIn,
    ) -> CoreResult<AuthConfigView> {
        self.app_info(app).await?;
        let next = self.stored_auth(app).await?.apply(input)?;
        let bytes = serde_json::to_vec(&next).map_err(|e| CoreError::Internal(e.to_string()))?;
        self.sys
            .write(sys_write(
                "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [0; 16],
                vec![
                    Value::Text(format!("auth:{app}").into()),
                    Value::Blob(bytes.into()),
                ],
            ))
            .await?;
        Ok(next.view(app))
    }

    // ---------- superusers ----------

    pub async fn superuser_count(&self) -> CoreResult<i64> {
        let rows = self
            .sys
            .query(Query {
                sql: "SELECT count(*) FROM superusers".into(),
                params: vec![],
            })
            .await?;
        Ok(match rows.first().and_then(|r| r.first()) {
            Some(Value::Int(n)) => *n,
            _ => 0,
        })
    }

    pub async fn create_superuser(&self, email: &str, password: &str) -> CoreResult<Id> {
        let email = email.trim().to_ascii_lowercase();
        if !email.contains('@') || email.len() > 254 {
            return Err(CoreError::Validation(vec![crate::FieldError::new(
                "email",
                "invalid_format",
                "not an email address",
            )]));
        }
        if password.chars().count() < MIN_PASSWORD {
            return Err(CoreError::Validation(vec![crate::FieldError::new(
                "password",
                "too_short",
                format!("at least {MIN_PASSWORD} characters"),
            )]));
        }
        let pw = password.to_owned();
        let hash = tokio::task::spawn_blocking(move || hash_password(&pw))
            .await
            .map_err(blocking_err)?
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        let id = new_id();
        self.sys
            .write(sys_write(
                "INSERT INTO superusers (id, email, hash, created) VALUES (?, ?, ?, ?)",
                id,
                vec![
                    Value::Blob(id.into()),
                    Value::Text(email.into()),
                    Value::Text(hash.into()),
                    Value::Int(now_us()),
                ],
            ))
            .await
            .map_err(|e| match e {
                StoreError::Constraint(_) => {
                    CoreError::Conflict("a superuser with this email exists".into())
                }
                e => e.into(),
            })?;
        Ok(id)
    }

    /// All superusers, oldest first. Control-plane read; not a hot path.
    pub async fn list_superusers(&self) -> CoreResult<Vec<SuperuserInfo>> {
        self.query_superusers(
            "SELECT id, email, created FROM superusers ORDER BY created",
            vec![],
        )
        .await
    }

    pub async fn superuser(&self, id: Id) -> CoreResult<SuperuserInfo> {
        self.query_superusers(
            "SELECT id, email, created FROM superusers WHERE id = ?",
            vec![Value::Blob(id.into())],
        )
        .await?
        .pop()
        .ok_or(CoreError::NotFound("superuser"))
    }

    async fn query_superusers(
        &self,
        sql: &str,
        params: Vec<Value>,
    ) -> CoreResult<Vec<SuperuserInfo>> {
        let rows = self
            .sys
            .query(Query {
                sql: sql.into(),
                params,
            })
            .await?;
        Ok(rows
            .iter()
            .filter_map(|r| match (&r[0], &r[1], &r[2]) {
                (Value::Blob(id), Value::Text(email), Value::Int(created)) => Some(SuperuserInfo {
                    id: <[u8; 16]>::try_from(&id[..]).ok()?,
                    email: email.to_string(),
                    created_us: *created,
                }),
                _ => None,
            })
            .collect())
    }

    /// Removes a superuser. `by` (the caller) cannot remove itself, and the last superuser
    /// stays, so the console never locks everyone out.
    // shortcut: tokens are not revoked; the removed admin's token works until it expires (1 h).
    pub async fn delete_superuser(&self, id: &str, by: Id) -> CoreResult<()> {
        let id = monolith_store::id::decode(id).ok_or(CoreError::NotFound("superuser"))?;
        if id == by {
            return Err(CoreError::Conflict("you cannot remove yourself".into()));
        }
        // The count check sits in the statement, so two concurrent deletes cannot remove both.
        let res = self
            .sys
            .write(sys_delete(
                "DELETE FROM superusers WHERE id = ? AND (SELECT count(*) FROM superusers) > 1",
                vec![Value::Blob(id.into())],
            ))
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(StoreError::NotFound) => {
                self.superuser(id).await?;
                Err(CoreError::Conflict(
                    "the last superuser cannot be removed".into(),
                ))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Issues the one-time installer token when no superuser exists (PocketBase-style
    /// setup URL). It lives in memory only and is printed to the server log, so only
    /// someone with console or log access can create the first superuser.
    pub async fn issue_install_token(&self) -> CoreResult<Option<String>> {
        let mut slot = self.install.lock().await;
        if self.superuser_count().await? > 0 {
            *slot = None;
            return Ok(None);
        }
        let t = hex(&random_bytes::<32>().map_err(|e| CoreError::Internal(e.to_string()))?);
        *slot = Some((t.clone(), now_us() + INSTALL_TOKEN_TTL_US));
        Ok(Some(t))
    }

    /// True while no superuser exists.
    pub async fn install_required(&self) -> CoreResult<bool> {
        Ok(self.superuser_count().await? == 0)
    }

    /// Creates the first superuser with the installer token and signs them in.
    /// The token is consumed on success; any token is rejected once a superuser exists.
    pub async fn install(
        &self,
        token: &str,
        email: &str,
        password: &str,
    ) -> CoreResult<(String, i64)> {
        let mut slot = self.install.lock().await;
        let valid = matches!(&*slot, Some((t, exp))
            if now_us() < *exp && ct_eq(t.as_bytes(), token.as_bytes()));
        if !valid || self.superuser_count().await? > 0 {
            return Err(CoreError::Unauthorized);
        }
        let sub = self.create_superuser(email, password).await?;
        *slot = None;
        let exp_us = now_us() + SUPERUSER_TOKEN_TTL_US;
        Ok((sign_token(&self.token_key, Claims { sub, exp_us }), exp_us))
    }

    /// Returns `(token, expires µs)`; wrong email and wrong password look the same.
    pub async fn login(&self, email: &str, password: &str) -> CoreResult<(String, i64)> {
        let rows = self
            .sys
            .query(Query {
                sql: "SELECT id, hash FROM superusers WHERE email = ?".into(),
                params: vec![Value::Text(email.trim().to_ascii_lowercase().into())],
            })
            .await?;
        let (id, hash) = match rows.first().map(|r| (&r[0], &r[1])) {
            Some((Value::Blob(id), Value::Text(h))) => {
                (<[u8; 16]>::try_from(&id[..]).ok(), h.to_string())
            }
            _ => (None, self.dummy_hash.clone()),
        };
        let pw = password.to_owned();
        let ok = tokio::task::spawn_blocking(move || verify_password(&pw, &hash))
            .await
            .map_err(blocking_err)?;
        match (ok, id) {
            (true, Some(sub)) => {
                let exp_us = now_us() + SUPERUSER_TOKEN_TTL_US;
                Ok((sign_token(&self.token_key, Claims { sub, exp_us }), exp_us))
            }
            _ => Err(CoreError::Unauthorized),
        }
    }

    /// No DB lookup (doc 08 §2).
    // shortcut: no revocation list yet; deleting a superuser takes effect when the token expires (1 h).
    pub fn verify_superuser(&self, token: &str) -> Option<Id> {
        verify_token(&self.token_key, token, now_us()).map(|c| c.sub)
    }
}
