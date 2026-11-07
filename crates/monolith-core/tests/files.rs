use std::io::Read;
use std::path::Path;

use monolith_core::{BucketIn, CoreError, Files, ObjectInfo, Registry};

fn bucket(name: &str) -> BucketIn {
    serde_json::from_value(serde_json::json!({"name": name})).unwrap()
}

fn bucket_with(v: serde_json::Value) -> BucketIn {
    serde_json::from_value(v).unwrap()
}

async fn put(
    f: &Files,
    b: &str,
    key: &str,
    mime: &str,
    data: &[u8],
) -> Result<ObjectInfo, CoreError> {
    let mut up = f.begin_upload(b, key, mime).await?;
    let data = data.to_vec();
    let fin = tokio::task::spawn_blocking(move || {
        up.write(&data)?;
        up.finish()
    })
    .await
    .unwrap()?;
    f.commit(fin).await
}

/// Blob files under `blobs/`.
fn blobs(dir: &Path) -> usize {
    let root = dir.join("apps/s/files/blobs");
    std::fs::read_dir(root)
        .map(|rd| {
            rd.flatten()
                .map(|d| std::fs::read_dir(d.path()).unwrap().count())
                .sum()
        })
        .unwrap_or(0)
}

fn read(mut f: std::fs::File) -> Vec<u8> {
    let mut v = Vec::new();
    f.read_to_end(&mut v).unwrap();
    v
}

async fn setup() -> (tempfile::TempDir, Registry, std::sync::Arc<Files>) {
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::open(dir.path()).await.unwrap();
    reg.create_app("s").await.unwrap();
    let f = reg.files("s").await.unwrap();
    (dir, reg, f)
}

