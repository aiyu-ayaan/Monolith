use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, header};
use axum::response::Response;
use axum::routing::get;
use futures_util::StreamExt;
use futures_util::stream;
use monolith_client::{
    ChangeType, Client, Error, RealtimeAction, Record, SnapshotOptions, SubscribeOptions,
};
use serde_json::json;
use tokio::net::TcpListener;

type Requests = Arc<Mutex<Vec<HashMap<String, String>>>>;

#[derive(Clone)]
struct Fake {
    bodies: Arc<Mutex<Vec<String>>>,
    requests: Requests,
    auth: Arc<Mutex<Option<String>>>,
    /// End the response after its body instead of holding it open.
    end: bool,
}

async fn sse(
    State(s): State<Fake>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    s.requests.lock().unwrap().push(q);
    *s.auth.lock().unwrap() = headers
        .get(header::AUTHORIZATION)
        .map(|v| v.to_str().unwrap().to_string());
    let body = {
        let mut b = s.bodies.lock().unwrap();
        if b.is_empty() {
            String::new()
        } else {
            b.remove(0)
        }
    };
    let first = stream::iter([Ok::<_, Infallible>(Bytes::from(body.clone()))]);
    let body = if s.end && !body.is_empty() {
        Body::from_stream(first)
    } else {
        Body::from_stream(first.chain(stream::pending()))
    };
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(body)
        .unwrap()
}

struct Harness {
    client: Client,
    requests: Requests,
    auth: Arc<Mutex<Option<String>>>,
}

async fn harness(bodies: Vec<String>, end: bool) -> Harness {
    let fake = Fake {
        bodies: Arc::new(Mutex::new(bodies)),
        requests: Arc::default(),
        auth: Arc::default(),
        end,
    };
    let requests = fake.requests.clone();
    let auth = fake.auth.clone();
    let router = Router::new()
        .route("/api/v1/apps/app/realtime", get(sse))
        .with_state(fake);
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = Client::builder(url)
        .app("app")
        .token("tok")
        .realtime_backoff(Duration::from_millis(10), Duration::from_millis(50))
        .build()
        .unwrap();
    Harness {
        client,
        requests,
        auth,
    }
}

fn rec(seq: u64, op: &str, record: serde_json::Value) -> String {
    let data = json!({"op": op, "collection": "tasks", "seq": seq, "record": record});
    format!("id: {seq}\nevent: record\ndata: {data}\n\n")
}

fn snapshot(seq: u64, items: serde_json::Value) -> String {
    let data = json!({"collection": "tasks", "seq": seq, "items": items, "next": null});
    format!("event: snapshot\ndata: {data}\n\n")
}

async fn next<S: futures_util::Stream + Unpin>(s: &mut S) -> S::Item {
    tokio::time::timeout(Duration::from_secs(3), s.next())
        .await
        .expect("timed out")
        .expect("stream ended")
}

fn ids(docs: &[Record]) -> Vec<&str> {
    docs.iter().map(|d| d.id.as_str()).collect()
}

#[tokio::test]
async fn subscribe_maps_events_and_sends_topic_params() {
    let h = harness(vec![rec(5, "leave", json!({"id": "a"}))], false).await;
    let mut sub = h.client.records("tasks").subscribe(
        SubscribeOptions::new()
            .filter("done = false")
            .ids(["a", "b"]),
    );
    let ev = next(&mut sub).await.unwrap();
    assert_eq!(ev.action, RealtimeAction::Leave);
    assert_eq!(ev.record.id, "a");
    assert_eq!((ev.collection.as_str(), ev.seq), ("tasks", Some(5)));
    let q = h.requests.lock().unwrap()[0].clone();
    assert_eq!(q["collections"], "tasks");
    assert_eq!(q["filter.tasks"], "done = false");
    assert_eq!(q["ids.tasks"], "a,b");
    assert_eq!(h.auth.lock().unwrap().as_deref(), Some("Bearer tok"));
}

