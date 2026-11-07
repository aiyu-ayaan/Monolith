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

#[tokio::test(flavor = "multi_thread")]
async fn backups_carry_storage_and_restore_it() {
    let (dir, reg, f) = setup().await;
    // An app that never used storage gets a plain database backup.
    reg.create_app("plain").await.unwrap();
    assert!(!reg.create_backup("plain").await.unwrap().files);

    f.put_bucket(
        bucket_with(serde_json::json!({"name":"b","public":true,"maxBytes":100})),
        true,
    )
    .await
    .unwrap();
    put(&f, "b", "x/one", "text/plain", b"first").await.unwrap();
    put(&f, "b", "two", "text/plain", b"first").await.unwrap(); // same content
    put(&f, "b", "three", "text/plain", b"second")
        .await
        .unwrap();
    let b = reg.create_backup("s").await.unwrap();
    assert!(b.files);
    assert!(reg.list_backups("s").await.unwrap()[0].files);
    let stem = &b.name[..18];
    let bdir = dir.path().join("backups/s").join(format!("{stem}.files"));
    assert!(bdir.join("files.db").exists());
    let blob_files = |p: &Path| -> usize {
        std::fs::read_dir(p.join("blobs"))
            .unwrap()
            .flatten()
            .map(|d| std::fs::read_dir(d.path()).unwrap().count())
            .sum()
    };
    assert_eq!(
        blob_files(&bdir),
        2,
        "deduplicated blobs, linked not copied"
    );

    // The live app moves on: the backup keeps the old state.
    f.delete_object("b", "three").await.unwrap();
    f.delete_bucket("b", true).await.unwrap();
    assert_eq!(blobs(dir.path()), 0);
    assert_eq!(
        blob_files(&bdir),
        2,
        "hard links survive deleting the originals"
    );

    reg.restore_backup("s", &b.name, "copy").await.unwrap();
    let c = reg.files("copy").await.unwrap();
    let bi = c.bucket_info("b").await.unwrap();
    assert_eq!(
        (bi.objects, bi.bytes, bi.max_bytes, bi.public),
        (3, 16, Some(100), true)
    );
    assert_eq!(read(c.open_object("b", "x/one").await.unwrap().1), b"first");
    assert_eq!(
        read(c.open_object("b", "three").await.unwrap().1),
        b"second"
    );
    // The restored app is independent: deleting there does not touch the backup.
    c.delete_object("b", "two").await.unwrap();
    c.delete_object("b", "x/one").await.unwrap();
    assert_eq!(blob_files(&bdir), 2);

    reg.delete_backup("s", &b.name).await.unwrap();
    assert!(!bdir.exists());
}

fn png(w: u32, h: u32, alpha: bool) -> Vec<u8> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    let enc = image::codecs::png::PngEncoder::new(&mut out);
    if alpha {
        enc.write_image(
            &vec![200u8; (w * h * 4) as usize],
            w,
            h,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    } else {
        let px: Vec<u8> = (0..w * h * 3).map(|i| (i % 251) as u8).collect();
        enc.write_image(&px, w, h, image::ExtendedColorType::Rgb8)
            .unwrap();
    }
    out
}

fn dims(f: std::fs::File) -> (u32, u32) {
    image::ImageReader::new(std::io::BufReader::new(f))
        .with_guessed_format()
        .unwrap()
        .into_dimensions()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn thumbnails_are_declared_cached_and_cleaned_up() {
    let (dir, _reg, f) = setup().await;
    assert!(matches!(
        f.put_bucket(
            bucket_with(serde_json::json!({"name":"x","thumbSizes":["9999x1"]})),
            true
        )
        .await,
        Err(CoreError::Validation(_))
    ));
    f.put_bucket(
        bucket_with(serde_json::json!({"name":"img","thumbSizes":["50x50","100x0","80x80f"]})),
        true,
    )
    .await
    .unwrap();
    put(&f, "img", "photo.png", "image/png", &png(200, 100, false))
        .await
        .unwrap();
    put(&f, "img", "logo.png", "image/png", &png(64, 64, true))
        .await
        .unwrap();
    put(&f, "img", "note.txt", "text/plain", b"hello")
        .await
        .unwrap();

    // Crop to fill, keep aspect, fit inside; opaque sources become JPEG.
    let (i, file) = f.thumbnail("img", "photo.png", "50x50").await.unwrap();
    assert_eq!((dims(file), i.mime.as_str()), ((50, 50), "image/jpeg"));
    assert!(i.etag.ends_with("-50x50") && i.size > 0);
    assert_eq!(
        dims(f.thumbnail("img", "photo.png", "100x0").await.unwrap().1),
        (100, 50)
    );
    assert_eq!(
        dims(f.thumbnail("img", "photo.png", "80x80f").await.unwrap().1),
        (80, 40)
    );
    // Transparency is kept (PNG out).
    let (i, file) = f.thumbnail("img", "logo.png", "50x50").await.unwrap();
    assert_eq!((dims(file), i.mime.as_str()), ((50, 50), "image/png"));

    // Only declared sizes, only images.
    assert!(matches!(
        f.thumbnail("img", "photo.png", "60x60").await,
        Err(CoreError::BadRequest(_))
    ));
    assert!(matches!(
        f.thumbnail("img", "note.txt", "50x50").await,
        Err(CoreError::BadRequest(_))
    ));
    assert!(matches!(
        f.thumbnail("img", "missing.png", "50x50").await,
        Err(CoreError::NotFound(_))
    ));

    // Many requests for a new size share one job and one cached file.
    let hash = blake3::hash(&png(200, 100, false)).to_hex().to_string();
    let tdir = dir.path().join("apps/s/files/thumbs").join(&hash);
    std::fs::remove_file(tdir.join("50x50.jpg")).unwrap();
    let all = futures_all((0..8).map(|_| {
        let f = f.clone();
        async move {
            f.thumbnail("img", "photo.png", "50x50")
                .await
                .map(|(i, _)| i.etag)
        }
    }))
    .await;
    assert!(
        all.iter()
            .all(|r| r.as_ref().unwrap() == &format!("{hash}-50x50"))
    );
    assert_eq!(
        std::fs::read_dir(&tdir).unwrap().count(),
        3,
        "50x50.jpg, 100x0.jpg, 80x80f.jpg"
    );
    assert_eq!(
        std::fs::read_dir(dir.path().join("apps/s/files/.tmp"))
            .unwrap()
            .count(),
        0
    );

    // Thumbnails disappear with the last reference to the blob.
    f.delete_object("img", "photo.png").await.unwrap();
    assert!(!tdir.exists());

    // Oversized sources are refused before decoding.
    let big = png_gray(8000, 6251);
    put(&f, "img", "big.png", "image/png", &big).await.unwrap();
    assert!(matches!(
        f.thumbnail("img", "big.png", "50x50").await,
        Err(CoreError::TooLarge(_))
    ));
}

fn png_gray(w: u32, h: u32) -> Vec<u8> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(
            &vec![0u8; (w * h) as usize],
            w,
            h,
            image::ExtendedColorType::L8,
        )
        .unwrap();
    out
}

async fn futures_all<T, F: std::future::Future<Output = T> + Send + 'static>(
    it: impl Iterator<Item = F>,
) -> Vec<T>
where
    T: Send + 'static,
{
    let hs: Vec<_> = it.map(tokio::spawn).collect();
    let mut out = Vec::new();
    for h in hs {
        out.push(h.await.unwrap());
    }
    out
}
