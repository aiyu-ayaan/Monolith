//! WASM plugins (doc 14 §1–3): sandboxed after-commit event handlers, run with `wasmtime`.
//!
//! ABI v1 (core module, no component model yet). The module exports `memory`,
//! `alloc(len: i32) -> i32` and `on_event(ptr: i32, len: i32) -> i32` and may import only
//! `monolith.log(level: i32, ptr: i32, len: i32)`. For each matching change after commit the
//! host copies the JSON envelope (the same one webhooks send) into guest memory and calls
//! `on_event`; a non-zero result counts as an error. Each call gets a fresh instance, a fuel
//! budget and a 16 MiB memory cap, runs on the blocking pool, and cannot touch the network,
//! the file system or the database.
//!
//! v1 limits: events only (no before-write hooks, custom routes or cron), at-most-once (a
//! plugin error is counted and logged, not retried), counters and logs live in memory and reset
//! on restart, no `kv`/`http.fetch` host functions yet.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

use monolith_store::id::{decode, encode_string, new_id};
use monolith_store::{AppStore, Query, Value};
use serde::{Deserialize, Serialize};
use wasmtime::{Caller, Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

use crate::app::now_us;
use crate::codec::format_us;
use crate::error::{CoreError, CoreResult, FieldError};
use crate::system::{Registry, sys_delete, sys_write};
use crate::webhooks::{Pattern, id_of, int, parse_pattern, patterns_of, text};

pub const MAX_WASM: usize = 8 << 20;
const MAX_PLUGINS: usize = 20;
const MAX_NAME: usize = 100;
const MAX_EVENTS: usize = 32;
const DEFAULT_FUEL: u64 = 50_000_000;
const MAX_FUEL: u64 = 5_000_000_000;
const MEMORY_CAP: usize = 16 << 20;
const LOG_LINES: usize = 100;
const LOG_LINE_LEN: usize = 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginEdit {
    pub name: String,
    pub events: Vec<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Instruction budget per call (wasmtime fuel). Default 50 million.
    pub fuel: Option<u64>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub events: Vec<String>,
    pub enabled: bool,
    pub created: String,
    pub size: usize,
    pub fuel: u64,
    /// Calls and failures since the server started.
    pub calls: u64,
    pub errors: u64,
    pub last_error: Option<String>,
    pub last_run: Option<String>,
}

/// A compiled, enabled plugin.
pub(crate) struct PluginRt {
    pub(crate) id: monolith_store::Id,
    name: String,
    pub(crate) patterns: Vec<Pattern>,
    module: Module,
    fuel: u64,
    calls: AtomicU64,
    errors: AtomicU64,
    last_error: Mutex<Option<String>>,
    last_run: AtomicI64,
    logs: Mutex<VecDeque<String>>,
}

impl PluginRt {
    pub(crate) fn matches(&self, coll: &str, op: &str) -> bool {
        self.patterns.iter().any(|p| p.matches(coll, op))
    }
}

struct Ctx {
    limits: StoreLimits,
    logs: Vec<String>,
}

pub(crate) fn engine() -> CoreResult<Engine> {
    let mut cfg = Config::new();
    cfg.consume_fuel(true);
    Engine::new(&cfg).map_err(|e| CoreError::Internal(e.to_string()))
}

fn linker(engine: &Engine) -> CoreResult<Linker<Ctx>> {
    let mut l = Linker::new(engine);
    l.func_wrap(
        "monolith",
        "log",
        |mut c: Caller<'_, Ctx>, level: i32, ptr: i32, len: i32| {
            let Some(mem) = c.get_export("memory").and_then(|e| e.into_memory()) else {
                return;
            };
            let (ptr, len) = (ptr as u32 as usize, (len as u32 as usize).min(LOG_LINE_LEN));
            let mut buf = vec![0u8; len];
            if mem.read(&c, ptr, &mut buf).is_ok() && c.data().logs.len() < LOG_LINES {
                let tag = match level {
                    0 => "debug",
                    1 => "info",
                    2 => "warn",
                    _ => "error",
                };
                c.data_mut()
                    .logs
                    .push(format!("[{tag}] {}", String::from_utf8_lossy(&buf)));
            }
        },
    )
    .map_err(|e| CoreError::Internal(e.to_string()))?;
    Ok(l)
}

