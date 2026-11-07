//! Outbound webhooks (doc 11 §7, doc 14 §1): after-commit HTTP callbacks for record events.
//!
//! One dispatcher task per open app turns committed changes into rows of `webhook_deliveries`
//! (system.db); one global worker POSTs due rows with an HMAC signature and retries with
//! exponential backoff. The dispatcher reads `_changes` from a persisted cursor, so a crash
//! between commit and enqueue loses nothing (at-least-once). The change bus is only a wake-up.
//!
//! Limits: events only for record changes (no auth/backup events yet), no per-hook filter
//! expression, deliveries of one hook may arrive out of order after retries (use `seq`), the
//! signing secret is stored in plain text in system.db (it is the HMAC key).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::Duration;

use monolith_auth::{hex, hmac_sha256_hex, random_bytes};
use monolith_store::id::{decode, encode_string, new_id};
use monolith_store::{AppStore, Change, Id, Op, Query, SqliteStore, Value};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::app::{App, now_us, row_multi};
use crate::codec::{self, format_us};
use crate::error::{CoreError, CoreResult, FieldError};
use crate::schema::valid_name;
use crate::system::{Registry, sys_delete, sys_write};

const MAX_NAME: usize = 100;
const MAX_URL: usize = 2048;
const MAX_EVENTS: usize = 32;
/// Hooks per app.
const MAX_HOOKS: usize = 20;
const MAX_ATTEMPTS: i64 = 10;
const TIMEOUT: Duration = Duration::from_secs(10);
const BATCH: u32 = 500;
const DUE_BATCH: usize = 32;
const SECOND_US: i64 = 1_000_000;
const DAY_US: i64 = 86_400 * SECOND_US;
/// Delivered rows are kept this long (dead ones four times as long).
const KEEP_OK_US: i64 = 7 * DAY_US;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WebhookIn {
    pub name: String,
    pub url: String,
    /// `collection.op` patterns: `posts.create`, `posts.*`, `*.delete`, `*`.
    pub events: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookInfo {
    pub id: String,
    pub name: String,
    pub url: String,
    pub events: Vec<String>,
    pub enabled: bool,
    pub created: String,
    /// First characters of the signing secret, to tell hooks apart.
    pub secret_hint: String,
    pub pending: u64,
    pub dead: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryInfo {
    pub id: String,
    pub event: String,
    pub seq: u64,
    /// `pending`, `ok` or `dead`.
    pub status: String,
    pub attempts: u32,
    pub next_attempt: Option<String>,
    pub last_status: Option<u16>,
    pub last_error: Option<String>,
    pub created: String,
}

/// `collection` / `op` of one event pattern; `None` matches anything.
#[derive(Debug, Clone)]
struct Pattern {
    coll: Option<String>,
    op: Option<&'static str>,
}

#[derive(Debug)]
struct Hook {
    id: Id,
    patterns: Vec<Pattern>,
}

impl Hook {
    fn matches(&self, coll: &str, op: &str) -> bool {
        self.patterns
            .iter()
            .any(|p| p.coll.as_deref().is_none_or(|c| c == coll) && p.op.is_none_or(|o| o == op))
    }
}

fn op_name(op: Op) -> &'static str {
    match op {
        Op::Insert => "create",
        Op::Update => "update",
        Op::Delete => "delete",
    }
}

fn parse_pattern(s: &str) -> Option<Pattern> {
    if s == "*" {
        return Some(Pattern {
            coll: None,
            op: None,
        });
    }
    let (c, o) = s.split_once('.')?;
    let op = match o {
        "*" => None,
        "create" => Some("create"),
        "update" => Some("update"),
        "delete" => Some("delete"),
        _ => return None,
    };
    let coll = match c {
        "*" => None,
        c if valid_name(c) => Some(c.to_owned()),
        _ => return None,
    };
    Some(Pattern { coll, op })
}

fn invalid(field: &str, code: &'static str, msg: impl Into<String>) -> CoreError {
    CoreError::Validation(vec![FieldError::new(field, code, msg)])
}

fn validate(input: &WebhookIn) -> CoreResult<()> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(invalid(
            "name",
            "out_of_range",
            format!("1 to {MAX_NAME} characters"),
        ));
    }
    let url_ok = input.url.len() <= MAX_URL
        && reqwest::Url::parse(&input.url)
            .is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some());
    if !url_ok {
        return Err(invalid(
            "url",
            "invalid",
            format!("an http(s) URL of at most {MAX_URL} characters"),
        ));
    }
    if input.events.is_empty() || input.events.len() > MAX_EVENTS {
        return Err(invalid(
            "events",
            "out_of_range",
            format!("1 to {MAX_EVENTS} event patterns"),
        ));
    }
    for e in &input.events {
        if parse_pattern(e).is_none() {
            return Err(invalid(
                "events",
                "invalid",
                format!(
                    "'{e}' is not a pattern; use posts.create|update|delete, posts.*, *.delete or *"
                ),
            ));
        }
    }
    Ok(())
}

