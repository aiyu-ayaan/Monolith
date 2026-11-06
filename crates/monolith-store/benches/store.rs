//! Phase 0 micro-benchmarks (doc 17 §3). Results are hypotheses until recorded in
//! `bench/results` with the hardware they ran on.

use std::hint::black_box;
use std::sync::Arc;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use monolith_store::id::{decode, encode, new_id};
use monolith_store::{
    AppStore, MigrationPlan, Mutation, Op, Query, SqliteConfig, SqliteStore, Value, WriteOp,
};
use tokio::runtime::Runtime;

const CONCURRENCY: u64 = 256;

fn ids(c: &mut Criterion) {
    let id = new_id();
    let wire = encode(&id);
    let wire = std::str::from_utf8(&wire).unwrap().to_owned();
    c.bench_function("id/new_v7", |b| b.iter(new_id));
    c.bench_function("id/encode", |b| b.iter(|| encode(black_box(&id))));
    c.bench_function("id/decode", |b| b.iter(|| decode(black_box(&wire))));
}

fn open(rt: &Runtime, layout: &str, max_batch: usize) -> (tempfile::TempDir, Arc<SqliteStore>) {
    let dir = tempfile::tempdir().unwrap();
    let cfg = SqliteConfig {
        max_batch,
        ..SqliteConfig::default()
    };
    let store = SqliteStore::open(&dir.path().join("data.db"), &cfg).unwrap();
    let ddl = match layout {
        "without_rowid" => {
            "CREATE TABLE t (id BLOB PRIMARY KEY, body TEXT NOT NULL) STRICT, WITHOUT ROWID"
        }
        _ => "CREATE TABLE t (id BLOB NOT NULL UNIQUE, body TEXT NOT NULL) STRICT",
    };
    rt.block_on(store.apply_schema(MigrationPlan {
        statements: vec![ddl.into()],
    }))
    .unwrap();
    (dir, Arc::new(store))
}

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

/// S3-shaped: 256 concurrent 1 KB inserts. Group commit on (256) vs off (1),
/// rowid + UNIQUE vs WITHOUT ROWID (doc 05 §2).
fn writes(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let sql: Arc<str> = "INSERT INTO t (id, body) VALUES (?1, ?2)".into();
    let body: Arc<str> = "x".repeat(1024).into();
    let mut g = c.benchmark_group("write_concurrent_256");
    g.throughput(Throughput::Elements(CONCURRENCY));
    g.sample_size(20);
    for layout in ["rowid", "without_rowid"] {
        for max_batch in [1, 256] {
            let (_dir, store) = open(&rt, layout, max_batch);
            let id = BenchmarkId::new(layout, format!("batch{max_batch}"));
            g.bench_function(id, |b| {
                b.to_async(&rt).iter(|| {
                    let tasks: Vec<_> = (0..CONCURRENCY)
                        .map(|_| {
                            let (store, op) = (store.clone(), insert_op(&sql, &body));
                            tokio::spawn(async move { store.write(op).await.unwrap() })
                        })
                        .collect();
                    async move {
                        for t in tasks {
                            t.await.unwrap();
                        }
                    }
                })
            });
        }
    }
    g.finish();
}

/// S1-shaped point read on 10k rows through the reader pool.
fn point_read(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let sql: Arc<str> = "INSERT INTO t (id, body) VALUES (?1, ?2)".into();
    let body: Arc<str> = "x".repeat(1024).into();
    let (_dir, store) = open(&rt, "without_rowid", 256);
    let mut ids = Vec::with_capacity(10_000);
    rt.block_on(async {
        for _ in 0..10 {
            let tasks: Vec<_> = (0..1000)
                .map(|_| {
                    let (store, op) = (store.clone(), insert_op(&sql, &body));
                    tokio::spawn(async move {
                        let rid = op.mutations[0].rid;
                        store.write(op).await.unwrap();
                        rid
                    })
                })
                .collect();
            for t in tasks {
                ids.push(t.await.unwrap());
            }
        }
    });
    let select: Arc<str> = "SELECT id, body FROM t WHERE id = ?1".into();
    let mut i = 0usize;
    c.bench_function("read/point_by_id", |b| {
        b.to_async(&rt).iter(|| {
            i = (i + 7919) % ids.len();
            let q = Query {
                sql: select.clone(),
                params: vec![Value::Blob(ids[i].into())],
            };
            let store = store.clone();
            async move { black_box(store.query(q).await.unwrap()) }
        })
    });
}

criterion_group!(benches, ids, writes, point_read);
criterion_main!(benches);
