//! Engine bake-off harness (doc 04 §6, doc 17 §3). Drives an `AppStore` in-process,
//! so it measures the engine, not HTTP.
//!
//! Usage: monolith-bench <s1|s3> [--concurrency N] [--seconds N] [--rows N]
//!        [--max-batch N] [--layout rowid|without_rowid] [--durability normal|full]
//!        [--dir PATH]
//!
//! s1: point read by id over `--rows` preloaded rows.
//! s3: concurrent 1 KB single-row inserts.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use monolith_store::id::new_id;
use monolith_store::{
    AppStore, Error, MigrationPlan, Mutation, Op, Query, SqliteConfig, SqliteStore, Value, WriteOp,
};

struct Args {
    scenario: String,
    concurrency: usize,
    seconds: u64,
    rows: usize,
    max_batch: usize,
    layout: String,
    durability_full: bool,
    dir: Option<String>,
}

fn parse() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let scenario = it.next().ok_or("missing scenario: s1 or s3")?;
    if scenario != "s1" && scenario != "s3" {
        return Err(format!("unknown scenario {scenario}"));
    }
    let mut a = Args {
        scenario,
        concurrency: 256,
        seconds: 10,
        rows: 100_000,
        max_batch: 256,
        layout: "without_rowid".into(),
        durability_full: false,
        dir: None,
    };
    while let Some(flag) = it.next() {
        let v = it.next().ok_or(format!("{flag} needs a value"))?;
        let num = || v.parse::<usize>().map_err(|e| format!("{flag}: {e}"));
        match flag.as_str() {
            "--concurrency" => a.concurrency = num()?,
            "--seconds" => a.seconds = num()? as u64,
            "--rows" => a.rows = num()?,
            "--max-batch" => a.max_batch = num()?,
            "--layout" if v == "rowid" || v == "without_rowid" => a.layout = v,
            "--durability" if v == "normal" || v == "full" => a.durability_full = v == "full",
            "--dir" => a.dir = Some(v),
            _ => return Err(format!("bad flag {flag} {v}")),
        }
    }
    if a.concurrency == 0 || a.seconds == 0 || a.rows == 0 || a.max_batch == 0 {
        return Err("numeric flags must be > 0".into());
    }
    Ok(a)
}

const INSERT: &str = "INSERT INTO t (id, body) VALUES (?1, ?2)";
const SELECT: &str = "SELECT id, body FROM t WHERE id = ?1";

fn insert_op(sql: &Arc<str>, body: &Arc<str>) -> WriteOp {
    let id = new_id();
    WriteOp {
        actor: None,
        mutations: vec![Mutation {
            op: Op::Insert,
            coll: 1,
            rid: id,
            fields: None,
            sql: sql.clone(),
            params: vec![Value::Blob(id.into()), Value::Text(body.as_ref().into())],
        }],
    }
}

#[derive(Default)]
struct Stats {
    lat_us: Vec<u32>,
    busy: u64,
    errors: u64,
}

/// Runs `op` from `concurrency` tasks until the deadline and merges latencies.
async fn drive<S, F, Fut>(store: Arc<S>, a: &Args, op: F) -> (Stats, Duration)
where
    S: AppStore + 'static,
    F: Fn(Arc<S>, u64) -> Fut + Send + Sync + Clone + 'static,
    Fut: Future<Output = Result<(), Error>> + Send,
{
    let start = Instant::now();
    let deadline = start + Duration::from_secs(a.seconds);
    let tasks: Vec<_> = (0..a.concurrency as u64)
        .map(|t| {
            let (store, op) = (store.clone(), op.clone());
            tokio::spawn(async move {
                let mut s = Stats::default();
                let mut n = t;
                while Instant::now() < deadline {
                    let t0 = Instant::now();
                    match op(store.clone(), n).await {
                        Ok(()) => s.lat_us.push(t0.elapsed().as_micros() as u32),
                        Err(Error::Busy) => {
                            s.busy += 1;
                            tokio::task::yield_now().await;
                        }
                        Err(_) => s.errors += 1,
                    }
                    n += 7919;
                }
                s
            })
        })
        .collect();
    let mut all = Stats::default();
    for t in tasks {
        let s = t.await.expect("bench task panicked");
        all.lat_us.extend(s.lat_us);
        all.busy += s.busy;
        all.errors += s.errors;
    }
    (all, start.elapsed())
}

fn report(a: &Args, mut s: Stats, elapsed: Duration) {
    s.lat_us.sort_unstable();
    let pct = |p: f64| {
        s.lat_us
            .get(((s.lat_us.len() as f64 * p) as usize).min(s.lat_us.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0)
    };
    let cores = std::thread::available_parallelism().map_or(0, |n| n.get());
    println!(
        "scenario={} engine=sqlite layout={} max_batch={} durability={} concurrency={} cores={cores}",
        a.scenario,
        a.layout,
        a.max_batch,
        if a.durability_full { "full" } else { "normal" },
        a.concurrency,
    );
    println!(
        "ok={} ops/s={:.0} p50_us={} p99_us={} max_us={} busy={} errors={}",
        s.lat_us.len(),
        s.lat_us.len() as f64 / elapsed.as_secs_f64(),
        pct(0.50),
        pct(0.99),
        s.lat_us.last().copied().unwrap_or(0),
        s.busy,
        s.errors,
    );
}

#[tokio::main]
async fn main() -> ExitCode {
    let a = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = a.dir.as_deref().map_or(tmp.path().to_owned(), Into::into);
    let cfg = SqliteConfig {
        max_batch: a.max_batch,
        durability_full: a.durability_full,
        ..SqliteConfig::default()
    };
    let store = Arc::new(SqliteStore::open(&dir.join("bench.db"), &cfg).expect("open store"));
    let ddl = if a.layout == "rowid" {
        "CREATE TABLE t (id BLOB NOT NULL UNIQUE, body TEXT NOT NULL) STRICT"
    } else {
        "CREATE TABLE t (id BLOB PRIMARY KEY, body TEXT NOT NULL) STRICT, WITHOUT ROWID"
    };
    store
        .apply_schema(MigrationPlan {
            statements: vec![ddl.into()],
        })
        .await
        .expect("create table");

    let insert: Arc<str> = INSERT.into();
    let body: Arc<str> = "x".repeat(1024).into();
    let (stats, elapsed) = if a.scenario == "s3" {
        drive(store, &a, move |s, _| {
            let op = insert_op(&insert, &body);
            async move { s.write(op).await.map(drop) }
        })
        .await
    } else {
        eprintln!("loading {} rows", a.rows);
        let mut ids = Vec::with_capacity(a.rows);
        for chunk in (0..a.rows).collect::<Vec<_>>().chunks(1000) {
            let tasks: Vec<_> = chunk
                .iter()
                .map(|_| {
                    let (store, op) = (store.clone(), insert_op(&insert, &body));
                    tokio::spawn(async move {
                        let rid = op.mutations[0].rid;
                        store.write(op).await.map(|_| rid)
                    })
                })
                .collect();
            for t in tasks {
                ids.push(t.await.expect("load task").expect("load insert"));
            }
        }
        let ids = Arc::new(ids);
        let select: Arc<str> = SELECT.into();
        drive(store, &a, move |s, n| {
            let q = Query {
                sql: select.clone(),
                params: vec![Value::Blob(ids[n as usize % ids.len()].into())],
            };
            async move { s.query(q).await.map(drop) }
        })
        .await
    };
    report(&a, stats, elapsed);
    ExitCode::SUCCESS
}