fn text(v: &Value) -> &str {
    match v {
        Value::Text(t) => t,
        _ => "",
    }
}

fn int(v: &Value) -> i64 {
    match v {
        Value::Int(n) => *n,
        _ => 0,
    }
}

fn id_of(v: &Value) -> Option<Id> {
    match v {
        Value::Blob(b) => <Id>::try_from(&b[..]).ok(),
        _ => None,
    }
}

fn patterns_of(events_json: &str) -> Vec<Pattern> {
    serde_json::from_str::<Vec<String>>(events_json)
        .unwrap_or_default()
        .iter()
        .filter_map(|e| parse_pattern(e))
        .collect()
}

/// Shared state: the per-app hook cache, change cursors and the delivery worker's wake-up.
pub(crate) struct WhHub {
    sys: Arc<SqliteStore>,
    hooks: RwLock<HashMap<String, Arc<Vec<Hook>>>>,
    /// Last change seq turned into deliveries, per open app.
    cursors: Mutex<HashMap<String, u64>>,
    notify: Arc<Notify>,
    client: reqwest::Client,
}

impl WhHub {
    pub(crate) async fn start(sys: Arc<SqliteStore>) -> CoreResult<Arc<WhHub>> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .user_agent("Monolith-Webhooks/1")
            .build()
            .map_err(|e| CoreError::Internal(e.to_string()))?;
        let hub = Arc::new(WhHub {
            sys,
            hooks: RwLock::new(HashMap::new()),
            cursors: Mutex::new(HashMap::new()),
            notify: Arc::new(Notify::new()),
            client,
        });
        let rows = hub
            .sys
            .query(Query {
                sql: "SELECT id, app, events FROM webhooks WHERE enabled = 1 ORDER BY created"
                    .into(),
                params: vec![],
            })
            .await?;
        let mut map: HashMap<String, Vec<Hook>> = HashMap::new();
        for r in &rows {
            if let Some(id) = id_of(&r[0]) {
                map.entry(text(&r[1]).to_owned()).or_default().push(Hook {
                    id,
                    patterns: patterns_of(text(&r[2])),
                });
            }
        }
        *hub.hooks.write().unwrap_or_else(|e| e.into_inner()) =
            map.into_iter().map(|(k, v)| (k, Arc::new(v))).collect();
        tokio::spawn(worker(Arc::downgrade(&hub), hub.notify.clone()));
        Ok(hub)
    }

    fn hooks_of(&self, app: &str) -> Arc<Vec<Hook>> {
        self.hooks
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(app)
            .cloned()
            .unwrap_or_default()
    }

    /// Re-reads the enabled hooks of `app`; the first hook starts counting from now.
    async fn reload(&self, app: &str) -> CoreResult<()> {
        let rows = self
            .sys
            .query(Query {
                sql:
                    "SELECT id, events FROM webhooks WHERE app = ? AND enabled = 1 ORDER BY created"
                        .into(),
                params: vec![Value::Text(app.into())],
            })
            .await?;
        let list: Vec<Hook> = rows
            .iter()
            .filter_map(|r| {
                Some(Hook {
                    id: id_of(&r[0])?,
                    patterns: patterns_of(text(&r[1])),
                })
            })
            .collect();
        let was_empty = self.hooks_of(app).is_empty();
        let cursor = self
            .cursors
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(app)
            .copied();
        if was_empty
            && !list.is_empty()
            && let Some(c) = cursor
        {
            self.save_cursor(app, c).await?;
        }
        let mut g = self.hooks.write().unwrap_or_else(|e| e.into_inner());
        if list.is_empty() {
            g.remove(app);
        } else {
            g.insert(app.to_owned(), Arc::new(list));
        }
        Ok(())
    }

    async fn save_cursor(&self, app: &str, seq: u64) -> CoreResult<()> {
        self.sys
            .write(sys_write(
                "INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)",
                [0; 16],
                vec![
                    Value::Text(format!("wh_cursor:{app}").into()),
                    Value::Blob(seq.to_le_bytes().to_vec().into()),
                ],
            ))
            .await?;
        Ok(())
    }

    /// Where the dispatcher of `app` resumes: its saved cursor when hooks exist, else now.
    async fn start_cursor(&self, app: &Arc<App>) -> u64 {
        if !self.hooks_of(&app.name).is_empty()
            && let Ok(rows) = self
                .sys
                .query(Query {
                    sql: "SELECT value FROM settings WHERE key = ?".into(),
                    params: vec![Value::Text(format!("wh_cursor:{}", app.name).into())],
                })
                .await
            && let Some(Value::Blob(b)) = rows.first().and_then(|r| r.first())
            && let Ok(a) = <[u8; 8]>::try_from(&b[..])
        {
            return u64::from_le_bytes(a);
        }
        match app
            .store()
            .query(Query {
                sql: "SELECT coalesce(max(seq), 0) FROM _changes".into(),
                params: vec![],
            })
            .await
            .ok()
            .and_then(|r| r.first().and_then(|r| r.first().map(int)))
        {
            Some(n) => n as u64,
            None => 0,
        }
    }

    /// Turns `changes` into delivery rows (and moves the persisted cursor) in one write.
    async fn enqueue(&self, app: &Arc<App>, changes: &[Change]) -> CoreResult<()> {
        let hooks = self.hooks_of(&app.name);
        let last = changes.last().map_or(0, |c| c.seq);
        if hooks.is_empty() {
            return Ok(());
        }
        let schema = app.schema();
        let by_cid: HashMap<u32, &crate::schema::Collection> =
            schema.collections.values().map(|c| (c.cid, &**c)).collect();
        // Changes some hook wants, with the hooks that want them.
        let mut wanted: Vec<(&Change, &crate::schema::Collection, String, Vec<Id>)> = Vec::new();
        for ch in changes {
            let Some(c) = by_cid.get(&ch.coll) else {
                continue;
            };
            let op = op_name(ch.op);
            let ids: Vec<Id> = hooks
                .iter()
                .filter(|h| h.matches(&c.def.name, op))
                .map(|h| h.id)
                .collect();
            if !ids.is_empty() {
                wanted.push((ch, c, format!("{}.{op}", c.def.name), ids));
            }
        }
        let mut writes = Vec::new();
        let now = now_us();
        for cid in wanted
            .iter()
            .map(|w| w.0.coll)
            .collect::<std::collections::BTreeSet<_>>()
        {
            let items: Vec<_> = wanted.iter().filter(|w| w.0.coll == cid).collect();
            let c = items[0].1;
            let mut ids: Vec<Id> = items
                .iter()
                .filter(|w| w.0.op != Op::Delete)
                .map(|w| w.0.rid)
                .collect();
            ids.sort_unstable();
            ids.dedup();
            let rows = if ids.is_empty() {
                Vec::new()
            } else {
                app.rt_fetch(c, schema.version, &ids).await
            };
            let multi = app.load_multi_rels(c, &rows).await.unwrap_or_default();
            let by_id: HashMap<Id, &Vec<Value>> = rows
                .iter()
                .filter_map(|r| id_of(r.first()?).map(|id| (id, r)))
                .collect();
            for (ch, _, event, hook_ids) in items {
                let mut rec = Vec::with_capacity(256);
                match by_id.get(&ch.rid) {
                    Some(row) => {
                        let rm = row_multi(row, &multi);
                        // Hidden fields never leave the server.
                        codec::write_record_ext(&mut rec, c, row, false, None, None, &rm, &[]);
                    }
                    None => {
                        rec.extend_from_slice(b"{\"id\":\"");
                        rec.extend_from_slice(&monolith_store::id::encode(&ch.rid));
                        rec.extend_from_slice(b"\"}");
                    }
                }
                for hook in hook_ids {
                    let did = new_id();
                    let payload = envelope(
                        &encode_string(&did),
                        &app.name,
                        event,
                        &c.def.name,
                        ch.seq,
                        ch.ts,
                        &rec,
                    );
                    writes.push(delivery_insert(
                        &did, hook, &app.name, event, ch.seq, payload, now,
                    ));
                }
            }
        }
        // A delivery for a change that was already enqueued would duplicate it, so the cursor
        // moves in the same commit as the rows.
        let mut op = monolith_store::WriteOp {
            actor: None,
            mutations: writes,
        };
        op.mutations.push(
            sys_write(
                "INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)",
                [0; 16],
                vec![
                    Value::Text(format!("wh_cursor:{}", app.name).into()),
                    Value::Blob(last.to_le_bytes().to_vec().into()),
                ],
            )
            .mutations
            .remove(0),
        );
        self.sys.write(op).await?;
        self.notify.notify_one();
        Ok(())
    }
}

