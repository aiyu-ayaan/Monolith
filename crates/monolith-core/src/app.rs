//! One app's runtime: its store, the current compiled schema, record operations.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use monolith_rules::{Auth, Class, Compiled, Ctx, Mode, compile};
use monolith_store::id::{decode as decode_id, new_id};
use monolith_store::{
    AppStore, Error as StoreError, Id, MigrationPlan, Mutation, Op, Query, SqliteConfig,
    SqliteStore, Value, WriteOp,
};

use crate::codec::{self, Body};
use crate::ddl::{self, sql_str};
use crate::debug::RequestLog;
use crate::error::{CoreError, CoreResult};
use crate::list::{self, ListPlan};
use crate::metrics::{Metrics, spawn_change_counter};
use crate::schema::{Collection, CollectionDef, FieldType, Schema};

pub const MAX_LIMIT: usize = 1000;
pub const DEFAULT_LIMIT: usize = 50;
pub const MAX_BATCH: usize = 500;
pub const MAX_EXPAND_DEPTH: usize = 4;
const PLAN_CACHE: usize = 4096;
const ID_PAST_US: i64 = 30 * 86_400_000_000;
const ID_FUTURE_US: i64 = 5 * 60_000_000;

const META_DDL: &str = "
CREATE TABLE IF NOT EXISTS _collections (name TEXT PRIMARY KEY, cid INTEGER NOT NULL UNIQUE, def TEXT NOT NULL) STRICT;
CREATE TABLE IF NOT EXISTS _meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL) STRICT;
INSERT OR IGNORE INTO _meta (key, value) VALUES ('schema_version', 0);
";

pub fn now_us() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_micros() as i64)
}

/// Who is calling. Superusers bypass rules; everyone else is checked by them.
#[derive(Debug, Clone, Default)]
pub struct Principal {
    pub superuser: bool,
    pub auth: Option<Auth>,
}

impl Principal {
    pub fn superuser() -> Self {
        Self {
            superuser: true,
            auth: None,
        }
    }