#[tokio::test(flavor = "multi_thread")]
async fn buckets_objects_overwrite_list_and_limits() {
    let (dir, reg, f) = setup().await;
    f.put_bucket(
        bucket_with(
            serde_json::json!({"name":"pics","public":true,"maxSize":10,"allowedMime":["image/*"]}),
        ),
        true,
    )
    .await
    .unwrap();
    assert!(matches!(
        f.put_bucket(bucket("pics"), true).await,
        Err(CoreError::Conflict(_))
    ));
    assert!(matches!(
        f.put_bucket(bucket("Bad"), true).await,
        Err(CoreError::Validation(_))
    ));
    assert!(f.bucket("pics").unwrap().public);

    // The etag is the BLAKE3 of the content; the bytes are stored under it.
    let o = put(&f, "pics", "a/b.png", "image/png", b"12345")
        .await
        .unwrap();
    assert_eq!(
        (o.size, o.etag.as_str()),
        (5, blake3::hash(b"12345").to_hex().as_str())
    );
    let (info, file) = f.open_object("pics", "a/b.png").await.unwrap();
    assert_eq!(
        (read(file), info.mime.as_str()),
        (b"12345".to_vec(), "image/png")
    );

    // Limits and validation reject before anything is published.
    assert!(matches!(
        put(&f, "pics", "x.png", "image/png", &[0; 11]).await,
        Err(CoreError::TooLarge(_))
    ));
    assert!(matches!(
        put(&f, "pics", "x.txt", "text/plain", b"x").await,
        Err(CoreError::BadRequest(_))
    ));
    for bad in ["", "/a", "a//b", "a/../b", "a\\b", "a\nb"] {
        assert!(
            matches!(
                put(&f, "pics", bad, "image/png", b"x").await,
                Err(CoreError::BadRequest(_))
            ),
            "{bad:?}"
        );
    }
    assert!(matches!(
        put(&f, "nope", "k", "image/png", b"x").await,
        Err(CoreError::NotFound(_))
    ));
    assert_eq!(blobs(dir.path()), 1);
    assert_eq!(
        std::fs::read_dir(dir.path().join("apps/s/files/.tmp"))
            .unwrap()
            .count(),
        0
    );

    // Overwrite keeps `created`, swaps the blob and removes the old one at once.
    let o2 = put(&f, "pics", "a/b.png", "image/jpeg", b"abc")
        .await
        .unwrap();
    assert_eq!((o2.size, o2.created.as_str()), (3, o.created.as_str()));
    assert_eq!(blobs(dir.path()), 1);

    // Keyset listing with prefix.
    for (k, d) in [("a/c.png", b"c"), ("a/d.png", b"d"), ("z.png", b"z")] {
        put(&f, "pics", k, "image/png", d).await.unwrap();
    }
    let p1 = f.list_objects("pics", "a/", 2, None).await.unwrap();
    assert_eq!(
        p1.items.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(),
        ["a/b.png", "a/c.png"]
    );
    let p2 = f
        .list_objects("pics", "a/", 2, p1.next_cursor.as_deref())
        .await
        .unwrap();
    assert_eq!(p2.items.len(), 1);
    assert!(p2.next_cursor.is_none());
    assert_eq!(
        f.list_objects("pics", "", 10, None)
            .await
            .unwrap()
            .items
            .len(),
        4
    );

    // Counters follow every change; a non-empty bucket needs force.
    let bi = f.bucket_info("pics").await.unwrap();
    assert_eq!((bi.objects, bi.bytes), (4, 6));
    f.delete_object("pics", "z.png").await.unwrap();
    assert!(matches!(
        f.delete_object("pics", "z.png").await,
        Err(CoreError::NotFound(_))
    ));
    assert_eq!(f.bucket_info("pics").await.unwrap().objects, 3);
    assert!(matches!(
        f.delete_bucket("pics", false).await,
        Err(CoreError::Conflict(_))
    ));
    f.delete_bucket("pics", true).await.unwrap();
    assert!(f.list_buckets().await.unwrap().is_empty());
    assert_eq!(blobs(dir.path()), 0, "force delete releases every blob");

    // Config survives a restart.
    f.put_bucket(bucket("docs"), true).await.unwrap();
    drop(f);
    drop(reg);
    let reg = Registry::open(dir.path()).await.unwrap();
    let f = reg.files("s").await.unwrap();
    assert_eq!(f.list_buckets().await.unwrap()[0].name, "docs");

    // Deleting the app drops its storage.
    reg.delete_app("s").await.unwrap();
    assert!(matches!(reg.files("s").await, Err(CoreError::NotFound(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn identical_content_is_stored_once_until_the_last_reference() {
    let (dir, _reg, f) = setup().await;
    f.put_bucket(bucket("a"), true).await.unwrap();
    f.put_bucket(bucket("b"), true).await.unwrap();
    let body = vec![7u8; 4096];
    put(&f, "a", "one", "text/plain", &body).await.unwrap();
    put(&f, "a", "two", "text/plain", &body).await.unwrap();
    put(&f, "b", "three", "text/plain", &body).await.unwrap();
    assert_eq!(blobs(dir.path()), 1, "three objects, one file");
    // Logical size still counts per object.
    assert_eq!(f.bucket_info("a").await.unwrap().bytes, 8192);

    // Re-uploading the same content under the same key changes nothing.
    put(&f, "a", "one", "text/plain", &body).await.unwrap();
    assert_eq!(
        (blobs(dir.path()), f.bucket_info("a").await.unwrap().objects),
        (1, 2)
    );

    f.delete_object("a", "one").await.unwrap();
    f.delete_object("a", "two").await.unwrap();
    assert_eq!(blobs(dir.path()), 1, "still referenced from bucket b");
    assert_eq!(read(f.open_object("b", "three").await.unwrap().1), body);
    // Replacing the last reference with other content frees the old blob.
    put(&f, "b", "three", "text/plain", b"other").await.unwrap();
    assert_eq!(blobs(dir.path()), 1);
    f.delete_object("b", "three").await.unwrap();
    assert_eq!(blobs(dir.path()), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn quota_and_sniffed_types() {
    let (_dir, _reg, f) = setup().await;
    f.put_bucket(
        bucket_with(serde_json::json!({"name":"q","maxBytes":10})),
        true,
    )
    .await
    .unwrap();
    put(&f, "q", "a", "text/plain", b"123456").await.unwrap();
    assert!(matches!(
        put(&f, "q", "b", "text/plain", b"12345").await,
        Err(CoreError::TooLarge(_))
    ));
    // Replacing counts only the difference.
    put(&f, "q", "a", "text/plain", b"1234567890")
        .await
        .unwrap();
    assert!(matches!(
        put(&f, "q", "a", "text/plain", b"12345678901").await,
        Err(CoreError::TooLarge(_))
    ));
    assert_eq!(f.bucket_info("q").await.unwrap().bytes, 10);
    // Raising the quota takes effect without touching the objects.
    f.put_bucket(
        bucket_with(serde_json::json!({"name":"q","maxBytes":100})),
        false,
    )
    .await
    .unwrap();
    assert_eq!(
        f.bucket_info("q").await.unwrap().bytes,
        10,
        "counters survive a config change"
    );
    put(&f, "q", "b", "text/plain", b"12345").await.unwrap();

    // The real type wins over the declared one, and the allow-list judges the real one.
    f.put_bucket(
        bucket_with(serde_json::json!({"name":"img","allowedMime":["image/png"]})),
        true,
    )
    .await
    .unwrap();
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    let o = put(&f, "img", "p", "application/octet-stream", &png)
        .await
        .unwrap();
    assert_eq!(o.mime, "image/png");
    let fake = b"<script>alert(1)</script>";
    assert!(
        put(&f, "img", "x", "image/png", fake).await.is_err(),
        "declared png, real html"
    );
    let gif = b"GIF89a\x01\0\x01\0".to_vec();
    assert!(
        matches!(
            put(&f, "img", "g", "image/png", &gif).await,
            Err(CoreError::BadRequest(_))
        ),
        "declared png, real gif"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn gc_removes_orphans_and_reports_missing_files() {
    let (dir, _reg, f) = setup().await;
    // Opening the app's storage starts one background gc; let it finish so it cannot take our orphan.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    f.put_bucket(bucket("b"), true).await.unwrap();
    put(&f, "b", "k", "text/plain", b"keep").await.unwrap();
    let hash = blake3::hash(b"keep").to_hex().to_string();
    let orphan = dir
        .path()
        .join("apps/s/files/blobs/ab")
        .join("f".repeat(64));
    std::fs::create_dir_all(orphan.parent().unwrap()).unwrap();
    std::fs::write(&orphan, b"orphan").unwrap();
    // Fresh files are spared (a commit may be about to claim them); old ones go.
    assert_eq!(f.gc().await.unwrap().removed_files, 0);
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(&orphan)
        .unwrap()
        .set_modified(old)
        .unwrap();
    let r = f.gc().await.unwrap();
    assert_eq!(
        (r.removed_files, r.removed_bytes, r.missing_files),
        (1, 6, 0)
    );
    assert!(!orphan.exists());
    // A referenced blob without its file is reported, not repaired.
    std::fs::remove_file(
        dir.path()
            .join("apps/s/files/blobs")
            .join(&hash[..2])
            .join(&hash),
    )
    .unwrap();
    assert_eq!(f.gc().await.unwrap().missing_files, 1);
    assert!(matches!(
        f.open_object("b", "k").await,
        Err(CoreError::NotFound(_))
    ));
}

/// Data written by the first storage layout (`<bucket>/<id>` files, no counters) is upgraded on open.
#[tokio::test(flavor = "multi_thread")]
async fn legacy_layout_is_migrated_on_open() {
    use monolith_store::{
        AppStore, MigrationPlan, Mutation, Op, SqliteConfig, SqliteStore, Value, WriteOp,
    };
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::open(dir.path()).await.unwrap();
    reg.create_app("s").await.unwrap();
    let root = dir.path().join("apps/s/files");
    std::fs::create_dir_all(root.join("old")).unwrap();
    std::fs::write(root.join("old/ID1"), b"hello").unwrap();
    std::fs::write(root.join("old/ID2"), b"hello").unwrap();
    {
        let st = SqliteStore::open(
            &root.join("files.db"),
            &SqliteConfig {
                readers: 1,
                ..Default::default()
            },
        )
        .unwrap();
        st.apply_schema(MigrationPlan {
            statements: vec![
                "CREATE TABLE buckets (name TEXT PRIMARY KEY, public INTEGER NOT NULL, max_size INTEGER NOT NULL, mime TEXT NOT NULL, created INTEGER NOT NULL) STRICT;
                 CREATE TABLE objects (bucket TEXT NOT NULL, key TEXT NOT NULL, blob TEXT NOT NULL, size INTEGER NOT NULL, mime TEXT NOT NULL, etag TEXT NOT NULL, created INTEGER NOT NULL, updated INTEGER NOT NULL, PRIMARY KEY (bucket, key)) STRICT, WITHOUT ROWID;".into(),
            ],
        })
        .await
        .unwrap();
        let ins = |sql: &str, p: Vec<Value>| WriteOp {
            actor: None,
            mutations: vec![Mutation {
                op: Op::Insert,
                coll: monolith_store::NO_CHANGE_LOG,
                rid: [0; 16],
                fields: None,
                sql: sql.into(),
                params: p,
                seq_slot: None,
                hlc_slot: None,
                post_sql: vec![],
            }],
        };
        let t = |s: &str| Value::Text(s.into());
        st.write(ins(
            "INSERT INTO buckets VALUES ('old', 0, 1000, '[]', 1)",
            vec![],
        ))
        .await
        .unwrap();
        for (k, id) in [("a", "ID1"), ("b", "ID2")] {
            st.write(ins(
                "INSERT INTO objects VALUES ('old', ?, ?, 5, 'text/plain', 'x', 1, 1)",
                vec![t(k), t(id)],
            ))
            .await
            .unwrap();
        }
    }
    let f = reg.files("s").await.unwrap();
    let bi = f.bucket_info("old").await.unwrap();
    assert_eq!((bi.objects, bi.bytes), (2, 10));
    assert_eq!(read(f.open_object("old", "a").await.unwrap().1), b"hello");
    assert_eq!(
        f.object_info("old", "b").await.unwrap().etag,
        blake3::hash(b"hello").to_hex().as_str()
    );
    assert_eq!(blobs(dir.path()), 1, "the two copies were deduplicated");
    f.delete_object("old", "a").await.unwrap();
    f.delete_object("old", "b").await.unwrap();
    assert_eq!(blobs(dir.path()), 0);
}