fn delivery_insert(
    did: &Id,
    hook: &Id,
    app: &str,
    event: &str,
    seq: u64,
    payload: Vec<u8>,
    now: i64,
) -> monolith_store::Mutation {
    sys_write(
        "INSERT INTO webhook_deliveries (id, hook, app, event, seq, payload, status, attempts, \
         next_at, created) VALUES (?, ?, ?, ?, ?, ?, 'pending', 0, ?, ?)",
        *did,
        vec![
            Value::Blob((*did).into()),
            Value::Blob((*hook).into()),
            Value::Text(app.into()),
            Value::Text(event.into()),
            Value::Int(seq as i64),
            Value::Blob(payload.into()),
            Value::Int(now),
            Value::Int(now),
        ],
    )
    .mutations
    .remove(0)
}

fn envelope(
    id: &str,
    app: &str,
    event: &str,
    coll: &str,
    seq: u64,
    ts: i64,
    record: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(record.len() + 200);
    out.extend_from_slice(b"{\"id\":");
    codec::write_str(&mut out, id);
    out.extend_from_slice(b",\"app\":");
    codec::write_str(&mut out, app);
    out.extend_from_slice(b",\"event\":");
    codec::write_str(&mut out, event);
    out.extend_from_slice(b",\"collection\":");
    codec::write_str(&mut out, coll);
    out.extend_from_slice(format!(",\"seq\":{seq},\"createdAt\":").as_bytes());
    codec::write_str(&mut out, &format_us(ts));
    out.extend_from_slice(b",\"record\":");
    out.extend_from_slice(record);
    out.push(b'}');
    out
}

