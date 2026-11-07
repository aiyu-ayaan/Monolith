use std::sync::{Arc, Mutex};
use std::time::Duration;

use monolith_auth::hmac_sha256_hex;
use monolith_core::{CollectionDef, Principal, Registry, WebhookIn};
use serde_json::{Value as J, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

type Seen = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

/// Minimal HTTP receiver: stores (headers, body) of each request and answers `status`.
async fn receiver(status: u16) -> (String, Seen) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", l.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let s2 = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut c, _)) = l.accept().await else {
                return;
            };
            let s = s2.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    let n = c.read(&mut tmp).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                        continue;
                    };
                    let head = String::from_utf8_lossy(&buf[..end]).to_string();
                    let len = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if buf.len() >= end + 4 + len {
                        s.lock()
                            .unwrap()
                            .push((head, buf[end + 4..end + 4 + len].to_vec()));
                        let _ = c
                            .write_all(
                                format!("HTTP/1.1 {status} X\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                                    .as_bytes(),
                            )
                            .await;
                        return;
                    }
                }
            });
        }
    });
    (url, seen)
}

async fn wait_for(seen: &Seen, n: usize) {
    for _ in 0..100 {
        if seen.lock().unwrap().len() >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "expected {n} deliveries, got {}",
        seen.lock().unwrap().len()
    );
}

fn hook(url: &str, events: &[&str]) -> WebhookIn {
    WebhookIn {
        name: "h".into(),
        url: url.into(),
        events: events.iter().map(|s| (*s).to_owned()).collect(),
        enabled: true,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn signed_delivery_matching_hidden_fields_ping_and_validation() {
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::open(dir.path()).await.unwrap();
    reg.create_app("wh").await.unwrap();
    let app = reg.app("wh").await.unwrap();
    let su = Principal::superuser();
    for d in [
        json!({"name":"users","fields":[],"rules":{"list":"","view":""}}),
        json!({"name":"posts","fields":[
            {"name":"title","type":"text"},
            {"name":"secret","type":"text","hidden":true}]}),
        json!({"name":"other","fields":[{"name":"n","type":"number"}]}),
    ] {
        app.put_collection(serde_json::from_value::<CollectionDef>(d).unwrap(), true)
            .await
            .unwrap();
    }
    let (url, seen) = receiver(200).await;

    // Bad input is refused field by field.
    assert!(
        reg.create_webhook("wh", hook("ftp://x", &["*"]))
            .await
            .is_err()
    );
    assert!(
        reg.create_webhook("wh", hook(&url, &["posts.bogus"]))
            .await
            .is_err()
    );
    assert!(reg.create_webhook("wh", hook(&url, &[])).await.is_err());

    let (info, secret) = reg
        .create_webhook("wh", hook(&url, &["posts.create"]))
        .await
        .unwrap();
    assert!(secret.starts_with("whsec_"));
    assert!(secret.starts_with(&info.secret_hint));

    // Only the matching event is delivered; the hidden field stays out.
    app.create_record(&su, "other", br#"{"n":1}"#)
        .await
        .unwrap();
    app.create_record(&su, "posts", br#"{"title":"hi","secret":"s3"}"#)
        .await
        .unwrap();
    wait_for(&seen, 1).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "non-matching change must not be delivered"
    );
    let (head, body) = seen.lock().unwrap()[0].clone();
    let j: J = serde_json::from_slice(&body).unwrap();
    assert_eq!(j["event"], "posts.create");
    assert_eq!(j["record"]["title"], "hi");
    assert!(j["record"].get("secret").is_none());
    assert!(j["seq"].as_u64().unwrap() > 0);

    // Signature: t=<secs>,v1=hmac(secret, "<t>.<body>").
    let sig = head
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("x-monolith-signature: ")
                .map(str::to_owned)
        })
        .unwrap();
    let (t, v1) = sig.split_once(",v1=").unwrap();
    let t = t.strip_prefix("t=").unwrap();
    let mut signed = format!("{t}.").into_bytes();
    signed.extend_from_slice(&body);
    assert_eq!(v1, hmac_sha256_hex(secret.as_bytes(), &signed));

    // Delivery log and ping.
    reg.test_webhook("wh", &info.id).await.unwrap();
    wait_for(&seen, 2).await;
    let ping: J = serde_json::from_slice(&seen.lock().unwrap()[1].1).unwrap();
    assert_eq!(ping["event"], "ping");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let log = reg
        .list_deliveries("wh", &info.id, Some("ok"), 10)
        .await
        .unwrap();
    assert_eq!(log.len(), 2);
    assert!(
        log.iter()
            .all(|d| d.last_status == Some(200) && d.attempts == 1)
    );

    // Disabling stops deliveries; deleting removes the hook and its log.
    let mut off = hook(&url, &["*"]);
    off.enabled = false;
    reg.update_webhook("wh", &info.id, off).await.unwrap();
    app.create_record(&su, "posts", br#"{"title":"later"}"#)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(seen.lock().unwrap().len(), 2);
    reg.delete_webhook("wh", &info.id).await.unwrap();
    assert!(reg.list_webhooks("wh").await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_delivery_stays_pending_with_backoff_and_retry_resets_it() {
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::open(dir.path()).await.unwrap();
    reg.create_app("wf").await.unwrap();
    let app = reg.app("wf").await.unwrap();
    app.put_collection(
        serde_json::from_value::<CollectionDef>(
            json!({"name":"t","fields":[{"name":"n","type":"number"}]}),
        )
        .unwrap(),
        true,
    )
    .await
    .unwrap();
    let (url, seen) = receiver(500).await;
    let (info, _) = reg.create_webhook("wf", hook(&url, &["*"])).await.unwrap();
    app.create_record(&Principal::superuser(), "t", br#"{"n":1}"#)
        .await
        .unwrap();
    wait_for(&seen, 1).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let log = reg
        .list_deliveries("wf", &info.id, Some("pending"), 10)
        .await
        .unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!((log[0].attempts, log[0].last_status), (1, Some(500)));
    assert!(
        log[0].next_attempt.is_some(),
        "next attempt is scheduled (backoff 5 s)"
    );
    assert_eq!(reg.list_webhooks("wf").await.unwrap()[0].pending, 1);

    // A manual retry puts it back in the queue at once.
    reg.retry_delivery("wf", &info.id, &log[0].id)
        .await
        .unwrap();
    wait_for(&seen, 2).await;
}