/// Runs `on_event` once on a fresh instance; the guest's log lines come back with the result.
fn run_once(
    engine: &Engine,
    module: &Module,
    fuel: u64,
    payload: &[u8],
) -> (Vec<String>, Result<(), String>) {
    let mut store = Store::new(
        engine,
        Ctx {
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY_CAP)
                .instances(1)
                .build(),
            logs: Vec::new(),
        },
    );
    store.limiter(|c| &mut c.limits);
    let res = (|| -> Result<(), String> {
        store.set_fuel(fuel).map_err(|e| e.to_string())?;
        let l = linker(engine).map_err(|e| e.to_string())?;
        let inst = l
            .instantiate(&mut store, module)
            .map_err(|e| format!("instantiate: {e}"))?;
        let mem = inst
            .get_memory(&mut store, "memory")
            .ok_or("no `memory` export")?;
        let alloc = inst
            .get_typed_func::<i32, i32>(&mut store, "alloc")
            .map_err(|e| format!("alloc: {e}"))?;
        let on_event = inst
            .get_typed_func::<(i32, i32), i32>(&mut store, "on_event")
            .map_err(|e| format!("on_event: {e}"))?;
        let len = i32::try_from(payload.len()).map_err(|_| "payload too large")?;
        let ptr = alloc.call(&mut store, len).map_err(trap)?;
        mem.write(&mut store, ptr as u32 as usize, payload)
            .map_err(|e| format!("write payload: {e}"))?;
        match on_event.call(&mut store, (ptr, len)).map_err(trap)? {
            0 => Ok(()),
            n => Err(format!("on_event returned {n}")),
        }
    })();
    let logs = std::mem::take(&mut store.data_mut().logs);
    (logs, res)
}

fn trap(e: wasmtime::Error) -> String {
    if e.downcast_ref::<wasmtime::Trap>() == Some(&wasmtime::Trap::OutOfFuel) {
        "out of fuel (instruction budget exceeded)".into()
    } else {
        e.to_string()
    }
}

impl PluginRt {
    /// Runs the plugin for one event and records the outcome.
    pub(crate) async fn invoke(
        self: &Arc<Self>,
        engine: &Engine,
        exec: &Arc<tokio::sync::Semaphore>,
        payload: Vec<u8>,
    ) {
        let Ok(_permit) = exec.clone().acquire_owned().await else {
            return;
        };
        let (rt, engine) = (self.clone(), engine.clone());
        let out =
            tokio::task::spawn_blocking(move || run_once(&engine, &rt.module, rt.fuel, &payload))
                .await;
        self.calls.fetch_add(1, Relaxed);
        self.last_run.store(now_us(), Relaxed);
        let (logs, res) = match out {
            Ok(r) => r,
            Err(e) => (Vec::new(), Err(format!("plugin task failed: {e}"))),
        };
        {
            let mut g = self.logs.lock().unwrap_or_else(|e| e.into_inner());
            for l in logs {
                if g.len() >= LOG_LINES {
                    g.pop_front();
                }
                g.push_back(l);
            }
        }
        if let Err(e) = res {
            self.errors.fetch_add(1, Relaxed);
            tracing::warn!(plugin = %self.name, error = %e, "plugin failed");
            *self.last_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(e);
        }
    }

    fn info(
        &self,
        wasm_len: usize,
        events: Vec<String>,
        enabled: bool,
        created: i64,
    ) -> PluginInfo {
        let last = self.last_run.load(Relaxed);
        PluginInfo {
            id: encode_string(&self.id),
            name: self.name.clone(),
            events,
            enabled,
            created: format_us(created),
            size: wasm_len,
            fuel: self.fuel,
            calls: self.calls.load(Relaxed),
            errors: self.errors.load(Relaxed),
            last_error: self
                .last_error
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
            last_run: (last > 0).then(|| format_us(last)),
        }
    }
}

fn invalid(field: &str, code: &'static str, msg: impl Into<String>) -> CoreError {
    CoreError::Validation(vec![FieldError::new(field, code, msg)])
}

fn validate(e: &PluginEdit) -> CoreResult<()> {
    let name = e.name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(invalid(
            "name",
            "out_of_range",
            format!("1 to {MAX_NAME} characters"),
        ));
    }
    if e.events.is_empty() || e.events.len() > MAX_EVENTS {
        return Err(invalid(
            "events",
            "out_of_range",
            format!("1 to {MAX_EVENTS} event patterns"),
        ));
    }
    if let Some(bad) = e.events.iter().find(|p| parse_pattern(p).is_none()) {
        return Err(invalid(
            "events",
            "invalid",
            format!(
                "'{bad}' is not a pattern; use posts.create|update|delete, posts.*, *.delete or *"
            ),
        ));
    }
    if e.fuel.is_some_and(|f| !(1_000..=MAX_FUEL).contains(&f)) {
        return Err(invalid(
            "fuel",
            "out_of_range",
            format!("1000 to {MAX_FUEL}"),
        ));
    }
    Ok(())
}

