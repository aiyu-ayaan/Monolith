//! One app's runtime: its store, the current compiled schema, record operations.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use monolith_rules::{Auth, Compiled, Ctx};
use monolith_store::id::{decode as decode_id, new_id};
use monolith_store::{
    AppStore, Error as StoreError, Id, MigrationPlan, Mutation, Op, Query, SqliteConfig,
    SqliteStore, Value, WriteOp,
};

use crate::codec::{self, Body};
use crate::ddl::{self, sql_str};
use crate::error::{CoreError, CoreResult};
use crate::list::{self, ListPlan};
use crate::schema::{Collection, CollectionDef, Schema};

pub const MAX_LIMIT: usize = 1000;
pub const DEFAULT_LIMIT: usize = 50;
pub const MAX_BATCH: usize = 500;
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

    fn ctx<'a>(&'a self, body: Option<&'a [Value]>, now: i64) -> Ctx<'a> {
        Ctx {
            auth: self.auth.as_ref(),
            body,
            now_us: now,
        }
    }

    fn actor(&self) -> Option<Id> {
        self.auth.as_ref().map(|a| a.id)
    }
}

#[derive(Debug, Default, Clone)]
pub struct ListParams {
    pub filter: String,
    pub sort: String,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub fields: Option<String>,
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

type PlanKey = (u64, String, bool, String, String, Option<u32>);

pub struct App {
    pub name: String,
    store: SqliteStore,
    schema: RwLock<Arc<Schema>>,
    /// Serializes schema changes; record traffic never takes it.
    schema_lock: tokio::sync::Mutex<()>,
    // shortcut: cleared when full instead of LRU; fine until profiles say otherwise.
    plans: Mutex<HashMap<PlanKey, Arc<ListPlan>>>,
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
    /// Opens (or creates) the app database at `path` and loads its schema.
    pub async fn open(name: &str, path: &Path) -> CoreResult<App> {
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
        Ok(App {
            name: name.to_owned(),
            store,
            schema: RwLock::new(Arc::new(schema)),
            schema_lock: tokio::sync::Mutex::new(()),
            plans: Mutex::new(HashMap::new()),
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
        cur.get(name)?;
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
        let statements = vec![
            format!("DROP TABLE \"{name}\""),
            format!("DELETE FROM _collections WHERE name = {}", sql_str(name)),
            "UPDATE _meta SET value = value + 1 WHERE key = 'schema_version'".into(),
        ];
        self.store
            .apply_schema(MigrationPlan { statements })
            .await?;
        *self.schema.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(next);
        Ok(())
    }

    // ---------- records ----------

    fn create_mutation(
        &self,
        p: &Principal,
        c: &Collection,
        json: &[u8],
        now: i64,
    ) -> CoreResult<Mutation> {
        let rule = rule_ref(p, &c.rules.create, "the create rule allows superusers only")?;
        let body = codec::decode_body(c, json, true)?;
        let id = match body.id {
            Some(id) => {
                check_client_id(&id, now)?;
                id
            }
            None => new_id(),
        };
        let mut params = vec![
            Value::Blob(id.into()),
            Value::Int(now),
            Value::Int(now),
            Value::Null,
            Value::Null,
        ];
        params.extend(body.values.iter().map(|v| v.clone().unwrap_or(Value::Null)));
        let sql = match rule {
            Some(r) => {
                let slots = body.rule_slots();
                params.extend(r.bind(&p.ctx(Some(&slots), now)));
                c.sql.insert.clone()
            }
            None => c.sql.insert_su.clone(),
        };
        Ok(Mutation {
            op: Op::Insert,
            coll: c.cid,
            rid: id,
            fields: None,
            sql,
            params,
            seq_slot: Some(3),
            hlc_slot: Some(4),
        })
    }

    fn update_mutation(
        &self,
        p: &Principal,
        c: &Collection,
        id: Id,
        json: &[u8],
        now: i64,
    ) -> CoreResult<Mutation> {
        let rule = rule_ref(p, &c.rules.update, "the update rule allows superusers only")?;
        let body: Body = codec::decode_body(c, json, false)?;
        let mut params = vec![Value::Int(now), Value::Null, Value::Null];
        let mut fields = 0u64;
        for (i, v) in body.values.iter().enumerate() {
            fields |= u64::from(v.is_some()) << i;
            params.push(Value::Int(v.is_some() as i64));
            params.push(v.clone().unwrap_or(Value::Null));
        }
        params.push(Value::Blob(id.into()));
        let sql = match rule {
            Some(r) => {
                let slots = body.rule_slots();
                params.extend(r.bind(&p.ctx(Some(&slots), now)));
                c.sql.update.clone()
            }
            None => c.sql.update_su.clone(),
        };
        Ok(Mutation {
            op: Op::Update,
            coll: c.cid,
            rid: id,
            fields: Some(fields),
            sql,
            params,
            seq_slot: Some(1),
            hlc_slot: Some(2),
        })
    }

    fn delete_mutation(
        &self,
        p: &Principal,
        c: &Collection,
        id: Id,
        now: i64,
    ) -> CoreResult<Mutation> {
        let rule = rule_ref(p, &c.rules.delete, "the delete rule allows superusers only")?;
        let mut params = vec![Value::Blob(id.into())];
        let sql = match rule {
            Some(r) => {
                params.extend(r.bind(&p.ctx(None, now)));
                c.sql.delete.clone()
            }
            None => c.sql.delete_su.clone(),
        };
        Ok(Mutation {
            op: Op::Delete,
            coll: c.cid,
            rid: id,
            fields: None,
            sql,
            params,
            seq_slot: None,
            hlc_slot: None,
        })
    }

    async fn write_one(
        &self,
        p: &Principal,
        m: Mutation,
    ) -> CoreResult<monolith_store::WriteResult> {
        let insert = m.op == Op::Insert;
        self.store
            .write(WriteOp {
                actor: p.actor(),
                mutations: vec![m],
            })
            .await
            .map_err(|e| match e {
                // An insert only "misses" when its create rule said no.
                StoreError::NotFound if insert => {
                    CoreError::Forbidden("the create rule rejected this record")
                }
                e => e.into(),
            })
    }

    fn encode_one(c: &Collection, row: &[Value], su: bool) -> Vec<u8> {
        let mut out = Vec::with_capacity(256);
        codec::write_record(&mut out, c, row, su, None);
        out
    }

    pub async fn create_record(
        &self,
        p: &Principal,
        coll: &str,
        json: &[u8],
    ) -> CoreResult<Vec<u8>> {
        let schema = self.schema();
        let c = schema.get(coll)?;
        let m = self.create_mutation(p, c, json, now_us())?;
        let r = self.write_one(p, m).await?;
        Ok(Self::encode_one(c, &r.returned[0], p.superuser))
    }

    pub async fn view_record(&self, p: &Principal, coll: &str, id: &str) -> CoreResult<Vec<u8>> {
        let schema = self.schema();
        let c = schema.get(coll)?;
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
        Ok(Self::encode_one(c, row, p.superuser))
    }

    pub async fn update_record(
        &self,
        p: &Principal,
        coll: &str,
        id: &str,
        json: &[u8],
    ) -> CoreResult<Vec<u8>> {
        let schema = self.schema();
        let c = schema.get(coll)?;
        let m = self.update_mutation(p, c, record_id(id)?, json, now_us())?;
        let r = self.write_one(p, m).await?;
        Ok(Self::encode_one(c, &r.returned[0], p.superuser))
    }

    pub async fn delete_record(&self, p: &Principal, coll: &str, id: &str) -> CoreResult<()> {
        let schema = self.schema();
        let c = schema.get(coll)?;
        let m = self.delete_mutation(p, c, record_id(id)?, now_us())?;
        self.write_one(p, m).await.map(drop)
    }

    fn list_plan(
        &self,
        schema: &Schema,
        c: &Collection,
        lp: &ListParams,
        nulls: Option<u32>,
        su: bool,
    ) -> CoreResult<Arc<ListPlan>> {
        let key = (
            schema.version,
            c.def.name.clone(),
            su,
            lp.filter.clone(),
            lp.sort.clone(),
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
        let plan = Arc::new(list::plan(schema, c, &lp.filter, &lp.sort, nulls, su)?);
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
        let only = match &lp.fields {
            None => None,
            Some(f) => Some(
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
            ),
        };
        let base = self.list_plan(&schema, c, lp, None, su)?;
        let hash = list::shape_hash(&[coll, &lp.filter, &lp.sort, if su { "su" } else { "" }]);
        let (plan, cursor) = match &lp.cursor {
            None => (base, vec![]),
            Some(cur) => {
                let vals = list::decode_cursor(cur, hash, base.sort.len())?;
                let nulls = vals
                    .iter()
                    .enumerate()
                    .fold(0u32, |m, (i, v)| m | u32::from(*v == Value::Null) << i);
                (self.list_plan(&schema, c, lp, Some(nulls), su)?, vals)
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

        let mut out = Vec::with_capacity(64 + rows.len() * 256);
        out.extend_from_slice(b"{\"items\":[");
        for (i, row) in rows.iter().enumerate() {
            if i > 0 {
                out.push(b',');
            }
            codec::write_record(&mut out, c, row, su, only.as_deref());
        }
        out.extend_from_slice(b"],\"next\":");
        match rows.last().filter(|_| more) {
            Some(last) => {
                let vals: Vec<&Value> = plan.sort.iter().map(|k| &last[k.slot]).collect();
                codec::write_str(&mut out, &list::encode_cursor(hash, &vals));
            }
            None => out.extend_from_slice(b"null"),
        }
        out.push(b'}');
        Ok(out)
    }

    /// All items commit atomically or none do.
    pub async fn batch(&self, p: &Principal, items: &[BatchItem<'_>]) -> CoreResult<Vec<Id>> {
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
        self.store
            .write(WriteOp {
                actor: p.actor(),
                mutations,
            })
            .await
            .map_err(|e| match e {
                StoreError::NotFound => {
                    CoreError::NotFound("batch target (missing, or rejected by a rule)")
                }
                e => e.into(),
            })?;
        Ok(ids)
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