    fn ctx<'a>(&'a self, body: Option<(&'a [Value], u64)>, now: i64) -> Ctx<'a> {
        Ctx {
            auth: self.auth.as_ref(),
            body: body.map(|(b, _)| b),
            body_isset: body.map(|(_, m)| m),
            now_us: now,
        }
    }

    fn actor(&self) -> Option<Id> {
        self.auth.as_ref().map(|a| a.id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TotalMode {
    #[default]
    None,
    Exact,
    Estimate,
}

#[derive(Debug, Default, Clone)]
pub struct ListParams {
    pub filter: String,
    pub sort: String,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub fields: Option<String>,
    pub expand: Option<String>,
    pub total: TotalMode,
}

#[derive(Debug, Default, Clone)]
pub struct WriteOpts {
    pub dry_run: bool,
    pub if_match: Option<u64>,
    pub expand: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct ReadOpts {
    pub fields: Option<String>,
    pub expand: Option<String>,
    pub if_none_match: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct RecordOut {
    pub json: Vec<u8>,
    pub version: u64,
    pub created: bool,
    pub not_modified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchKind {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Clone)]
pub struct BatchItem<'a> {
    pub kind: BatchKind,
    pub collection: &'a str,
    pub id: Option<&'a str>,
    pub body: Option<&'a [u8]>,
}

type PlanKey = (
    u64,
    String,
    bool,
    String,
    String,
    Option<Vec<usize>>,
    Option<u32>,
);
type ExpandMaps = Vec<(HashMap<Id, Vec<u8>>, Option<HashMap<Id, Vec<Id>>>)>;

pub struct App {
    pub name: String,
    store: SqliteStore,
    schema: RwLock<Arc<Schema>>,
    /// Serializes schema changes; record traffic never takes it.
    schema_lock: tokio::sync::Mutex<()>,
    // shortcut: cleared when full instead of LRU; fine until profiles say otherwise.
    plans: Mutex<HashMap<PlanKey, Arc<ListPlan>>>,
    pub(crate) logs: Mutex<VecDeque<RequestLog>>,
    pub metrics: Arc<Metrics>,
    pub(crate) db_path: PathBuf,
}

fn rule_ref<'a>(
    p: &Principal,
    r: &'a Option<Compiled>,
    what: &'static str,
) -> CoreResult<Option<&'a Compiled>> {
    if p.superuser {
        return Ok(None);
    }
    r.as_ref().map(Some).ok_or(CoreError::Forbidden(what))
}

fn record_id(s: &str) -> CoreResult<Id> {
    decode_id(s).ok_or(CoreError::NotFound("record"))
}

fn row_version(row: &[Value]) -> u64 {
    match row.get(3) {
        Some(Value::Int(v)) => *v as u64,
        _ => 0,
    }
}

fn parse_fields(c: &Collection, fields: Option<&str>, su: bool) -> CoreResult<Option<Vec<usize>>> {
    match fields {
        None => Ok(None),
        Some(f) => Ok(Some(
            f.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|name| {
                    c.slot(name)
                        .filter(|&s| c.field_at(s).is_none_or(|f| su || !f.hidden))
                        .ok_or_else(|| {
                            CoreError::BadRequest(format!("unknown field '{name}' in fields"))
                        })
                })
                .collect::<CoreResult<Vec<_>>>()?,
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExpandKind {
    Forward { slot: usize, multi: bool },
    BackSingle { rel_field: String, rel_slot: usize },
    BackMulti { junction: String },
}

#[derive(Debug, Clone)]
struct ExpandNode {
    field: String,
    kind: ExpandKind,
    target: Arc<Collection>,
    children: Vec<ExpandNode>,
}

type MultiRelMaps = Vec<(usize, HashMap<Id, Vec<Id>>)>;

fn row_multi<'a>(row: &[Value], maps: &'a MultiRelMaps) -> Vec<(usize, &'a [Id])> {
    let Some(Value::Blob(b)) = row.first() else {
        return Vec::new();
    };
    let Ok(id) = <&Id>::try_from(&b[..]) else {
        return Vec::new();
    };
    maps.iter()
        .map(|(slot, m)| (*slot, m.get(id).map_or(&[][..], Vec::as_slice)))
        .collect()
}

fn build_row_expand<'a>(
    row: &[Value],
    multi_maps: &MultiRelMaps,
    nodes: &'a [ExpandNode],
    exp_maps: &ExpandMaps,
) -> Vec<(&'a str, Vec<u8>)> {
    let row_id = row.first().and_then(|v| match v {
        Value::Blob(b) => <&Id>::try_from(&b[..]).ok(),
        _ => None,
    });
    nodes
        .iter()
        .zip(exp_maps)
        .filter_map(|(n, (map, back_map))| match &n.kind {
            ExpandKind::Forward { slot, multi: false } => match row.get(*slot) {
                Some(Value::Blob(b)) => <&Id>::try_from(&b[..])
                    .ok()
                    .and_then(|id| map.get(id))
                    .map(|bytes| (n.field.as_str(), bytes.clone())),
                _ => None,
            },
            ExpandKind::Forward { slot, multi: true } => {
                let rid = row_id?;
                let targets = multi_maps
                    .iter()
                    .find(|(s, _)| *s == *slot)
                    .and_then(|(_, m)| m.get(rid))
                    .map_or(&[][..], Vec::as_slice);
                Some((n.field.as_str(), encode_expand_array(targets, map)))
            }
            ExpandKind::BackSingle { .. } | ExpandKind::BackMulti { .. } => {
                let rid = row_id?;
                let targets = back_map
                    .as_ref()
                    .and_then(|bm| bm.get(rid))
                    .map_or(&[][..], Vec::as_slice);
                Some((n.field.as_str(), encode_expand_array(targets, map)))
            }
        })
        .collect()
}

fn encode_expand_array(targets: &[Id], map: &HashMap<Id, Vec<u8>>) -> Vec<u8> {
    let mut arr = Vec::new();
    arr.push(b'[');
    let mut first = true;
    for tid in targets {
        if let Some(bytes) = map.get(tid) {
            if !first {
                arr.push(b',');
            }
            first = false;
            arr.extend_from_slice(bytes);
        }
    }
    arr.push(b']');
    arr
}

fn parse_expand(
    schema: &Schema,
    c: &Collection,
    expand: Option<&str>,
    su: bool,
) -> CoreResult<Vec<ExpandNode>> {
    let Some(expand) = expand.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Vec::new());
    };
    let mut roots: Vec<ExpandNode> = Vec::new();
    for raw_path in expand.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let segs: Vec<&str> = raw_path.split('.').collect();
        if segs.is_empty() || segs.len() > MAX_EXPAND_DEPTH {
            return Err(CoreError::BadRequest(format!(
                "expand path '{raw_path}' exceeds max depth {MAX_EXPAND_DEPTH}"
            )));
        }
        let mut cur_coll = c;
        let mut level = &mut roots;
        for seg in segs {
            let (kind, target) = if let Some(slot) = cur_coll.slot(seg) {
                let f = cur_coll
                    .field_at(slot)
                    .filter(|f| f.kind == FieldType::Relation && (su || !f.hidden))
                    .ok_or_else(|| {
                        CoreError::BadRequest(format!(
                            "field '{seg}' on '{}' is not a relation",
                            cur_coll.def.name
                        ))
                    })?;
                let target_name = f.collection.as_deref().ok_or_else(|| {
                    CoreError::BadRequest(format!("relation '{seg}' has no target collection"))
                })?;
                let target = schema.get(target_name)?;
                (
                    ExpandKind::Forward {
                        slot,
                        multi: f.is_multi_rel(),
                    },
                    target,
                )
            } else if let Some((src_coll_name, rel_field)) = seg.split_once("_via_")
                && let Some(src_coll) = schema.collections.get(src_coll_name)
                && let Some(rel_slot) = src_coll.slot(rel_field)
                && let Some(f) = src_coll.field_at(rel_slot)
                && f.kind == FieldType::Relation
                && (su || !f.hidden)
                && f.collection.as_deref() == Some(cur_coll.def.name.as_str())
            {
                let kind = if f.is_multi_rel() {
                    ExpandKind::BackMulti {
                        junction: f.junction_table(&src_coll.def.name),
                    }
                } else {
                    ExpandKind::BackSingle {
                        rel_field: rel_field.to_owned(),
                        rel_slot,
                    }
                };
                (kind, src_coll)
            } else {
                return Err(CoreError::BadRequest(format!(
                    "unknown expand field '{seg}' on '{}'",
                    cur_coll.def.name
                )));
            };
            let pos = match level.iter().position(|n| n.field == seg) {
                Some(i) => i,
                None => {
                    level.push(ExpandNode {
                        field: seg.to_owned(),
                        kind,
                        target: target.clone(),
                        children: Vec::new(),
                    });
                    level.len() - 1
                }
            };
            cur_coll = target;
            level = &mut level[pos].children;
        }
    }
    Ok(roots)
}

/// Client-made ids must be real UUIDv7s with a sane timestamp (doc 05 §2).
fn check_client_id(id: &Id, now: i64) -> CoreResult<()> {
    let ms = id[..6].iter().fold(0i64, |a, b| a << 8 | i64::from(*b));
    let us = ms * 1000;
    if id[6] >> 4 != 7 || id[8] & 0xc0 != 0x80 || us < now - ID_PAST_US || us > now + ID_FUTURE_US {
        return Err(CoreError::Validation(vec![crate::FieldError::new(
            "id",
            "invalid_format",
            "id must be a UUIDv7 created within the last 30 days",
        )]));
    }
    Ok(())
}

impl App {
    /// Opens (or creates) the app database at `path` and loads its schema. Record ops
    /// are counted into the app's metrics and into `global`.
    pub async fn open(name: &str, path: &Path, global: Arc<Metrics>) -> CoreResult<App> {
        let p = path.to_owned();
        let store =
            tokio::task::spawn_blocking(move || SqliteStore::open(&p, &SqliteConfig::default()))
                .await
                .map_err(|e| CoreError::Internal(e.to_string()))??;
        store
            .apply_schema(MigrationPlan {
                statements: vec![META_DDL.into()],
            })
            .await?;
        let version = match store
            .query(Query {
                sql: "SELECT value FROM _meta WHERE key = 'schema_version'".into(),
                params: vec![],
            })
            .await?
            .first()
            .and_then(|r| r.first())
        {
            Some(Value::Int(v)) => *v as u64,
            _ => 0,
        };
        let rows = store
            .query(Query {
                sql: "SELECT name, cid, def FROM _collections".into(),
                params: vec![],
            })
            .await?;
        let mut defs = HashMap::new();
        for r in rows {
            let (Value::Text(name), Value::Int(cid), Value::Text(def)) = (&r[0], &r[1], &r[2])
            else {
                return Err(CoreError::Internal("corrupt _collections row".into()));
            };
            let def: CollectionDef = serde_json::from_str(def)
                .map_err(|e| CoreError::Internal(format!("collection {name}: {e}")))?;
            defs.insert(name.to_string(), (*cid as u32, def));
        }
        let schema = Schema::build(version, defs)?;
        let metrics = Arc::new(Metrics::new());
        spawn_change_counter(store.subscribe(), [metrics.clone(), global]);
        Ok(App {
            name: name.to_owned(),
            store,
            schema: RwLock::new(Arc::new(schema)),
            schema_lock: tokio::sync::Mutex::new(()),
            plans: Mutex::new(HashMap::new()),
            logs: Mutex::new(VecDeque::new()),
            metrics,
            db_path: path.to_owned(),
        })
    }