/// Compiles `wasm` and checks the ABI by instantiating it once with a tiny budget.
fn compile(engine: &Engine, wasm: &[u8]) -> CoreResult<Module> {
    let module = Module::new(engine, wasm).map_err(|e| {
        invalid(
            "wasm",
            "invalid",
            format!("not a valid WebAssembly module: {e}"),
        )
    })?;
    for imp in module.imports() {
        if (imp.module(), imp.name()) != ("monolith", "log") {
            return Err(invalid(
                "wasm",
                "unsupported_import",
                format!(
                    "imports `{}.{}`; only `monolith.log` exists",
                    imp.module(),
                    imp.name()
                ),
            ));
        }
    }
    for (name, want) in [
        ("memory", "memory"),
        ("alloc", "func"),
        ("on_event", "func"),
    ] {
        let ok = module.get_export(name).is_some_and(|t| match want {
            "memory" => t.memory().is_some(),
            _ => t.func().is_some(),
        });
        if !ok {
            return Err(invalid(
                "wasm",
                "missing_export",
                format!(
                    "the module must export `{name}` (ABI: memory, alloc(i32)->i32, on_event(i32,i32)->i32)"
                ),
            ));
        }
    }
    Ok(module)
}

pub(crate) fn build_rt(
    engine: &Engine,
    id: monolith_store::Id,
    name: &str,
    events: &str,
    fuel: u64,
    wasm: &[u8],
) -> CoreResult<Arc<PluginRt>> {
    Ok(Arc::new(PluginRt {
        id,
        name: name.to_owned(),
        patterns: patterns_of(events),
        module: compile(engine, wasm)?,
        fuel,
        calls: AtomicU64::new(0),
        errors: AtomicU64::new(0),
        last_error: Mutex::new(None),
        last_run: AtomicI64::new(0),
        logs: Mutex::new(VecDeque::new()),
    }))
}

const COLS: &str = "id, name, events, enabled, fuel, created, length(wasm)";

fn row_info(hub: &crate::webhooks::WhHub, r: &[Value]) -> Option<PluginInfo> {
    let id = id_of(&r[0])?;
    let events: Vec<String> = serde_json::from_str(text(&r[2])).unwrap_or_default();
    let (enabled, created, size) = (int(&r[3]) != 0, int(&r[5]), int(&r[6]) as usize);
    Some(match hub.plugin(&id) {
        Some(rt) => rt.info(size, events, enabled, created),
        None => PluginInfo {
            id: encode_string(&id),
            name: text(&r[1]).to_owned(),
            events,
            enabled,
            created: format_us(created),
            size,
            fuel: int(&r[4]) as u64,
            calls: 0,
            errors: 0,
            last_error: None,
            last_run: None,
        },
    })
}