#[tokio::test]
async fn subscribe_resumes_with_since_after_stream_ends() {
    let h = harness(
        vec![rec(9, "create", json!({"id": "z"})), String::new()],
        true,
    )
    .await;
    let mut sub = h.client.records("tasks").subscribe(SubscribeOptions::new());
    assert_eq!(next(&mut sub).await.unwrap().seq, Some(9));
    for _ in 0..200 {
        if h.requests.lock().unwrap().len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let reqs = h.requests.lock().unwrap().clone();
    assert!(reqs.len() >= 2);
    assert!(!reqs[0].contains_key("since"));
    assert_eq!(reqs[1]["since"], "9");
}

#[tokio::test]
async fn resync_reconnects_without_since() {
    let body =
        rec(9, "create", json!({"id": "z"})) + "event: resync\ndata: {\"reason\":\"since\"}\n\n";
    let h = harness(vec![body, rec(1, "create", json!({"id": "y"}))], false).await;
    let mut sub = h.client.records("tasks").subscribe(SubscribeOptions::new());
    assert_eq!(next(&mut sub).await.unwrap().record.id, "z");
    assert_eq!(next(&mut sub).await.unwrap().record.id, "y");
    let reqs = h.requests.lock().unwrap().clone();
    assert!(!reqs[1].contains_key("since"));
}

#[tokio::test]
async fn on_snapshot_merges_snapshot_updates_leave_and_delete() {
    let body = snapshot(3, json!([{"id": "a", "n": 2}, {"id": "b", "n": 1}]))
        + &rec(4, "create", json!({"id": "c", "n": 0}))
        + &rec(5, "leave", json!({"id": "a"}))
        + &rec(6, "delete", json!({"id": "b"}));
    let h = harness(vec![body], false).await;
    let mut sub = h
        .client
        .records("tasks")
        .on_snapshot(SnapshotOptions::new().filter("n >= 0").sort("n").limit(5));
    let s1 = next(&mut sub).await.unwrap();
    let s2 = next(&mut sub).await.unwrap();
    let s3 = next(&mut sub).await.unwrap();
    let s4 = next(&mut sub).await.unwrap();
    assert_eq!(ids(&s1.docs), ["b", "a"]);
    assert_eq!(ids(&s2.docs), ["c", "b", "a"]);
    assert_eq!(ids(&s3.docs), ["c", "b"]);
    assert_eq!(ids(&s4.docs), ["c"]);
    assert_eq!(s2.changes.len(), 1);
    assert_eq!(s2.changes[0].change_type, ChangeType::Added);
    assert_eq!(s2.changes[0].doc.id, "c");
    assert_eq!(s3.changes[0].change_type, ChangeType::Removed);
    let q = h.requests.lock().unwrap()[0].clone();
    assert_eq!(q["snapshot"], "5");
    assert_eq!(q["sort.tasks"], "n");
    assert_eq!(q["filter.tasks"], "n >= 0");
}

#[tokio::test]
async fn on_snapshot_resnapshots_on_reconnect() {
    let h = harness(
        vec![
            snapshot(1, json!([{"id": "a"}, {"id": "b"}])),
            snapshot(2, json!([{"id": "b"}, {"id": "c"}])),
        ],
        true,
    )
    .await;
    let mut sub = h
        .client
        .records("tasks")
        .on_snapshot(SnapshotOptions::new());
    next(&mut sub).await.unwrap();
    let s2 = next(&mut sub).await.unwrap();
    assert!(!h.requests.lock().unwrap()[1].contains_key("since"));
    let mut got: Vec<_> = s2
        .changes
        .iter()
        .map(|c| (c.change_type, c.doc.id.clone()))
        .collect();
    got.sort_by(|a, b| a.1.cmp(&b.1));
    assert_eq!(
        got,
        [
            (ChangeType::Removed, "a".to_string()),
            (ChangeType::Modified, "b".to_string()),
            (ChangeType::Added, "c".to_string()),
        ]
    );
}

#[tokio::test]
async fn on_doc_snapshot_filters_by_id_and_yields_none_after_delete() {
    let body = snapshot(1, json!([{"id": "x", "t": 1}])) + &rec(2, "delete", json!({"id": "x"}));
    let h = harness(vec![body], false).await;
    let mut sub = h.client.records("tasks").on_doc_snapshot("x'y");
    let first = next(&mut sub).await.unwrap();
    assert_eq!(first.unwrap().id, "x");
    assert!(next(&mut sub).await.unwrap().is_none());
    let q = h.requests.lock().unwrap()[0].clone();
    assert_eq!(q["filter.tasks"], "id = 'xy'");
    assert_eq!(q["snapshot"], "1");
}

#[tokio::test]
async fn server_error_event_becomes_err() {
    let h = harness(
        vec!["event: error\ndata: {\"message\":\"boom\"}\n\n".into()],
        false,
    )
    .await;
    let mut sub = h
        .client
        .records("tasks")
        .on_snapshot(SnapshotOptions::new());
    match next(&mut sub).await {
        Err(Error::Realtime(m)) => assert_eq!(m, "boom"),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn dropping_subscription_stops_reconnecting() {
    let h = harness(vec![], true).await;
    let sub = h.client.records("tasks").subscribe(SubscribeOptions::new());
    for _ in 0..200 {
        if !h.requests.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    drop(sub);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let n = h.requests.lock().unwrap().len();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(h.requests.lock().unwrap().len(), n);
}
