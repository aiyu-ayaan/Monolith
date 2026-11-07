//! Runs `api/conformance/vectors/rules.json`: semantic rule and filter vectors ported from
//! PocketBase (see the file's `source`). Failing here means the rules engine diverges from
//! PocketBase behaviour that Monolith promises to keep (doc 06).

use std::collections::{BTreeSet, HashMap};

use monolith_core::{Auth, CollectionDef, CoreError, ListParams, Principal, Registry, WriteOpts};
use monolith_store::id::{encode_string, new_id};
use serde_json::{Value as J, json};

fn load() -> J {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../api/conformance/vectors/rules.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn def(v: &J) -> CollectionDef {
    serde_json::from_value(v.clone()).unwrap()
}

/// Replaces `$alice`-style placeholders in every string of `v`.
fn sub(v: &J, ids: &HashMap<String, String>) -> J {
    match v {
        J::String(s) => {
            let mut s = s.clone();
            for (name, id) in ids {
                s = s.replace(&format!("${name}"), id);
            }
            J::String(s)
        }
        J::Array(a) => J::Array(a.iter().map(|x| sub(x, ids)).collect()),
        J::Object(o) => J::Object(o.iter().map(|(k, x)| (k.clone(), sub(x, ids))).collect()),
        _ => v.clone(),
    }
}

fn titles(out: &[u8]) -> BTreeSet<String> {
    let v: J = serde_json::from_slice(out).unwrap();
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["title"].as_str().unwrap().to_owned())
        .collect()
}

fn want(v: &J) -> BTreeSet<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap().to_owned())
        .collect()
}

struct Env {
    _dir: tempfile::TempDir,
    app: std::sync::Arc<monolith_core::app::App>,
    ids: HashMap<String, String>,
    who: HashMap<String, Principal>,
}

async fn env(v: &J) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::open(dir.path()).await.unwrap();
    reg.create_app("v").await.unwrap();
    let app = reg.app("v").await.unwrap();
    let su = Principal::superuser();
    app.put_collection(
        def(&json!({"name":"users","fields":[{"name":"name","type":"text"}],"rules":{"list":"","view":""}})),
        true,
    )
    .await
    .unwrap();
    let (mut ids, mut who) = (HashMap::new(), HashMap::new());
    who.insert("su".to_owned(), su.clone());
    who.insert("anon".to_owned(), Principal::default());
    for name in v["users"].as_array().unwrap() {
        let name = name.as_str().unwrap();
        let raw = new_id();
        let id = encode_string(&raw);
        app.create_record(
            &su,
            "users",
            json!({"id": id, "name": name}).to_string().as_bytes(),
        )
        .await
        .unwrap();
        who.insert(
            name.to_owned(),
            Principal {
                superuser: false,
                auth: Some(Auth {
                    id: raw,
                    collection: "users".into(),
                    claims: vec![],
                }),
            },
        );
        ids.insert(name.to_owned(), id);
    }
    Env {
        _dir: dir,
        app,
        ids,
        who,
    }
}