/// One task per open app: wakes on the change bus, then drains `_changes` from the cursor.
pub(crate) fn spawn_dispatcher(wh: Arc<WhHub>, app: &Arc<App>) {
    let (name, weak, mut rx) = (
        app.name.clone(),
        Arc::downgrade(app),
        app.store().subscribe(),
    );
    let app = app.clone();
    tokio::spawn(async move {
        let mut cursor = wh.start_cursor(&app).await;
        drop(app);
        wh.cursors
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.clone(), cursor);
        loop {
            loop {
                let Some(app) = weak.upgrade() else {
                    return;
                };
                let Ok(changes) = app.store().changes_since(cursor, BATCH).await else {
                    break;
                };
                let Some(last) = changes.last().map(|c| c.seq) else {
                    break;
                };
                if let Err(e) = wh.enqueue(&app, &changes).await {
                    tracing::warn!(app = %name, error = %e, "webhook enqueue failed; retrying on the next change");
                    break;
                }
                cursor = last;
                wh.cursors
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(name.clone(), cursor);
            }
            if let Err(tokio::sync::broadcast::error::RecvError::Closed) = rx.recv().await {
                return;
            }
        }
    });
}

struct Due {
    id: Id,
    event: String,
    payload: Vec<u8>,
    attempts: i64,
    url: String,
    secret: String,
}