    pub fn schema(&self) -> Arc<Schema> {
        self.schema
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn store(&self) -> &SqliteStore {
        &self.store
    }

    // ---------- collections ----------

    /// Creates (`create = true`) or replaces a collection, migrating its table.
    pub async fn put_collection(
        &self,
        def: CollectionDef,
        create: bool,
    ) -> CoreResult<Arc<Collection>> {
        let _guard = self.schema_lock.lock().await;
        let cur = self.schema();
        let old = cur.collections.get(&def.name).cloned();
        match (&old, create) {
            (Some(_), true) => {
                return Err(CoreError::Conflict(format!(
                    "collection '{}' exists",
                    def.name
                )));
            }
            (None, false) => return Err(CoreError::NotFound("collection")),
            _ => {}
        }
        let mut defs: HashMap<String, (u32, CollectionDef)> = cur
            .collections
            .iter()
            .map(|(n, c)| (n.clone(), (c.cid, c.def.clone())))
            .collect();
        let cid = old.as_ref().map_or_else(
            || defs.values().map(|d| d.0).max().unwrap_or(0) + 1,
            |c| c.cid,
        );
        defs.insert(def.name.clone(), (cid, def.clone()));
        let next = Schema::build(cur.version + 1, defs)?;
        let mut statements = ddl::plan(old.as_ref().map(|c| &c.def), &def)?;
        let json = serde_json::to_string(&def).map_err(|e| CoreError::Internal(e.to_string()))?;
        statements.push(format!(
            "INSERT INTO _collections (name, cid, def) VALUES ({}, {cid}, {}) ON CONFLICT(name) DO UPDATE SET def = excluded.def",
            sql_str(&def.name),
            sql_str(&json)
        ));
        statements.push("UPDATE _meta SET value = value + 1 WHERE key = 'schema_version'".into());
        self.store
            .apply_schema(MigrationPlan { statements })
            .await
            .map_err(|e| match e {
                StoreError::Engine(m) | StoreError::Constraint(m) => {
                    CoreError::BadRequest(format!("migration failed: {m}"))
                }
                e => e.into(),
            })?;
        let c = next.get(&def.name)?.clone();
        *self.schema.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(next);
        Ok(c)
    }

    pub async fn delete_collection(&self, name: &str) -> CoreResult<()> {
        let _guard = self.schema_lock.lock().await;
        let cur = self.schema();
        let old = cur.get(name)?;
        if let Some(other) = cur.collections.values().find(|c| {
            c.def.name != name
                && c.def
                    .fields
                    .iter()
                    .any(|f| f.collection.as_deref() == Some(name))
        }) {
            return Err(CoreError::Conflict(format!(
                "collection '{}' has a relation to '{name}'",
                other.def.name
            )));
        }
        let defs = cur
            .collections
            .iter()
            .filter(|(n, _)| n.as_str() != name)
            .map(|(n, c)| (n.clone(), (c.cid, c.def.clone())))
            .collect();
        let next = Schema::build(cur.version + 1, defs)?;
        let mut statements: Vec<String> = old
            .def
            .fields
            .iter()
            .filter(|f| f.is_multi_rel())
            .map(|f| format!("DROP TABLE \"{}\"", f.junction_table(name)))
            .collect();
        statements.push(format!("DROP TABLE \"{name}\""));
        statements.push(format!(
            "DELETE FROM _collections WHERE name = {}",
            sql_str(name)
        ));
        statements.push("UPDATE _meta SET value = value + 1 WHERE key = 'schema_version'".into());
        self.store
            .apply_schema(MigrationPlan { statements })
            .await?;
        *self.schema.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(next);
        Ok(())
    }

    // ---------- records ----------

    fn create_mutation_with_id(
        &self,
        p: &Principal,
        c: &Collection,
        json: &[u8],
        forced_id: Option<Id>,
        now: i64,
    ) -> CoreResult<Mutation> {
        let rule = rule_ref(p, &c.rules.create, "the create rule allows superusers only")?;
        let body = codec::decode_body(c, json, true)?;
        let id = match (forced_id, body.id) {
            (Some(fid), Some(bid)) if fid != bid => {
                return Err(CoreError::BadRequest("body id must match path id".into()));
            }
            (Some(id), _) | (None, Some(id)) => {
                check_client_id(&id, now)?;
                id
            }
            (None, None) => new_id(),
        };
        let mut params = vec![
            Value::Blob(id.into()),
            Value::Int(now),
            Value::Int(now),
            Value::Null,
            Value::Null,
        ];
        for (i, f) in c.def.fields.iter().enumerate() {
            if !f.is_multi_rel() {
                params.push(body.values[i].clone().unwrap_or(Value::Null));
            }
        }
        let sql = match rule {
            Some(r) => {
                let slots = body.rule_slots();
                params.extend(r.bind(&p.ctx(Some((&slots, body.isset_mask())), now)));
                c.sql.insert.clone()
            }
            None => c.sql.insert_su.clone(),
        };
        let mut post_sql = Vec::new();
        for (idx, _, _, ins_sql) in &c.multi_rels {
            if let Some(ids) = &body.multi[*idx] {
                for (pos, tid) in ids.iter().enumerate() {
                    post_sql.push((
                        ins_sql.clone(),
                        vec![
                            Value::Blob(id.into()),
                            Value::Blob((*tid).into()),
                            Value::Int(pos as i64),
                        ],
                    ));
                }
            }
        }
        Ok(Mutation {
            op: Op::Insert,
            coll: c.cid,
            rid: id,
            fields: None,
            sql,
            params,
            seq_slot: Some(3),
            hlc_slot: Some(4),
            post_sql,
        })
    }

    fn create_mutation(
        &self,
        p: &Principal,
        c: &Collection,
        json: &[u8],
        now: i64,
    ) -> CoreResult<Mutation> {
        self.create_mutation_with_id(p, c, json, None, now)
    }

    fn update_mutation_with(
        &self,
        p: &Principal,
        c: &Collection,
        id: Id,
        json: &[u8],
        if_match: Option<u64>,
        now: i64,
    ) -> CoreResult<(Mutation, Vec<Value>)> {
        let rule = rule_ref(p, &c.rules.update, "the update rule allows superusers only")?;
        let body: Body = codec::decode_body(c, json, false)?;
        let mut params = vec![Value::Int(now), Value::Null, Value::Null];
        let mut fields = 0u64;
        for (i, f) in c.def.fields.iter().enumerate() {
            if f.is_multi_rel() {
                fields |= u64::from(body.multi[i].is_some()) << i;
            } else {
                let v = &body.values[i];
                fields |= u64::from(v.is_some()) << i;
                params.push(Value::Int(v.is_some() as i64));
                params.push(v.clone().unwrap_or(Value::Null));
            }
        }
        params.push(Value::Blob(id.into()));
        let mut rule_binds = Vec::new();
        let base_sql = match rule {
            Some(r) => {
                let slots = body.rule_slots();
                rule_binds = r.bind(&p.ctx(Some((&slots, body.isset_mask())), now));
                params.extend(rule_binds.iter().cloned());
                &c.sql.update
            }
            None => &c.sql.update_su,
        };
        let sql = match if_match {
            None => base_sql.clone(),
            Some(v) => {
                params.push(Value::Int(v as i64));
                let ret = format!(" RETURNING {}", c.sql.cols);
                let prefix = base_sql.strip_suffix(&ret).unwrap_or(base_sql);
                format!("{prefix} AND t.\"_v\" = ?{ret}").into()
            }
        };
        let mut post_sql = Vec::new();
        for (idx, _, del_sql, ins_sql) in &c.multi_rels {
            if let Some(ids) = &body.multi[*idx] {
                post_sql.push((del_sql.clone(), vec![Value::Blob(id.into())]));
                for (pos, tid) in ids.iter().enumerate() {
                    post_sql.push((
                        ins_sql.clone(),
                        vec![
                            Value::Blob(id.into()),
                            Value::Blob((*tid).into()),
                            Value::Int(pos as i64),
                        ],
                    ));
                }
            }
        }
        Ok((
            Mutation {
                op: Op::Update,
                coll: c.cid,
                rid: id,
                fields: Some(fields),
                sql,
                params,
                seq_slot: Some(1),
                hlc_slot: Some(2),
                post_sql,
            },
            rule_binds,
        ))
    }

    fn update_mutation(
        &self,
        p: &Principal,
        c: &Collection,
        id: Id,
        json: &[u8],
        now: i64,
    ) -> CoreResult<Mutation> {
        self.update_mutation_with(p, c, id, json, None, now)
            .map(|(m, _)| m)
    }

    fn delete_mutation_with(
        &self,
        p: &Principal,
        c: &Collection,
        id: Id,
        if_match: Option<u64>,
        now: i64,
    ) -> CoreResult<(Mutation, Vec<Value>)> {
        let rule = rule_ref(p, &c.rules.delete, "the delete rule allows superusers only")?;
        let mut params = vec![Value::Blob(id.into())];
        let mut rule_binds = Vec::new();
        let base_sql = match rule {
            Some(r) => {
                rule_binds = r.bind(&p.ctx(None, now));
                params.extend(rule_binds.iter().cloned());
                &c.sql.delete
            }
            None => &c.sql.delete_su,
        };
        let sql = match if_match {
            None => base_sql.clone(),
            Some(v) => {
                params.push(Value::Int(v as i64));
                format!("{base_sql} AND t.\"_v\" = ?").into()
            }
        };
        Ok((
            Mutation {
                op: Op::Delete,
                coll: c.cid,
                rid: id,
                fields: None,
                sql,
                params,
                seq_slot: None,
                hlc_slot: None,
                post_sql: Vec::new(),
            },
            rule_binds,
        ))
    }

    fn delete_mutation(
        &self,
        p: &Principal,
        c: &Collection,
        id: Id,
        now: i64,
    ) -> CoreResult<Mutation> {
        self.delete_mutation_with(p, c, id, None, now)
            .map(|(m, _)| m)
    }

    async fn write_one(
        &self,
        p: &Principal,
        m: Mutation,
        dry_run: bool,
    ) -> CoreResult<monolith_store::WriteResult> {
        let insert = m.op == Op::Insert;
        let op = WriteOp {
            actor: p.actor(),
            mutations: vec![m],
        };
        let res = if dry_run {
            self.store.write_dry_run(op).await
        } else {
            self.store.write(op).await
        };
        res.map_err(|e| match e {
            // An insert only "misses" when its create rule said no.
            StoreError::NotFound if insert => {
                CoreError::Forbidden("the create rule rejected this record")
            }
            e => e.into(),
        })
    }

    /// Distinguishes a 412 `If-Match` version mismatch from a 404 (missing or rule-hidden).
    async fn check_if_match_miss(
        &self,
        p: &Principal,
        c: &Collection,
        id: Id,
        rule: &Option<Compiled>,
        rule_binds: Vec<Value>,
    ) -> CoreError {
        let rule_sql = if p.superuser {
            "1"
        } else {
            match rule {
                Some(r) => r.sql.as_str(),
                None => return CoreError::NotFound("record"),
            }
        };
        let mut params = vec![Value::Blob(id.into())];
        if !p.superuser {
            params.extend(rule_binds);
        }
        let sql = format!(
            "SELECT 1 FROM \"{}\" AS t WHERE t.\"id\" = ? AND ({rule_sql})",
            c.def.name
        );
        match self
            .store
            .query(Query {
                sql: sql.into(),
                params,
            })
            .await
        {
            Ok(rows) if !rows.is_empty() => {
                CoreError::PreconditionFailed("record _v does not match If-Match")
            }
            _ => CoreError::NotFound("record"),
        }
    }

    /// Batched junction lookup for all multi-relation fields on `c` across `rows`.
    async fn load_multi_rels(
        &self,
        c: &Collection,
        rows: &[Vec<Value>],
    ) -> CoreResult<MultiRelMaps> {
        if c.multi_rels.is_empty() || rows.is_empty() {
            return Ok(Vec::new());
        }
        let owners: Vec<Value> = rows
            .iter()
            .filter_map(|r| match r.first() {
                Some(v @ Value::Blob(_)) => Some(v.clone()),
                _ => None,
            })
            .collect();
        if owners.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = vec!["?"; owners.len()].join(", ");
        let mut out = Vec::with_capacity(c.multi_rels.len());
        for &(field_idx, slot, _, _) in &c.multi_rels {
            let j = c.def.fields[field_idx].junction_table(&c.def.name);
            let sql = format!(
                "SELECT \"owner\", \"target\" FROM \"{j}\" WHERE \"owner\" IN ({placeholders}) ORDER BY \"owner\", \"pos\""
            );
            let jrows = self
                .store
                .query(Query {
                    sql: sql.into(),
                    params: owners.clone(),
                })
                .await?;
            let mut map: HashMap<Id, Vec<Id>> = HashMap::new();
            for jr in jrows {
                if let [Value::Blob(ob), Value::Blob(tb)] = &jr[..]
                    && let (Ok(oid), Ok(tid)) = (<&Id>::try_from(&ob[..]), <&Id>::try_from(&tb[..]))
                {
                    map.entry(*oid).or_default().push(*tid);
                }
            }
            out.push((slot, map));
        }
        Ok(out)
    }

    fn resolve_expand<'a>(
        &'a self,
        p: &'a Principal,
        rows: &'a [Vec<Value>],
        multi_maps: &'a MultiRelMaps,
        nodes: &'a [ExpandNode],
        now: i64,
    ) -> Pin<Box<dyn Future<Output = CoreResult<ExpandMaps>> + Send + 'a>> {
        Box::pin(async move {
            let mut out = Vec::with_capacity(nodes.len());
            for node in nodes {
                if !p.superuser && node.target.rules.view.is_none() {
                    out.push((HashMap::new(), None));
                    continue;
                }
                let mut back_map: Option<HashMap<Id, Vec<Id>>> = None;
                let (where_col, ids): (&str, Vec<Id>) = match &node.kind {
                    ExpandKind::Forward { slot, multi: true } => {
                        let mut ids = Vec::new();
                        if let Some((_, m)) = multi_maps.iter().find(|(s, _)| *s == *slot) {
                            for r in rows {
                                if let Some(Value::Blob(b)) = r.first()
                                    && let Ok(oid) = <&Id>::try_from(&b[..])
                                    && let Some(tids) = m.get(oid)
                                {
                                    for tid in tids {
                                        if !ids.contains(tid) {
                                            ids.push(*tid);
                                        }
                                    }
                                }
                            }
                        }
                        ("id", ids)
                    }
                    ExpandKind::Forward { slot, multi: false } => {
                        let mut ids = Vec::new();
                        for r in rows {
                            if let Some(Value::Blob(b)) = r.get(*slot)
                                && let Ok(id) = <&Id>::try_from(&b[..])
                                && !ids.contains(id)
                            {
                                ids.push(*id);
                            }
                        }
                        ("id", ids)
                    }
                    ExpandKind::BackSingle { rel_field, .. } => {
                        let mut pids = Vec::new();
                        for r in rows {
                            if let Some(Value::Blob(b)) = r.first()
                                && let Ok(id) = <&Id>::try_from(&b[..])
                                && !pids.contains(id)
                            {
                                pids.push(*id);
                            }
                        }
                        back_map = Some(HashMap::new());
                        (rel_field.as_str(), pids)
                    }
                    ExpandKind::BackMulti { junction } => {
                        let mut pids: Vec<Value> = Vec::new();
                        for r in rows {
                            if let Some(v @ Value::Blob(_)) = r.first() {
                                pids.push(v.clone());
                            }
                        }
                        if pids.is_empty() {
                            out.push((HashMap::new(), Some(HashMap::new())));
                            continue;
                        }
                        let ph = vec!["?"; pids.len()].join(", ");
                        let jsql = format!(
                            "SELECT \"target\", \"owner\" FROM \"{junction}\" WHERE \"target\" IN ({ph}) ORDER BY \"target\", \"owner\""
                        );
                        let jrows = self
                            .store
                            .query(Query {
                                sql: jsql.into(),
                                params: pids,
                            })
                            .await?;
                        let mut bm: HashMap<Id, Vec<Id>> = HashMap::new();
                        let mut oids = Vec::new();
                        for jr in jrows {
                            if let [Value::Blob(tb), Value::Blob(ob)] = &jr[..]
                                && let (Ok(tid), Ok(oid)) =
                                    (<&Id>::try_from(&tb[..]), <&Id>::try_from(&ob[..]))
                            {
                                bm.entry(*tid).or_default().push(*oid);
                                if !oids.contains(oid) {
                                    oids.push(*oid);
                                }
                            }
                        }
                        back_map = Some(bm);
                        ("id", oids)
                    }
                };
                if ids.is_empty() {
                    out.push((HashMap::new(), back_map));
                    continue;
                }
                let placeholders = vec!["?"; ids.len()].join(", ");
                let mut params: Vec<Value> =
                    ids.iter().map(|id| Value::Blob((*id).into())).collect();
                let rule_sql = if p.superuser {
                    "1"
                } else {
                    let r = node.target.rules.view.as_ref().expect("checked Some above");
                    params.extend(r.bind(&p.ctx(None, now)));
                    r.sql.as_str()
                };
                let sql = format!(
                    "SELECT {} FROM \"{}\" AS t WHERE t.\"{where_col}\" IN ({placeholders}) AND ({rule_sql}) ORDER BY t.\"id\"",
                    node.target.sql.cols, node.target.def.name
                );
                let target_rows = self
                    .store
                    .query(Query {
                        sql: sql.into(),
                        params,
                    })
                    .await?;
                if let ExpandKind::BackSingle { rel_slot, .. } = &node.kind
                    && let Some(bm) = back_map.as_mut()
                {
                    for tr in &target_rows {
                        if let (Some(Value::Blob(tb)), Some(Value::Blob(pb))) =
                            (tr.first(), tr.get(*rel_slot))
                            && let (Ok(tid), Ok(pid)) =
                                (<&Id>::try_from(&tb[..]), <&Id>::try_from(&pb[..]))
                        {
                            bm.entry(*pid).or_default().push(*tid);
                        }
                    }
                }
                let target_multi = self.load_multi_rels(&node.target, &target_rows).await?;
                let child_maps = if node.children.is_empty() || target_rows.is_empty() {
                    Vec::new()
                } else {
                    self.resolve_expand(p, &target_rows, &target_multi, &node.children, now)
                        .await?
                };
                let mut encoded_map = HashMap::with_capacity(target_rows.len());
                for tr in &target_rows {
                    let Some(Value::Blob(b)) = tr.first() else {
                        continue;
                    };
                    let Ok(tid) = <&Id>::try_from(&b[..]) else {
                        continue;
                    };
                    let tr_multi = row_multi(tr, &target_multi);
                    let child_owned =
                        build_row_expand(tr, &target_multi, &node.children, &child_maps);
                    let child_exp: Vec<(&str, &[u8])> = child_owned
                        .iter()
                        .map(|(k, v)| (*k, v.as_slice()))
                        .collect();
                    let mut buf = Vec::with_capacity(256);
                    codec::write_record_ext(
                        &mut buf,
                        &node.target,
                        tr,
                        p.superuser,
                        None,
                        None,
                        &tr_multi,
                        &child_exp,
                    );
                    encoded_map.insert(*tid, buf);
                }
                out.push((encoded_map, back_map));
            }
            Ok(out)
        })
    }

