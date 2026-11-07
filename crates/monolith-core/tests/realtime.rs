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

/// Next frame named `event`, with its `id:` line and JSON data; `None` on timeout.
async fn next_event(
    sub: &mut monolith_core::realtime::Subscription,
    event: &str,
) -> Option<(Option<u64>, J)> {
    loop {
        let f = tokio::time::timeout(Duration::from_millis(800), sub.rx.recv())
            .await
            .ok()??;
        if f.contains(&format!("event: {event}\n")) {
            let id = f
                .strip_prefix("id: ")
                .and_then(|r| r.split('\n').next())
                .and_then(|n| n.parse().ok());
            let d = f.split("data: ").nth(1)?;
            return Some((id, serde_json::from_str(d.trim()).unwrap()));
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn filtered_topic_snapshot_leave_delete_and_since_replay() {
    use monolith_core::realtime::TopicSpec;
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::open(dir.path()).await.unwrap();
    reg.create_app("rt2").await.unwrap();
    let app = reg.app("rt2").await.unwrap();
    let su = Principal::superuser();
    app.put_collection(
        def(json!({"name":"users","fields":[],"rules":{"list":"","view":""}})),
        true,
    )
    .await
    .unwrap();
    app.put_collection(
        def(json!({"name":"tasks","fields":[
            {"name":"title","type":"text"},
            {"name":"done","type":"bool"}
        ],"rules":{"list":"","view":""}})),
        true,
    )
    .await
    .unwrap();
    let mk = |t: &str, d: bool| format!(r#"{{"title":"{t}","done":{d}}}"#);
    let a: J = serde_json::from_slice(
        &app.create_record(&su, "tasks", mk("a", false).as_bytes())
            .await
            .unwrap(),
    )
    .unwrap();
    let a = a["id"].as_str().unwrap().to_owned();
    app.create_record(&su, "tasks", mk("b", true).as_bytes())
        .await
        .unwrap();

    // Snapshot holds only matching records; unrelated changes stay out.
    let anon = Principal::default();
    let spec = TopicSpec {
        collection: "tasks".into(),
        filter: "done = false".into(),
        snapshot: Some(10),
        ..Default::default()
    };
    let mut s = app
        .subscribe_with(&anon, std::slice::from_ref(&spec), None)
        .unwrap();
    let (_, snap) = next_event(&mut s, "snapshot").await.unwrap();
    let items = snap["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["title"], "a");
    let seq_at_snapshot = snap["seq"].as_u64().unwrap();

    app.create_record(&su, "tasks", mk("c", true).as_bytes())
        .await
        .unwrap();
    assert!(
        next(&mut s).await.is_none(),
        "non-matching create is skipped"
    );

    // The record stops matching: `leave`, not silence.
    app.update_record(&su, "tasks", &a, br#"{"done":true}"#)
        .await
        .unwrap();
    let leave = next(&mut s).await.unwrap();
    assert_eq!(leave["op"], "leave");
    assert_eq!(leave["record"]["id"], a.as_str());

    // Once it matches again it re-enters as an update; deleting it is then delivered.
    app.update_record(&su, "tasks", &a, br#"{"done":false}"#)
        .await
        .unwrap();
    assert_eq!(next(&mut s).await.unwrap()["op"], "update");
    app.delete_record(&su, "tasks", &a).await.unwrap();
    assert_eq!(next(&mut s).await.unwrap()["op"], "delete");

    // Per-record topic with `since`: replays what happened after the given seq.
    let only = TopicSpec {
        collection: "tasks".into(),
        ids: vec![a.clone()],
        ..Default::default()
    };
    let mut r = app
        .subscribe_with(&su, std::slice::from_ref(&only), Some(seq_at_snapshot))
        .unwrap();
    // `a` was updated twice then deleted after the snapshot; the live rows are gone, so only
    // the delete survives replay.
    let (id, ev) = next_event(&mut r, "record").await.unwrap();
    assert_eq!(ev["op"], "delete");
    assert!(id.is_some());
    // Live events for other records stay out of a per-record topic.
    app.create_record(&su, "tasks", mk("d", false).as_bytes())
        .await
        .unwrap();
    assert!(next_event(&mut r, "record").await.is_none());

    // Relational or bad filters and bad ids are refused up front.
    let bad = TopicSpec {
        collection: "tasks".into(),
        filter: "nope = 1".into(),
        ..Default::default()
    };
    assert!(app.subscribe_with(&su, &[bad], None).is_err());
    let bad_id = TopicSpec {
        collection: "tasks".into(),
        ids: vec!["!!".into()],
        ..Default::default()
    };
    assert!(app.subscribe_with(&su, &[bad_id], None).is_err());
    reg.close_realtime();
}