async fn worker(weak: Weak<WhHub>, notify: Arc<Notify>) {
    let mut last_gc = 0i64;
    loop {
        let Some(wh) = weak.upgrade() else {
            return;
        };
        let now = now_us();
        let due = wh.due(now).await;
        if now - last_gc > 60 * SECOND_US {
            last_gc = now;
            let _ = wh
                .sys
                .write(sys_delete(
                    "DELETE FROM webhook_deliveries WHERE (status = 'ok' AND created < ?) OR \
                     (status = 'dead' AND created < ?)",
                    vec![
                        Value::Int(now - KEEP_OK_US),
                        Value::Int(now - 4 * KEEP_OK_US),
                    ],
                ))
                .await;
        }
        if due.is_empty() {
            drop(wh);
            tokio::select! {
                () = notify.notified() => {}
                () = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
            continue;
        }
        futures_util::future::join_all(due.into_iter().map(|d| wh.attempt(d))).await;
    }
}

impl WhHub {
    async fn due(&self, now: i64) -> Vec<Due> {
        let rows = self
            .sys
            .query(Query {
                sql: "SELECT d.id, d.event, d.payload, d.attempts, h.url, h.secret \
                      FROM webhook_deliveries d JOIN webhooks h ON h.id = d.hook \
                      WHERE d.status = 'pending' AND d.next_at <= ? ORDER BY d.next_at LIMIT ?"
                    .into(),
                params: vec![Value::Int(now), Value::Int(DUE_BATCH as i64)],
            })
            .await
            .unwrap_or_default();
        rows.iter()
            .filter_map(|r| {
                Some(Due {
                    id: id_of(&r[0])?,
                    event: text(&r[1]).to_owned(),
                    payload: match &r[2] {
                        Value::Blob(b) => b.to_vec(),
                        _ => return None,
                    },
                    attempts: int(&r[3]),
                    url: text(&r[4]).to_owned(),
                    secret: text(&r[5]).to_owned(),
                })
            })
            .collect()
    }

    async fn attempt(&self, d: Due) {
        let ts = now_us() / SECOND_US;
        let mut signed = format!("{ts}.").into_bytes();
        signed.extend_from_slice(&d.payload);
        let sig = hmac_sha256_hex(d.secret.as_bytes(), &signed);
        let res = self
            .client
            .post(&d.url)
            .timeout(TIMEOUT)
            .header("content-type", "application/json")
            .header("x-monolith-event", &d.event)
            .header("x-monolith-delivery", encode_string(&d.id))
            .header("x-monolith-signature", format!("t={ts},v1={sig}"))
            .body(d.payload.clone())
            .send()
            .await;
        let attempts = d.attempts + 1;
        let (code, err) = match res {
            Ok(r) => (Some(r.status().as_u16()), None),
            Err(e) => (None, Some(e.without_url().to_string())),
        };
        let ok = code.is_some_and(|c| (200..300).contains(&c));
        let err = err.or_else(|| (!ok).then(|| format!("HTTP {}", code.unwrap_or(0))));
        let (status, next) = if ok {
            ("ok", now_us())
        } else if attempts >= MAX_ATTEMPTS {
            ("dead", now_us())
        } else {
            let secs = (5i64 << (attempts - 1).min(10)).min(3600);
            ("pending", now_us() + secs * SECOND_US)
        };
        let _ = self
            .sys
            .write(sys_write(
                "UPDATE webhook_deliveries SET status = ?, attempts = ?, next_at = ?, \
                 last_status = ?, last_error = ? WHERE id = ?",
                d.id,
                vec![
                    Value::Text(status.into()),
                    Value::Int(attempts),
                    Value::Int(next),
                    code.map_or(Value::Null, |c| Value::Int(i64::from(c))),
                    err.map_or(Value::Null, |e| {
                        Value::Text(e.chars().take(300).collect::<String>().into())
                    }),
                    Value::Blob(d.id.into()),
                ],
            ))
            .await;
    }
}

const HOOK_COLS: &str = "id, name, url, events, enabled, created, secret, \
    (SELECT count(*) FROM webhook_deliveries d WHERE d.hook = webhooks.id AND d.status = 'pending'), \
    (SELECT count(*) FROM webhook_deliveries d WHERE d.hook = webhooks.id AND d.status = 'dead')";

fn hook_info(r: &[Value]) -> Option<WebhookInfo> {
    Some(WebhookInfo {
        id: encode_string(&id_of(&r[0])?),
        name: text(&r[1]).to_owned(),
        url: text(&r[2]).to_owned(),
        events: serde_json::from_str(text(&r[3])).unwrap_or_default(),
        enabled: int(&r[4]) != 0,
        created: format_us(int(&r[5])),
        secret_hint: text(&r[6]).chars().take(12).collect(),
        pending: int(&r[7]) as u64,
        dead: int(&r[8]) as u64,
    })
}

fn new_secret() -> CoreResult<String> {
    let b = random_bytes::<32>().map_err(|e| CoreError::Internal(e.to_string()))?;
    Ok(format!("whsec_{}", hex(&b)))
}

impl Registry {
    /// Creates a hook; the returned signing secret is shown only this once.
    pub async fn create_webhook(
        &self,
        app: &str,
        input: WebhookIn,
    ) -> CoreResult<(WebhookInfo, String)> {
        let loaded = self.app(app).await?; // also starts the app's dispatcher
        validate(&input)?;
        let n = self
            .sys
            .query(Query {
                sql: "SELECT count(*) FROM webhooks WHERE app = ?".into(),
                params: vec![Value::Text(app.into())],
            })
            .await?;
        if n.first().and_then(|r| r.first()).map_or(0, int) as usize >= MAX_HOOKS {
            return Err(CoreError::Conflict(format!(
                "an app can have at most {MAX_HOOKS} webhooks"
            )));
        }
        let (id, secret, now) = (new_id(), new_secret()?, now_us());
        let events =
            serde_json::to_string(&input.events).map_err(|e| CoreError::Internal(e.to_string()))?;
        self.sys
            .write(sys_write(
                "INSERT INTO webhooks (id, app, name, url, secret, events, enabled, created) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                id,
                vec![
                    Value::Blob(id.into()),
                    Value::Text(app.into()),
                    Value::Text(input.name.trim().into()),
                    Value::Text(input.url.as_str().into()),
                    Value::Text(secret.as_str().into()),
                    Value::Text(events.into()),
                    Value::Int(i64::from(input.enabled)),
                    Value::Int(now),
                ],
            ))
            .await?;
        drop(loaded);
        self.wh.reload(app).await?;
        let info = self.webhook_info(app, &id).await?;
        Ok((info, secret))
    }

    async fn webhook_info(&self, app: &str, id: &Id) -> CoreResult<WebhookInfo> {
        self.sys
            .query(Query {
                sql: format!("SELECT {HOOK_COLS} FROM webhooks WHERE id = ? AND app = ?").into(),
                params: vec![Value::Blob((*id).into()), Value::Text(app.into())],
            })
            .await?
            .first()
            .and_then(|r| hook_info(r))
            .ok_or(CoreError::NotFound("webhook"))
    }

    pub async fn list_webhooks(&self, app: &str) -> CoreResult<Vec<WebhookInfo>> {
        self.app_info(app).await?;
        let rows = self
            .sys
            .query(Query {
                sql: format!("SELECT {HOOK_COLS} FROM webhooks WHERE app = ? ORDER BY created")
                    .into(),
                params: vec![Value::Text(app.into())],
            })
            .await?;
        Ok(rows.iter().filter_map(|r| hook_info(r)).collect())
    }

    /// Replaces name, URL, events and the enabled flag; the secret stays.
    pub async fn update_webhook(
        &self,
        app: &str,
        id: &str,
        input: WebhookIn,
    ) -> CoreResult<WebhookInfo> {
        let id = decode(id).ok_or(CoreError::NotFound("webhook"))?;
        validate(&input)?;
        let events =
            serde_json::to_string(&input.events).map_err(|e| CoreError::Internal(e.to_string()))?;
        self.sys
            .write(sys_write(
                "UPDATE webhooks SET name = ?, url = ?, events = ?, enabled = ? \
                 WHERE id = ? AND app = ?",
                id,
                vec![
                    Value::Text(input.name.trim().into()),
                    Value::Text(input.url.as_str().into()),
                    Value::Text(events.into()),
                    Value::Int(i64::from(input.enabled)),
                    Value::Blob(id.into()),
                    Value::Text(app.into()),
                ],
            ))
            .await
            .map_err(|e| match e {
                monolith_store::Error::NotFound => CoreError::NotFound("webhook"),
                e => e.into(),
            })?;
        self.app(app).await?;
        self.wh.reload(app).await?;
        self.webhook_info(app, &id).await
    }

    pub async fn delete_webhook(&self, app: &str, id: &str) -> CoreResult<()> {
        let id = decode(id).ok_or(CoreError::NotFound("webhook"))?;
        self.sys
            .write(sys_delete(
                "DELETE FROM webhooks WHERE id = ? AND app = ?",
                vec![Value::Blob(id.into()), Value::Text(app.into())],
            ))
            .await
            .map_err(|e| match e {
                monolith_store::Error::NotFound => CoreError::NotFound("webhook"),
                e => e.into(),
            })?;
        let _ = self
            .sys
            .write(sys_delete(
                "DELETE FROM webhook_deliveries WHERE hook = ?",
                vec![Value::Blob(id.into())],
            ))
            .await;
        self.wh.reload(app).await
    }

    pub(crate) async fn drop_webhooks(&self, app: &str) -> CoreResult<()> {
        for (sql, p) in [
            (
                "DELETE FROM webhook_deliveries WHERE app = ?",
                Value::Text(app.into()),
            ),
            (
                "DELETE FROM webhooks WHERE app = ?",
                Value::Text(app.into()),
            ),
            (
                "DELETE FROM settings WHERE key = ?",
                Value::Text(format!("wh_cursor:{app}").into()),
            ),
        ] {
            match self.sys.write(sys_delete(sql, vec![p])).await {
                Ok(_) | Err(monolith_store::Error::NotFound) => {}
                Err(e) => return Err(e.into()),
            }
        }
        self.wh.reload(app).await
    }

    /// Queues a `ping` delivery so the receiver can be checked end to end.
    pub async fn test_webhook(&self, app: &str, id: &str) -> CoreResult<DeliveryInfo> {
        let hook = decode(id).ok_or(CoreError::NotFound("webhook"))?;
        self.webhook_info(app, &hook).await?;
        let (did, now) = (new_id(), now_us());
        let payload = envelope(&encode_string(&did), app, "ping", "", 0, now, b"{}");
        self.sys
            .write(monolith_store::WriteOp {
                actor: None,
                mutations: vec![delivery_insert(&did, &hook, app, "ping", 0, payload, now)],
            })
            .await?;
        self.wh.notify.notify_one();
        self.delivery_info(app, &hook, &did).await
    }

    const DELIVERY_COLS: &'static str =
        "id, event, seq, status, attempts, next_at, last_status, last_error, created";

    async fn delivery_info(&self, app: &str, hook: &Id, id: &Id) -> CoreResult<DeliveryInfo> {
        self.sys
            .query(Query {
                sql: format!(
                    "SELECT {} FROM webhook_deliveries WHERE id = ? AND hook = ? AND app = ?",
                    Self::DELIVERY_COLS
                )
                .into(),
                params: vec![
                    Value::Blob((*id).into()),
                    Value::Blob((*hook).into()),
                    Value::Text(app.into()),
                ],
            })
            .await?
            .first()
            .and_then(|r| delivery_from(r))
            .ok_or(CoreError::NotFound("delivery"))
    }

    /// Latest deliveries of one hook, newest first; `status` is `pending`, `ok` or `dead`.
    pub async fn list_deliveries(
        &self,
        app: &str,
        id: &str,
        status: Option<&str>,
        limit: usize,
    ) -> CoreResult<Vec<DeliveryInfo>> {
        let hook = decode(id).ok_or(CoreError::NotFound("webhook"))?;
        self.webhook_info(app, &hook).await?;
        if status.is_some_and(|s| !matches!(s, "pending" | "ok" | "dead")) {
            return Err(CoreError::BadRequest(
                "status must be pending, ok or dead".into(),
            ));
        }
        let rows = self
            .sys
            .query(Query {
                sql: format!(
                    "SELECT {} FROM webhook_deliveries WHERE hook = ? AND (? IS NULL OR status = ?) \
                     ORDER BY created DESC, id DESC LIMIT ?",
                    Self::DELIVERY_COLS
                )
                .into(),
                params: vec![
                    Value::Blob(hook.into()),
                    status.map_or(Value::Null, |s| Value::Text(s.into())),
                    status.map_or(Value::Null, |s| Value::Text(s.into())),
                    Value::Int(limit.clamp(1, 200) as i64),
                ],
            })
            .await?;
        Ok(rows.iter().filter_map(|r| delivery_from(r)).collect())
    }

    /// Puts a dead or finished delivery back in the queue for a fresh run of attempts.
    pub async fn retry_delivery(&self, app: &str, id: &str, delivery: &str) -> CoreResult<()> {
        let hook = decode(id).ok_or(CoreError::NotFound("webhook"))?;
        let did = decode(delivery).ok_or(CoreError::NotFound("delivery"))?;
        self.delivery_info(app, &hook, &did).await?;
        self.sys
            .write(sys_write(
                "UPDATE webhook_deliveries SET status = 'pending', attempts = 0, next_at = ? \
                 WHERE id = ?",
                did,
                vec![Value::Int(now_us()), Value::Blob(did.into())],
            ))
            .await?;
        self.wh.notify.notify_one();
        Ok(())
    }
}

fn delivery_from(r: &[Value]) -> Option<DeliveryInfo> {
    let status = text(&r[3]).to_owned();
    Some(DeliveryInfo {
        id: encode_string(&id_of(&r[0])?),
        event: text(&r[1]).to_owned(),
        seq: int(&r[2]) as u64,
        next_attempt: (status == "pending").then(|| format_us(int(&r[5]))),
        status,
        attempts: int(&r[4]) as u32,
        last_status: match &r[6] {
            Value::Int(n) => u16::try_from(*n).ok(),
            _ => None,
        },
        last_error: match &r[7] {
            Value::Text(t) => Some(t.to_string()),
            _ => None,
        },
        created: format_us(int(&r[8])),
    })
}