    async fn encode_single(
        &self,
        p: &Principal,
        schema: &Schema,
        c: &Collection,
        row: &[Value],
        only: Option<&[usize]>,
        expand: Option<&str>,
    ) -> CoreResult<RecordOut> {
        let nodes = parse_expand(schema, c, expand, p.superuser)?;
        let rows_slice = [row.to_vec()];
        let multi_maps = self.load_multi_rels(c, &rows_slice).await?;
        let exp_maps = if nodes.is_empty() {
            Vec::new()
        } else {
            self.resolve_expand(p, &rows_slice, &multi_maps, &nodes, now_us())
                .await?
        };
        let rm = row_multi(row, &multi_maps);
        let row_owned = build_row_expand(row, &multi_maps, &nodes, &exp_maps);
        let row_exp: Vec<(&str, &[u8])> =
            row_owned.iter().map(|(k, v)| (*k, v.as_slice())).collect();
        let mut json = Vec::with_capacity(256);
        codec::write_record_ext(&mut json, c, row, p.superuser, only, None, &rm, &row_exp);
        Ok(RecordOut {
            json,
            version: row_version(row),
            created: false,
            not_modified: false,
        })
    }

    pub async fn create_record_with(
        &self,
        p: &Principal,
        coll: &str,
        json: &[u8],
        opts: &WriteOpts,
    ) -> CoreResult<RecordOut> {
        let schema = self.schema();
        let c = schema.get(coll)?;
        let m = self.create_mutation(p, c, json, now_us())?;
        let r = self.write_one(p, m, opts.dry_run).await?;
        let mut out = self
            .encode_single(p, &schema, c, &r.returned[0], None, opts.expand.as_deref())
            .await?;
        out.created = true;
        Ok(out)
    }