impl Registry {
    /// Installs a plugin from raw `.wasm` bytes.
    pub async fn create_plugin(
        &self,
        app: &str,
        edit: PluginEdit,
        wasm: Vec<u8>,
    ) -> CoreResult<PluginInfo> {
        self.app(app).await?; // opens the app and starts its dispatcher
        validate(&edit)?;
        if wasm.is_empty() || wasm.len() > MAX_WASM {
            return Err(CoreError::TooLarge(format!(
                "the module must be 1 byte to {MAX_WASM} bytes"
            )));
        }
        let n = self
            .sys
            .query(Query {
                sql: "SELECT count(*) FROM plugins WHERE app = ?".into(),
                params: vec![Value::Text(app.into())],
            })
            .await?;
        if n.first().and_then(|r| r.first()).map_or(0, int) as usize >= MAX_PLUGINS {
            return Err(CoreError::Conflict(format!(
                "an app can have at most {MAX_PLUGINS} plugins"
            )));
        }
        let fuel = edit.fuel.unwrap_or(DEFAULT_FUEL);
        let events =
            serde_json::to_string(&edit.events).map_err(|e| CoreError::Internal(e.to_string()))?;
        let id = new_id();
        let engine = self.wh.engine().clone();
        let (name, ev, w) = (edit.name.trim().to_owned(), events.clone(), wasm.clone());
        // Compiling is CPU-bound (Cranelift): keep it off the async workers.
        tokio::task::spawn_blocking(move || build_rt(&engine, id, &name, &ev, fuel, &w))
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))??;
        self.sys
            .write(sys_write(
                "INSERT INTO plugins (id, app, name, wasm, events, enabled, fuel, created) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                id,
                vec![
                    Value::Blob(id.into()),
                    Value::Text(app.into()),
                    Value::Text(edit.name.trim().into()),
                    Value::Blob(wasm.into()),
                    Value::Text(events.into()),
                    Value::Int(i64::from(edit.enabled)),
                    Value::Int(fuel as i64),
                    Value::Int(now_us()),
                ],
            ))
            .await?;
        self.wh.reload_plugins(app).await?;
        self.plugin_info(app, &id).await
    }

    async fn plugin_info(&self, app: &str, id: &monolith_store::Id) -> CoreResult<PluginInfo> {
        self.sys
            .query(Query {
                sql: format!("SELECT {COLS} FROM plugins WHERE id = ? AND app = ?").into(),
                params: vec![Value::Blob((*id).into()), Value::Text(app.into())],
            })
            .await?
            .first()
            .and_then(|r| row_info(&self.wh, r))
            .ok_or(CoreError::NotFound("plugin"))
    }

    pub async fn list_plugins(&self, app: &str) -> CoreResult<Vec<PluginInfo>> {
        self.app_info(app).await?;
        let rows = self
            .sys
            .query(Query {
                sql: format!("SELECT {COLS} FROM plugins WHERE app = ? ORDER BY created").into(),
                params: vec![Value::Text(app.into())],
            })
            .await?;
        Ok(rows.iter().filter_map(|r| row_info(&self.wh, r)).collect())
    }

    /// Changes name, events, enabled flag and fuel; the module stays.
    pub async fn update_plugin(
        &self,
        app: &str,
        id: &str,
        edit: PluginEdit,
    ) -> CoreResult<PluginInfo> {
        let id = decode(id).ok_or(CoreError::NotFound("plugin"))?;
        validate(&edit)?;
        let events =
            serde_json::to_string(&edit.events).map_err(|e| CoreError::Internal(e.to_string()))?;
        self.sys
            .write(sys_write(
                "UPDATE plugins SET name = ?, events = ?, enabled = ?, fuel = coalesce(?, fuel) \
                 WHERE id = ? AND app = ?",
                id,
                vec![
                    Value::Text(edit.name.trim().into()),
                    Value::Text(events.into()),
                    Value::Int(i64::from(edit.enabled)),
                    edit.fuel.map_or(Value::Null, |f| Value::Int(f as i64)),
                    Value::Blob(id.into()),
                    Value::Text(app.into()),
                ],
            ))
            .await
            .map_err(|e| match e {
                monolith_store::Error::NotFound => CoreError::NotFound("plugin"),
                e => e.into(),
            })?;
        self.app(app).await?;
        self.wh.reload_plugins(app).await?;
        self.plugin_info(app, &id).await
    }

    pub async fn delete_plugin(&self, app: &str, id: &str) -> CoreResult<()> {
        let id = decode(id).ok_or(CoreError::NotFound("plugin"))?;
        self.sys
            .write(sys_delete(
                "DELETE FROM plugins WHERE id = ? AND app = ?",
                vec![Value::Blob(id.into()), Value::Text(app.into())],
            ))
            .await
            .map_err(|e| match e {
                monolith_store::Error::NotFound => CoreError::NotFound("plugin"),
                e => e.into(),
            })?;
        self.wh.reload_plugins(app).await
    }

    /// The guest's latest log lines (memory only, newest last).
    pub async fn plugin_logs(&self, app: &str, id: &str) -> CoreResult<Vec<String>> {
        let id = decode(id).ok_or(CoreError::NotFound("plugin"))?;
        self.plugin_info(app, &id).await?;
        Ok(self
            .wh
            .plugin(&id)
            .map(|rt| {
                rt.logs
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .iter()
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    pub(crate) async fn drop_plugins(&self, app: &str) -> CoreResult<()> {
        match self
            .sys
            .write(sys_delete(
                "DELETE FROM plugins WHERE app = ?",
                vec![Value::Text(app.into())],
            ))
            .await
        {
            Ok(_) | Err(monolith_store::Error::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
        self.wh.reload_plugins(app).await
    }
}