async fn seed(e: &Env, group: &J) {
    let su = Principal::superuser();
    e.app
        .put_collection(def(&group["collection"]), true)
        .await
        .unwrap();
    let name = group["collection"]["name"].as_str().unwrap();
    for r in group["records"].as_array().unwrap() {
        let body = sub(r, &e.ids);
        e.app
            .create_record(&su, name, body.to_string().as_bytes())
            .await
            .unwrap_or_else(|err| panic!("seed {body}: {err}"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn pocketbase_filter_vectors() {
    let v = load();
    let e = env(&v).await;
    let items = &v["items"];
    seed(&e, items).await;
    let su = &e.who["su"];
    let mut failures = Vec::new();
    for c in items["filters"].as_array().unwrap() {
        let filter = sub(&c["filter"], &e.ids);
        let lp = ListParams {
            filter: filter.as_str().unwrap().into(),
            limit: Some(1000),
            ..Default::default()
        };
        // Anonymous with a public list rule uses the client-filter mode (no @request access).
        for who in ["anon", "su"] {
            match e.app.list_records(&e.who[who], "items", &lp).await {
                Ok(out) if titles(&out) == want(&c["expect"]) => {}
                Ok(out) => failures.push(format!(
                    "[{who}] {}: `{filter}` got {:?}, want {:?}",
                    c["name"],
                    titles(&out),
                    want(&c["expect"])
                )),
                Err(err) => failures.push(format!("[{who}] {}: `{filter}` error {err}", c["name"])),
            }
        }
    }
    for f in items["invalid_filters"].as_array().unwrap() {
        let lp = ListParams {
            filter: f.as_str().unwrap().into(),
            ..Default::default()
        };
        if !matches!(
            e.app.list_records(su, "items", &lp).await,
            Err(CoreError::BadRequest(_))
        ) {
            failures.push(format!("invalid filter accepted: `{f}`"));
        }
    }
    for c in items["known_gaps"].as_array().unwrap() {
        let lp = ListParams {
            filter: sub(&c["filter"], &e.ids).as_str().unwrap().into(),
            ..Default::default()
        };
        if !matches!(
            e.app.list_records(su, "items", &lp).await,
            Err(CoreError::BadRequest(_))
        ) {
            failures.push(format!(
                "known gap now works, move it to 'filters': {}",
                c["name"]
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn pocketbase_rule_vectors() {
    let v = load();
    let e = env(&v).await;
    let docs = &v["docs"];
    seed(&e, docs).await;
    let su = Principal::superuser();
    let base = def(&docs["collection"]);
    let mut failures = Vec::new();

    for c in docs["list_rules"].as_array().unwrap() {
        let mut d = base.clone();
        d.rules.list = c["rule"].as_str().map(str::to_owned);
        e.app.put_collection(d, false).await.unwrap();
        for (who, expect) in c["expect"].as_object().unwrap() {
            let got = e
                .app
                .list_records(
                    &e.who[who],
                    "docs",
                    &ListParams {
                        limit: Some(1000),
                        ..Default::default()
                    },
                )
                .await;
            let ok = match (&got, expect) {
                (Ok(out), J::Array(_)) => titles(out) == want(expect),
                (Err(CoreError::Forbidden(_)), J::String(s)) => s == "forbidden",
                _ => false,
            };
            if !ok {
                failures.push(format!(
                    "[{who}] list `{}`: got {:?}, want {expect}",
                    c["rule"],
                    got.map(|o| titles(&o))
                ));
            }
        }
    }

    for c in docs["write_rules"].as_array().unwrap() {
        let mut d = base.clone();
        let rule = c["rule"].as_str().map(str::to_owned);
        let op = c["op"].as_str().unwrap();
        // The target record must stay visible to the actor; only the tested rule varies.
        d.rules.view = Some(String::new());
        match op {
            "create" => d.rules.create = rule,
            "update" => d.rules.update = rule,
            _ => d.rules.delete = rule,
        }
        e.app.put_collection(d, false).await.unwrap();
        let p = &e.who[c["as"].as_str().unwrap()];
        let opts = WriteOpts {
            dry_run: true,
            ..Default::default()
        };
        let body = sub(&c["body"], &e.ids).to_string();
        let id = match c["record"].as_str() {
            Some(t) => {
                let all = e
                    .app
                    .list_records(
                        &su,
                        "docs",
                        &ListParams {
                            limit: Some(100),
                            ..Default::default()
                        },
                    )
                    .await
                    .unwrap();
                let all: J = serde_json::from_slice(&all).unwrap();
                all["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["title"] == t)
                    .map(|r| r["id"].as_str().unwrap().to_owned())
                    .unwrap_or_default()
            }
            None => String::new(),
        };
        let got = match op {
            "create" => e
                .app
                .create_record_with(p, "docs", body.as_bytes(), &opts)
                .await
                .map(drop),
            "update" => e
                .app
                .update_record_with(p, "docs", &id, body.as_bytes(), &opts)
                .await
                .map(drop),
            _ => e.app.delete_record_with(p, "docs", &id, &opts).await,
        };
        let got = match got {
            Ok(()) => "ok",
            Err(CoreError::Forbidden(_)) => "forbidden",
            Err(CoreError::NotFound(_)) => "not_found",
            Err(CoreError::BadRequest(_)) => "bad_request",
            Err(_) => "other",
        };
        if got != c["expect"].as_str().unwrap() {
            failures.push(format!(
                "write {}: got {got}, want {}",
                c["name"], c["expect"]
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