    pub async fn create_record(
        &self,
        p: &Principal,
        coll: &str,
        json: &[u8],
    ) -> CoreResult<Vec<u8>> {
        Ok(self
            .create_record_with(p, coll, json, &WriteOpts::default())
            .await?
            .json)
    }

    pub async fn view_record_with(
        &self,
        p: &Principal,
        coll: &str,
        id: &str,
        opts: &ReadOpts,
    ) -> CoreResult<RecordOut> {
        let schema = self.schema();
        let c = schema.get(coll)?;
        let only = parse_fields(c, opts.fields.as_deref(), p.superuser)?;
        let _ = parse_expand(&schema, c, opts.expand.as_deref(), p.superuser)?;
        let rule = rule_ref(p, &c.rules.view, "the view rule allows superusers only")?;
        let mut params = vec![Value::Blob(record_id(id)?.into())];
        let sql = match rule {
            Some(r) => {
                params.extend(r.bind(&p.ctx(None, now_us())));
                c.sql.view.clone()
            }
            None => c.sql.view_su.clone(),
        };
        let rows = self.store.query(Query { sql, params }).await?;
        let row = rows.first().ok_or(CoreError::NotFound("record"))?;
        let version = row_version(row);
        if opts.if_none_match == Some(version) {
            return Ok(RecordOut {
                json: Vec::new(),
                version,
                created: false,
                not_modified: true,
            });
        }
        self.encode_single(p, &schema, c, row, only.as_deref(), opts.expand.as_deref())
            .await
    }

