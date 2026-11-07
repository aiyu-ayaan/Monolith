use std::time::Duration;

use monolith_core::{CollectionDef, CoreError, Principal, Registry};
use serde_json::{Value as J, json};

fn def(v: J) -> CollectionDef {
    serde_json::from_value(v).unwrap()
}

/// Next `record` frame as JSON, skipping `hello`; `None` on timeout.
async fn next(sub: &mut monolith_core::realtime::Subscription) -> Option<J> {
    loop {
        let f = tokio::time::timeout(Duration::from_millis(500), sub.rx.recv())
            .await
            .ok()??;
        if let Some(d) = f.split("data: ").nth(1)
            && f.contains("event: record")
        {
            return Some(serde_json::from_str(d.trim()).unwrap());
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn realtime_events_respect_view_rules_and_hidden_fields() {
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::open(dir.path()).await.unwrap();
    reg.create_app("rt").await.unwrap();
    let app = reg.app("rt").await.unwrap();
    let su = Principal::superuser();
    app.put_collection(
        def(json!({"name":"users","fields":[],"rules":{"list":"","view":""}})),
        true,
    )
    .await
    .unwrap();
    app.put_collection(
        def(json!({"name":"posts","fields":[
            {"name":"title","type":"text"},
            {"name":"public","type":"bool"},
            {"name":"secret","type":"text","hidden":true}
        ],"rules":{"list":"","view":"public = true"}})),
        true,
    )
    .await
    .unwrap();
    app.put_collection(
        def(json!({"name":"locked","fields":[{"name":"n","type":"number"}]})),
        true,
    )
    .await
    .unwrap();

    let anon = Principal::default();
    let mut s_su = app.subscribe(&su, &["posts".into()]).unwrap();
    let mut s_anon = app.subscribe(&anon, &["posts".into()]).unwrap();
    // Locked view rule (null) and unknown collections are refused for anonymous callers.
    assert!(matches!(
        app.subscribe(&anon, &["locked".into()]),
        Err(CoreError::Forbidden(_))
    ));
    assert!(matches!(
        app.subscribe(&su, &["nope".into()]),
        Err(CoreError::NotFound(_))
    ));
    assert!(matches!(
        app.subscribe(&su, &[]),
        Err(CoreError::BadRequest(_))
    ));
    assert_eq!(app.stats(false).await.unwrap().realtime.connections, 2);

    // One write at a time: an event carries the row as it is when the hub dispatches it.
    let a = app
        .create_record(
            &su,
            "posts",
            br#"{"title":"hi","public":true,"secret":"s"}"#,
        )
        .await
        .unwrap();
    let a: J = serde_json::from_slice(&a).unwrap();
    let id = a["id"].as_str().unwrap();
    // Superuser sees hidden fields; anonymous gets the public row without them.
    let e1 = next(&mut s_su).await.unwrap();
    assert_eq!(
        (e1["op"].as_str(), e1["record"]["secret"].as_str()),
        (Some("create"), Some("s"))
    );
    let g1 = next(&mut s_anon).await.unwrap();
    assert_eq!(g1["record"]["title"], "hi");
    assert!(g1["record"].get("secret").is_none());

    app.create_record(
        &su,
        "posts",
        br#"{"title":"private","public":false,"secret":"s"}"#,
    )
    .await
    .unwrap();
    assert_eq!(next(&mut s_su).await.unwrap()["record"]["title"], "private");
    assert!(
        next(&mut s_anon).await.is_none(),
        "private row must not leak"
    );

    app.update_record(&su, "posts", id, br#"{"title":"hi2"}"#)
        .await
        .unwrap();
    let e3 = next(&mut s_su).await.unwrap();
    assert_eq!(
        (e3["op"].as_str(), e3["record"]["title"].as_str()),
        (Some("update"), Some("hi2"))
    );
    assert_eq!(next(&mut s_anon).await.unwrap()["record"]["title"], "hi2");

    // Deletes reach a non-superuser only when the view rule is public; superusers always.
    app.delete_record(&su, "posts", id).await.unwrap();
    assert_eq!(next(&mut s_su).await.unwrap()["op"], "delete");
    assert!(next(&mut s_anon).await.is_none());

    drop(s_anon);
    assert_eq!(app.stats(false).await.unwrap().realtime.connections, 1);
    reg.close_realtime();
    assert!(s_su.rx.recv().await.is_none(), "close ends streams");
}
