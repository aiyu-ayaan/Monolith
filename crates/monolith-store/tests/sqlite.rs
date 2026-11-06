use std::sync::Arc;

use monolith_store::id::new_id;
use monolith_store::{
    AppStore, Error, MigrationPlan, Mutation, Op, Query, SqliteConfig, SqliteStore, Value, WriteOp,
};

const DDL: &str = "CREATE TABLE t (id BLOB PRIMARY KEY, n INTEGER NOT NULL) STRICT, WITHOUT ROWID";

struct Fixture {
    store: Arc<SqliteStore>,
    insert: Arc<str>,
    update: Arc<str>,
    _dir: tempfile::TempDir,
}

async fn fixture(cfg: SqliteConfig) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(&dir.path().join("data.db"), &cfg).unwrap();
    store
        .apply_schema(MigrationPlan {
            statements: vec![DDL.into()],
        })
        .await
        .unwrap();
    Fixture {
        store: Arc::new(store),
        insert: "INSERT INTO t (id, n) VALUES (?1, ?2)".into(),
        update: "UPDATE t SET n = ?2 WHERE id = ?1".into(),
        _dir: dir,
    }
}

fn mutation(op: Op, sql: &Arc<str>, id: [u8; 16], n: i64) -> Mutation {
    Mutation {
        op,
        coll: 1,
        rid: id,
        fields: None,
        sql: sql.clone(),
        params: vec![Value::Blob(id.into()), Value::Int(n)],
    }
}

fn single(m: Mutation) -> WriteOp {
    WriteOp {
        actor: None,
        mutations: vec![m],
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_writes_get_gap_free_seqs() {
    let f = fixture(SqliteConfig::default()).await;
    let mut bus = f.store.subscribe();
    let tasks: Vec<_> = (0..500)
        .map(|i| {
            let (store, sql) = (f.store.clone(), f.insert.clone());
            tokio::spawn(async move {
                store
                    .write(single(mutation(Op::Insert, &sql, new_id(), i)))
                    .await
            })
        })
        .collect();
    let mut seqs = Vec::new();
    for t in tasks {
        let r = t.await.unwrap().unwrap();
        assert_eq!(r.first_seq, r.last_seq);
        seqs.push(r.first_seq);
    }
    seqs.sort_unstable();
    assert_eq!(seqs, (1..=500).collect::<Vec<_>>());

    let log = f.store.changes_since(0, 1000).await.unwrap();
    assert_eq!(log.len(), 500);
    assert!(
        log.windows(2)
            .all(|w| w[1].seq == w[0].seq + 1 && w[1].hlc > w[0].hlc)
    );

    // The bus delivers the same changes, in seq order, batch by batch.
    let mut seen = 0;
    while seen < 500 {
        let batch = bus.recv().await.unwrap();
        for c in batch.iter() {
            seen += 1;
            assert_eq!(c.seq, seen);
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_op_does_not_poison_the_batch() {
    let f = fixture(SqliteConfig::default()).await;
    let id = new_id();
    f.store
        .write(single(mutation(Op::Insert, &f.insert, id, 1)))
        .await
        .unwrap();

    // Duplicate key, missing row, and a good insert race into the same batches.
    let dup = f
        .store
        .write(single(mutation(Op::Insert, &f.insert, id, 2)));
    let missing = f
        .store
        .write(single(mutation(Op::Update, &f.update, new_id(), 3)));
    let good = f
        .store
        .write(single(mutation(Op::Insert, &f.insert, new_id(), 4)));
    let (dup, missing, good) = tokio::join!(dup, missing, good);
    assert!(matches!(dup, Err(Error::Engine(_))));
    assert!(matches!(missing, Err(Error::NotFound)));
    assert_eq!(good.unwrap().first_seq, 2, "failed ops must not burn seqs");

    let rows = f
        .store
        .query(Query {
            sql: "SELECT n FROM t ORDER BY n".into(),
            params: vec![],
        })
        .await
        .unwrap();
    assert_eq!(rows, vec![vec![Value::Int(1)], vec![Value::Int(4)]]);
}

#[tokio::test(flavor = "multi_thread")]
async fn multi_mutation_op_is_atomic() {
    let f = fixture(SqliteConfig::default()).await;
    let id = new_id();
    let op = WriteOp {
        actor: Some(new_id()),
        mutations: vec![
            mutation(Op::Insert, &f.insert, id, 1),
            mutation(Op::Insert, &f.insert, id, 2), // duplicate: whole op fails
        ],
    };
    assert!(f.store.write(op).await.is_err());
    assert!(f.store.changes_since(0, 10).await.unwrap().is_empty());

    let actor = new_id();
    let op = WriteOp {
        actor: Some(actor),
        mutations: vec![
            mutation(Op::Insert, &f.insert, id, 1),
            mutation(Op::Update, &f.update, id, 2),
        ],
    };
    let r = f.store.write(op).await.unwrap();
    assert_eq!((r.first_seq, r.last_seq), (1, 2));
    let log = f.store.changes_since(0, 10).await.unwrap();
    assert_eq!(log.len(), 2);
    assert!(log.iter().all(|c| c.actor == Some(actor) && c.rid == id));
    assert_eq!(log[1].op, Op::Update);
}

#[tokio::test(flavor = "multi_thread")]
async fn seq_survives_reopen_and_snapshot_works() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.db");
    let insert: Arc<str> = "INSERT INTO t (id, n) VALUES (?1, ?2)".into();
    {
        let store = SqliteStore::open(&path, &SqliteConfig::default()).unwrap();
        store
            .apply_schema(MigrationPlan {
                statements: vec![DDL.into()],
            })
            .await
            .unwrap();
        store
            .write(single(mutation(Op::Insert, &insert, new_id(), 1)))
            .await
            .unwrap();
    }
    let store = SqliteStore::open(&path, &SqliteConfig::default()).unwrap();
    let r = store
        .write(single(mutation(Op::Insert, &insert, new_id(), 2)))
        .await
        .unwrap();
    assert_eq!(r.first_seq, 2);

    let snap = dir.path().join("snap.db");
    let info = store.snapshot(snap.clone()).await.unwrap();
    assert!(info.bytes > 0);
    let copy = SqliteStore::open(&snap, &SqliteConfig::default()).unwrap();
    assert_eq!(copy.changes_since(0, 10).await.unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn full_writer_queue_is_busy_not_growth() {
    let f = fixture(SqliteConfig {
        writer_queue: 1,
        max_batch: 1,
        ..SqliteConfig::default()
    })
    .await;
    let results = futures_join(
        (0..200)
            .map(|i| {
                let (store, sql) = (f.store.clone(), f.insert.clone());
                tokio::spawn(async move {
                    store
                        .write(single(mutation(Op::Insert, &sql, new_id(), i)))
                        .await
                })
            })
            .collect(),
    )
    .await;
    assert!(results.iter().any(|r| matches!(r, Err(Error::Busy))));
    assert!(results.iter().any(|r| r.is_ok()));
}

#[tokio::test]
async fn rejects_empty_write() {
    let f = fixture(SqliteConfig::default()).await;
    let r = f
        .store
        .write(WriteOp {
            actor: None,
            mutations: vec![],
        })
        .await;
    assert!(matches!(r, Err(Error::Invalid(_))));
}

async fn futures_join<T>(tasks: Vec<tokio::task::JoinHandle<T>>) -> Vec<T> {
    let mut out = Vec::with_capacity(tasks.len());
    for t in tasks {
        out.push(t.await.unwrap());
    }
    out
}