    pub async fn view_record(&self, p: &Principal, coll: &str, id: &str) -> CoreResult<Vec<u8>> {
        Ok(self
            .view_record_with(p, coll, id, &ReadOpts::default())
            .await?
            .json)
    }

    pub async fn update_record_with(
        &self,
        p: &Principal,
        coll: &str,
        id: &str,
        json: &[u8],
        opts: &WriteOpts,
    ) -> CoreResult<RecordOut> {
        let schema = self.schema();
        let c = schema.get(coll)?;
        let _ = parse_expand(&schema, c, opts.expand.as_deref(), p.superuser)?;
        let rid = record_id(id)?;
        let (m, rule_binds) =
            self.update_mutation_with(p, c, rid, json, opts.if_match, now_us())?;
        let r = match self.write_one(p, m, opts.dry_run).await {
            Ok(r) => r,
            Err(CoreError::NotFound(_)) if opts.if_match.is_some() => {
                return Err(self
                    .check_if_match_miss(p, c, rid, &c.rules.update, rule_binds)
                    .await);
            }
            Err(e) => return Err(e),
        };
        self.encode_single(p, &schema, c, &r.returned[0], None, opts.expand.as_deref())
            .await
    }

    pub async fn update_record(
        &self,
        p: &Principal,
        coll: &str,
        id: &str,
        json: &[u8],
    ) -> CoreResult<Vec<u8>> {
        Ok(self
            .update_record_with(p, coll, id, json, &WriteOpts::default())
            .await?
            .json)
    }

