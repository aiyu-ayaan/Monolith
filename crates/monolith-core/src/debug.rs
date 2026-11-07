//! Debug and introspection helpers: `collections:diff`, `rules:explain`,
//! `query:explain`, and the per-app request trace ring buffer (doc 22 §2).

use std::collections::HashMap;

use monolith_rules::{Auth, Class, Ctx, Mode, compile};
use monolith_store::{AppStore, Query, Value};
use serde::Serialize;

use crate::app::{App, now_us};
use crate::codec;
use crate::ddl;
use crate::error::{CoreError, CoreResult};
use crate::list;
use crate::schema::{CollectionDef, Schema};

pub const MAX_LOGS: usize = 2048;

#[derive(Debug, Default)]
pub struct ExplainRuleReq {
    pub collection: String,
    pub op: String,
    pub expr: Option<String>,
    pub auth: Option<Auth>,
    pub record: Option<Vec<u8>>,
    pub body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExplainRuleResp {
    pub collection: String,
    pub op: String,
    pub expr: Option<String>,
    pub class: &'static str,
    pub sql: String,
    pub verdict: Option<bool>,
}

#[derive(Debug, Default)]
pub struct ExplainQueryReq {
    pub collection: String,
    pub filter: String,
    pub sort: String,
    pub superuser: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanStep {
    pub id: i64,
    pub parent: i64,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplainQueryResp {
    pub sql: String,
    pub plan: Vec<PlanStep>,
    pub uses_scan: bool,
    pub suggested_indexes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestLog {
    pub request_id: String,
    pub timestamp: String,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub duration_us: u64,
    pub superuser: bool,
    pub error: Option<String>,
}

fn class_name(c: Class) -> &'static str {
    match c {
        Class::Const => "const",
        Class::AuthOnly => "authOnly",
        Class::RowLocal => "rowLocal",
        Class::Relational => "relational",
    }
}

impl App {
    /// Validates `def` against the live schema and returns the planned DDL without applying it.
    pub fn diff_collection(&self, def: CollectionDef) -> CoreResult<Vec<String>> {
        let cur = self.schema();
        let old = cur.collections.get(&def.name).cloned();
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
        let _ = Schema::build(cur.version + 1, defs)?;
        ddl::plan(old.as_ref().map(|c| &c.def), &def)
    }

    /// Compiles a collection rule or custom expression and evaluates it if sample data is given.
    pub fn explain_rule(&self, req: ExplainRuleReq) -> CoreResult<ExplainRuleResp> {
        let schema = self.schema();
        let c = schema.get(&req.collection)?;
        let op = if req.op.is_empty() {
            "list"
        } else {
            req.op.as_str()
        };
        let (expr, mode) = match (&req.expr, op) {
            (Some(e), "filter") => (Some(e.clone()), Mode::Filter),
            (Some(e), "list" | "view" | "create" | "update" | "delete") => {
                (Some(e.clone()), Mode::Rule)
            }
            (None, "list") => (c.def.rules.list.clone(), Mode::Rule),
            (None, "view") => (c.def.rules.view.clone(), Mode::Rule),
            (None, "create") => (c.def.rules.create.clone(), Mode::Rule),
            (None, "update") => (c.def.rules.update.clone(), Mode::Rule),
            (None, "delete") => (c.def.rules.delete.clone(), Mode::Rule),
            (None, "filter") => (Some(String::new()), Mode::Filter),
            _ => {
                return Err(CoreError::BadRequest(format!(
                    "op must be list, view, create, update, delete or filter (got '{op}')"
                )));
            }
        };
        let Some(src) = expr else {
            return Ok(ExplainRuleResp {
                collection: c.def.name.clone(),
                op: op.to_owned(),
                expr: None,
                class: "superusersOnly",
                sql: "0".into(),
                verdict: Some(false),
            });
        };
        let compiled = compile(&*schema, &c.def.name, &src, mode)?;
        let row = match &req.record {
            Some(b) => Some(codec::decode_sample(c, b)?),
            None => None,
        };
        let body = match &req.body {
            Some(b) => Some(codec::decode_sample(c, b)?),
            None => row.clone(),
        };
        let verdict = if row.is_some() || compiled.class <= Class::AuthOnly {
            let empty_row = vec![Value::Null; c.slots()];
            let r = row
                .as_ref()
                .map_or(empty_row.as_slice(), |(v, _)| v.as_slice());
            let ctx = Ctx {
                auth: req.auth.as_ref(),
                body: body.as_ref().map(|(v, _)| v.as_slice()),
                body_isset: body.as_ref().map(|(_, m)| *m),
                now_us: now_us(),
            };
            let bound = compiled.bind(&ctx);
            compiled.eval(r, &bound)
        } else {
            None
        };
        Ok(ExplainRuleResp {
            collection: c.def.name.clone(),
            op: op.to_owned(),
            expr: Some(src),
            class: class_name(compiled.class),
            sql: compiled.sql,
            verdict,
        })
    }

    /// Runs `EXPLAIN QUERY PLAN` on a compiled list query and suggests indexes when needed.
    pub async fn explain_query(&self, req: ExplainQueryReq) -> CoreResult<ExplainQueryResp> {
        let schema = self.schema();
        let c = schema.get(&req.collection)?;
        let plan = list::plan(
            &schema,
            c,
            &req.filter,
            &req.sort,
            None,
            None,
            req.superuser,
        )?;
        let mut n_binds = 1; // LIMIT ?
        if !req.superuser
            && let Some(r) = &c.rules.list
        {
            n_binds += r.bind(&Ctx::default()).len();
        }
        if let Some(f) = &plan.filter {
            n_binds += f.bind(&Ctx::default()).len();
        }
        let rows = self
            .store()
            .query(Query {
                sql: format!("EXPLAIN QUERY PLAN {}", plan.sql).into(),
                params: vec![Value::Null; n_binds],
            })
            .await?;
        let mut steps = Vec::with_capacity(rows.len());
        let mut uses_scan = false;
        let mut temp_btree = false;
        for r in rows {
            let id = match r.first() {
                Some(Value::Int(i)) => *i,
                _ => 0,
            };
            let parent = match r.get(1) {
                Some(Value::Int(i)) => *i,
                _ => 0,
            };
            let detail = match r.get(3) {
                Some(Value::Text(s)) => s.to_string(),
                _ => String::new(),
            };
            if detail.starts_with("SCAN ") && !detail.contains("USING COVERING INDEX") {
                uses_scan = true;
            }
            if detail.contains("USE TEMP B-TREE FOR ORDER BY") {
                temp_btree = true;
            }
            steps.push(PlanStep { id, parent, detail });
        }
        let mut suggested_indexes = Vec::new();
        if uses_scan || temp_btree {
            let sort_cols: Vec<String> = plan
                .sort
                .iter()
                .filter(|k| k.slot != 0)
                .map(|k| format!("\"{}\"", k.column))
                .collect();
            if !sort_cols.is_empty() {
                suggested_indexes.push(format!(
                    "CREATE INDEX \"idx_{}_sort\" ON \"{}\" ({})",
                    c.def.name,
                    c.def.name,
                    sort_cols.join(", ")
                ));
            }
        }
        Ok(ExplainQueryResp {
            sql: plan.sql.to_string(),
            plan: steps,
            uses_scan,
            suggested_indexes,
        })
    }

    pub fn record_log(&self, entry: RequestLog) {
        let mut logs = self.logs.lock().unwrap_or_else(|e| e.into_inner());
        if logs.len() >= MAX_LOGS {
            logs.pop_front();
        }
        logs.push_back(entry);
    }

    pub fn list_logs(&self, request_id: Option<&str>, limit: usize) -> Vec<RequestLog> {
        let logs = self.logs.lock().unwrap_or_else(|e| e.into_inner());
        logs.iter()
            .rev()
            .filter(|l| request_id.is_none_or(|id| l.request_id == id))
            .take(limit.clamp(1, 500))
            .cloned()
            .collect()
    }

    pub fn slow_logs(&self, limit: usize) -> Vec<RequestLog> {
        let logs = self.logs.lock().unwrap_or_else(|e| e.into_inner());
        let mut v: Vec<RequestLog> = logs.iter().cloned().collect();
        v.sort_by_key(|a| std::cmp::Reverse(a.duration_us));
        v.truncate(limit.clamp(1, 500));
        v
    }
}
