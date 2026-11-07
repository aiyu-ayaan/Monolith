use monolith_core::{BucketIn, CoreError, Files, ObjectInfo, Registry};
use std::io::Read;

fn bucket(name: &str) -> BucketIn {
    serde_json::from_value(serde_json::json!({"name": name})).unwrap()
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

fn blobs(dir: &std::path::Path, bucket: &str) -> usize {
    std::fs::read_dir(dir.join("apps/s/files").join(bucket))
        .unwrap()
        .count()
}

#[tokio::test(flavor = "multi_thread")]
async fn buckets_objects_overwrite_list_and_limits() {
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::open(dir.path()).await.unwrap();
    reg.create_app("s").await.unwrap();
    let f = reg.files("s").await.unwrap();

    f.put_bucket(
        serde_json::from_value(
            serde_json::json!({"name":"pics","public":true,"maxSize":10,"allowedMime":["image/*"]}),
        )
        .unwrap(),
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

    // Upload, read back, etag is the SHA-256 of the content.
    let o = put(&f, "pics", "a/b.png", "image/png", b"12345")
        .await
        .unwrap();
    assert_eq!(o.size, 5);
    assert_eq!(
        o.etag,
        "5994471abb01112afcc18159f6cc74b4f511b99806da59b3caf5a9c173cacfc5"
    );
    let (info, mut file) = f.open_object("pics", "a/b.png").await.unwrap();
    let mut s = String::new();
    file.read_to_string(&mut s).unwrap();
    assert_eq!((s.as_str(), info.mime.as_str()), ("12345", "image/png"));

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
    assert_eq!(blobs(dir.path(), "pics"), 1);
    assert_eq!(
        std::fs::read_dir(dir.path().join("apps/s/files/.tmp"))
            .unwrap()
            .count(),
        0
    );

    // Overwrite keeps `created`, replaces the blob and removes the old one.
    let o2 = put(&f, "pics", "a/b.png", "image/jpeg", b"abc")
        .await
        .unwrap();
    assert_eq!((o2.size, o2.created.as_str()), (3, o.created.as_str()));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(blobs(dir.path(), "pics"), 1);

    // Keyset listing with prefix.
    for k in ["a/c.png", "a/d.png", "z.png"] {
        put(&f, "pics", k, "image/png", b"x").await.unwrap();
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

    // Usage, delete object, non-empty bucket needs force.
    let bi = f.bucket_info("pics").await.unwrap();
    assert_eq!((bi.objects, bi.bytes), (4, 6));
    f.delete_object("pics", "z.png").await.unwrap();
    assert!(matches!(
        f.delete_object("pics", "z.png").await,
        Err(CoreError::NotFound(_))
    ));
    assert!(matches!(
        f.delete_bucket("pics", false).await,
        Err(CoreError::Conflict(_))
    ));
    f.delete_bucket("pics", true).await.unwrap();
    assert!(f.list_buckets().await.unwrap().is_empty());
    assert!(!dir.path().join("apps/s/files/pics").exists());

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