    pub async fn upsert_record_with(
        &self,
        p: &Principal,
        coll: &str,
        id: &str,
        json: &[u8],
        opts: &WriteOpts,
    ) -> CoreResult<RecordOut> {
        let schema = self.schema();
        let c = schema.get(coll)?;
        let _ = parse_expand(&schema, c, opts.expand.as_deref(), p.superuser)?;
        let rid = record_id(id)?;
        let exists = !self
            .store
            .query(Query {
                sql: format!("SELECT 1 FROM \"{}\" WHERE \"id\" = ?", c.def.name).into(),
                params: vec![Value::Blob(rid.into())],
            })
            .await?
            .is_empty();
        if exists {
            return self.update_record_with(p, coll, id, json, opts).await;
        }
        if opts.if_match.is_some() {
            return Err(CoreError::PreconditionFailed(
                "cannot use If-Match when creating a new record",
            ));
        }
        let m = self.create_mutation_with_id(p, c, json, Some(rid), now_us())?;
        let r = self.write_one(p, m, opts.dry_run).await?;
        let mut out = self
            .encode_single(p, &schema, c, &r.returned[0], None, opts.expand.as_deref())
            .await?;
        out.created = true;
        Ok(out)
    }

    pub async fn delete_record_with(
        &self,
        p: &Principal,
        coll: &str,
        id: &str,
        opts: &WriteOpts,
    ) -> CoreResult<()> {
        let schema = self.schema();
        let c = schema.get(coll)?;
        let rid = record_id(id)?;
        let (m, rule_binds) = self.delete_mutation_with(p, c, rid, opts.if_match, now_us())?;
        match self.write_one(p, m, opts.dry_run).await {
            Ok(_) => Ok(()),
            Err(CoreError::NotFound(_)) if opts.if_match.is_some() => Err(self
                .check_if_match_miss(p, c, rid, &c.rules.delete, rule_binds)
                .await),
            Err(e) => Err(e),
        }
    }

    pub async fn delete_record(&self, p: &Principal, coll: &str, id: &str) -> CoreResult<()> {
        self.delete_record_with(p, coll, id, &WriteOpts::default())
            .await
    }

    pub async fn count_records(
        &self,
        p: &Principal,
        coll: &str,
        filter: &str,
        mode: TotalMode,
    ) -> CoreResult<(u64, bool)> {
        let schema = self.schema();
        let c = schema.get(coll)?;
        let su = p.superuser;
        let rule_sql = if su {
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
        let compiled_filter = if filter.trim().is_empty() {
            None
        } else {
            Some(compile(
                &*schema,
                &c.def.name,
                filter,
                if su { Mode::Rule } else { Mode::Filter },
            )?)
        };
        let unfiltered = compiled_filter.is_none()
            && (su
                || c.rules
                    .list
                    .as_ref()
                    .is_some_and(|r| r.class == Class::Const && r.sql == "1"));
        if mode == TotalMode::Estimate
            && unfiltered
            && let Ok(rows) = self
                .store
                .query(Query {
                    sql: "SELECT CAST(substr(stat, 1, instr(stat || ' ', ' ') - 1) AS INTEGER) FROM sqlite_stat1 WHERE tbl = ? LIMIT 1".into(),
                    params: vec![Value::Text(c.def.name.clone().into())],
                })
                .await
            && let Some(Value::Int(est)) = rows.first().and_then(|r| r.first())
        {
            return Ok((*est as u64, false));
        }
        let ctx = p.ctx(None, now_us());
        let mut params = Vec::new();
        if !su && let Some(r) = &c.rules.list {
            params.extend(r.bind(&ctx));
        }
        if let Some(f) = &compiled_filter {
            params.extend(f.bind(&ctx));
        }
        let sql = format!(
            "SELECT count(*) FROM \"{}\" AS t WHERE ({rule_sql}) AND ({})",
            c.def.name,
            compiled_filter.as_ref().map_or("1", |f| f.sql.as_str())
        );
        let rows = self
            .store
            .query(Query {
                sql: sql.into(),
                params,
            })
            .await?;
        let n = match rows.first().and_then(|r| r.first()) {
            Some(Value::Int(i)) => *i as u64,
            _ => 0,
        };
        Ok((n, true))
    }

    fn list_plan(
        &self,
        schema: &Schema,
        c: &Collection,
        lp: &ListParams,
        fields: Option<&[usize]>,
        nulls: Option<u32>,
        su: bool,
    ) -> CoreResult<Arc<ListPlan>> {
        let key = (
            schema.version,
            c.def.name.clone(),
            su,
            lp.filter.clone(),
            lp.sort.clone(),
            fields.map(<[usize]>::to_vec),
            nulls,
        );
        if let Some(plan) = self
            .plans
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
        {
            return Ok(plan.clone());
        }
        let plan = Arc::new(list::plan(
            schema, c, &lp.filter, &lp.sort, fields, nulls, su,
        )?);
        let mut cache = self.plans.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= PLAN_CACHE {
            cache.clear();
        }
        cache.insert(key, plan.clone());
        Ok(plan)
    }

    pub async fn list_records(
        &self,
        p: &Principal,
        coll: &str,
        lp: &ListParams,
    ) -> CoreResult<Vec<u8>> {
        let schema = self.schema();
        let c = schema.get(coll)?;
        let su = p.superuser;
        let limit = lp.limit.unwrap_or(DEFAULT_LIMIT);
        if !(1..=MAX_LIMIT).contains(&limit) {
            return Err(CoreError::BadRequest(format!(
                "limit must be within 1..={MAX_LIMIT}"
            )));
        }
        let only = parse_fields(c, lp.fields.as_deref(), su)?;
        let expand_nodes = parse_expand(&schema, c, lp.expand.as_deref(), su)?;
        // Expand reads relation slots by position, so it keeps the full column list.
        let proj = if expand_nodes.is_empty() {
            only.as_deref()
        } else {
            None
        };
        let base = self.list_plan(&schema, c, lp, proj, None, su)?;
        let hash = list::shape_hash(&[coll, &lp.filter, &lp.sort, if su { "su" } else { "" }]);
        let (plan, cursor) = match &lp.cursor {
            None => (base, vec![]),
            Some(cur) => {
                let vals = list::decode_cursor(cur, hash, base.sort.len())?;
                let nulls = vals
                    .iter()
                    .enumerate()
                    .fold(0u32, |m, (i, v)| m | u32::from(*v == Value::Null) << i);
                (self.list_plan(&schema, c, lp, proj, Some(nulls), su)?, vals)
            }
        };
        let now = now_us();
        let ctx = p.ctx(None, now);
        let mut params = Vec::new();
        if !su && let Some(r) = &c.rules.list {
            params.extend(r.bind(&ctx));
        }
        if let Some(f) = &plan.filter {
            params.extend(f.bind(&ctx));
        }
        params.extend(plan.keyset_binds.iter().map(|&i| cursor[i].clone()));
        params.push(Value::Int(limit as i64 + 1));
        let mut rows = self
            .store
            .query(Query {
                sql: plan.sql.clone(),
                params,
            })
            .await?;
        let more = rows.len() > limit;
        rows.truncate(limit);

        let multi_maps = self.load_multi_rels(c, &rows).await?;
        let exp_maps = if expand_nodes.is_empty() || rows.is_empty() {
            Vec::new()
        } else {
            self.resolve_expand(p, &rows, &multi_maps, &expand_nodes, now)
                .await?
        };
        let total = match lp.total {
            TotalMode::None => None,
            mode => Some(self.count_records(p, coll, &lp.filter, mode).await?.0),
        };

        let mut out = Vec::with_capacity(64 + rows.len() * 256);
        out.extend_from_slice(b"{\"items\":[");
        for (i, row) in rows.iter().enumerate() {
            if i > 0 {
                out.push(b',');
            }
            let rm = row_multi(row, &multi_maps);
            let row_owned = build_row_expand(row, &multi_maps, &expand_nodes, &exp_maps);
            let row_exp: Vec<(&str, &[u8])> =
                row_owned.iter().map(|(k, v)| (*k, v.as_slice())).collect();
            codec::write_record_ext(
                &mut out,
                c,
                row,
                su,
                only.as_deref(),
                plan.projected_slots.as_deref(),
                &rm,
                &row_exp,
            );
        }
        out.extend_from_slice(b"],\"next\":");
        match rows.last().filter(|_| more) {
            Some(last) => {
                let vals: Vec<&Value> = plan
                    .sort
                    .iter()
                    .map(|k| &last[plan.row_index(k.slot)])
                    .collect();
                codec::write_str(&mut out, &list::encode_cursor(hash, &vals));
            }
            None => out.extend_from_slice(b"null"),
        }
        if let Some(t) = total {
            use std::io::Write;
            let _ = write!(out, ",\"total\":{t}");
        }
        out.push(b'}');
        Ok(out)
    }

    /// All items commit atomically or none do.
    pub async fn batch_with(
        &self,
        p: &Principal,
        items: &[BatchItem<'_>],
        dry_run: bool,
    ) -> CoreResult<Vec<Id>> {
        if items.is_empty() || items.len() > MAX_BATCH {
            return Err(CoreError::BadRequest(format!(
                "a batch holds 1..={MAX_BATCH} operations"
            )));
        }
        let schema = self.schema();
        let now = now_us();
        let mut mutations = Vec::with_capacity(items.len());
        for (i, it) in items.iter().enumerate() {
            let at = |e: CoreError| match e {
                CoreError::BadRequest(m) => CoreError::BadRequest(format!("ops[{i}]: {m}")),
                CoreError::Validation(mut errs) => {
                    errs.iter_mut()
                        .for_each(|f| f.field = format!("ops[{i}].{}", f.field));
                    CoreError::Validation(errs)
                }
                e => e,
            };
            let c = schema.get(it.collection)?;
            let id = || {
                it.id
                    .ok_or_else(|| CoreError::BadRequest(format!("ops[{i}]: id required")))
                    .and_then(record_id)
            };
            let body = || {
                it.body
                    .ok_or_else(|| CoreError::BadRequest(format!("ops[{i}]: body required")))
            };
            mutations.push(
                match it.kind {
                    BatchKind::Create => self.create_mutation(p, c, body()?, now),
                    BatchKind::Update => self.update_mutation(p, c, id()?, body()?, now),
                    BatchKind::Delete => self.delete_mutation(p, c, id()?, now),
                }
                .map_err(at)?,
            );
        }
        let ids = mutations.iter().map(|m| m.rid).collect();
        let op = WriteOp {
            actor: p.actor(),
            mutations,
        };
        let res = if dry_run {
            self.store.write_dry_run(op).await
        } else {
            self.store.write(op).await
        };
        res.map_err(|e| match e {
            StoreError::NotFound => {
                CoreError::NotFound("batch target (missing, or rejected by a rule)")
            }
            e => e.into(),
        })?;
        Ok(ids)
    }

    pub async fn batch(&self, p: &Principal, items: &[BatchItem<'_>]) -> CoreResult<Vec<Id>> {
        self.batch_with(p, items, false).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_id_checks() {
        let now = now_us();
        assert!(check_client_id(&new_id(), now).is_ok());
        assert!(check_client_id(&[0; 16], now).is_err());
        let old = new_id();
        assert!(check_client_id(&old, now + 31 * 86_400_000_000).is_err());
    }
}
