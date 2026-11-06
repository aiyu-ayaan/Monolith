use monolith_core::{
    BatchItem, BatchKind, CollectionDef, CoreError, ListParams, Principal, Registry,
};
use monolith_rules::Auth;
use monolith_store::id::{encode_string, new_id};
use serde_json::{Value as J, json};

fn def(v: J) -> CollectionDef {
    serde_json::from_value(v).unwrap()
}

fn parse(b: &[u8]) -> J {
    serde_json::from_slice(b).unwrap()
}

async fn setup() -> (tempfile::TempDir, Registry) {
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::open(dir.path()).await.unwrap();
    reg.create_app("demo").await.unwrap();
    let app = reg.app("demo").await.unwrap();
    app.put_collection(
        def(
            json!({"name":"users","fields":[{"name":"name","type":"text","required":true}],
                   "rules":{"list":"","view":""}}),
        ),
        true,
    )
    .await
    .unwrap();
    app.put_collection(
        def(json!({
            "name":"tasks",
            "fields":[
                {"name":"title","type":"text","required":true,"max":20},
                {"name":"status","type":"select","values":["open","done"]},
                {"name":"priority","type":"number","onlyInt":true},
                {"name":"owner","type":"relation","collection":"users","onDelete":"cascade"},
                {"name":"secret","type":"text","hidden":true},
                {"name":"meta","type":"json"}
            ],
            "rules":{
                "list":"status = 'open' || owner = @request.auth.id",
                "view":"",
                "create":"title !~ 'forbidden'",
                "update":"owner = @request.auth.id && @request.body.title != 'blocked'",
                "delete":null
            }
        })),
        true,
    )
    .await
    .unwrap();
    (dir, reg)
}

fn su() -> Principal {
    Principal::superuser()
}

fn anon() -> Principal {
    Principal::default()
}

fn user(id: [u8; 16]) -> Principal {
    Principal {
        superuser: false,
        auth: Some(Auth {
            id,
            collection: "users".into(),
            claims: vec![],
        }),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn superusers_and_apps() {
    let (dir, reg) = setup().await;
    assert_eq!(reg.superuser_count().await.unwrap(), 0);
    reg.create_superuser("Admin@Example.com", "long enough pw")
        .await
        .unwrap();
    assert!(matches!(
        reg.create_superuser("admin@example.com", "long enough pw")
            .await,
        Err(CoreError::Conflict(_))
    ));
    assert!(matches!(
        reg.create_superuser("x@y.z", "short").await,
        Err(CoreError::Validation(_))
    ));
    let (token, _) = reg
        .login("admin@example.com", "long enough pw")
        .await
        .unwrap();
    assert!(reg.verify_superuser(&token).is_some());
    assert!(reg.verify_superuser(&format!("{token}0")).is_none());
    assert!(matches!(
        reg.login("admin@example.com", "wrong password").await,
        Err(CoreError::Unauthorized)
    ));
    assert!(matches!(
        reg.login("nobody@example.com", "long enough pw").await,
        Err(CoreError::Unauthorized)
    ));

    assert!(matches!(
        reg.create_app("demo").await,
        Err(CoreError::Conflict(_))
    ));
    assert!(matches!(
        reg.create_app("Bad Name").await,
        Err(CoreError::BadRequest(_))
    ));
    assert_eq!(reg.list_apps().await.unwrap().len(), 1);

    // Schema and token key survive a restart.
    let app = reg.app("demo").await.unwrap();
    let created = app
        .create_record(&su(), "users", br#"{"name":"ann"}"#)
        .await
        .unwrap();
    drop(app);
    drop(reg);
    let reg = Registry::open(dir.path()).await.unwrap();
    assert!(reg.verify_superuser(&token).is_some());
    let app = reg.app("demo").await.unwrap();
    assert_eq!(app.schema().version, 2);
    let id = parse(&created)["id"].as_str().unwrap().to_owned();
    assert_eq!(
        parse(&app.view_record(&anon(), "users", &id).await.unwrap())["name"],
        "ann"
    );

    reg.delete_app("demo").await.unwrap();
    assert!(matches!(reg.app("demo").await, Err(CoreError::NotFound(_))));
    assert!(!dir.path().join("apps/demo").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn crud_with_rules() {
    let (_dir, reg) = setup().await;
    let app = reg.app("demo").await.unwrap();
    let ann = parse(
        &app.create_record(&su(), "users", br#"{"name":"ann"}"#)
            .await
            .unwrap(),
    );
    let ann_id = ann["id"].as_str().unwrap();
    let ann_raw = monolith_store::id::decode(ann_id).unwrap();

    // Anonymous create passes the create rule; system fields and _v are filled in.
    let body = json!({"title":"Ride","status":"open","priority":2,"owner":ann_id,"secret":"s","meta":{"k":[1,2]}});
    let t = parse(
        &app.create_record(&anon(), "tasks", body.to_string().as_bytes())
            .await
            .unwrap(),
    );
    assert_eq!(t["title"], "Ride");
    assert_eq!(t["meta"], json!({"k":[1,2]}));
    assert!(t["_v"].as_i64().unwrap() > 0);
    assert!(t.get("secret").is_none(), "hidden from non-superusers");
    assert!(t["created"].as_str().unwrap().ends_with('Z'));
    let tid = t["id"].as_str().unwrap().to_owned();

    let full = parse(&app.view_record(&su(), "tasks", &tid).await.unwrap());
    assert_eq!(full["secret"], "s");

    // Create rule rejection.
    let r = app
        .create_record(&anon(), "tasks", br#"{"title":"forbidden"}"#)
        .await;
    assert!(matches!(r, Err(CoreError::Forbidden(_))));

    // Validation: required, too long, select option, type, unknown, read-only.
    let r = app
        .create_record(
            &anon(),
            "tasks",
            br#"{"title":"x","status":"nope","priority":1.5,"bogus":1,"created":"x"}"#,
        )
        .await;
    let Err(CoreError::Validation(errs)) = r else {
        panic!("{r:?}")
    };
    let codes: Vec<_> = errs.iter().map(|e| (e.field.as_str(), e.code)).collect();
    assert!(codes.contains(&("status", "invalid_option")));
    assert!(codes.contains(&("priority", "type")));
    assert!(codes.contains(&("bogus", "unknown_field")));
    assert!(codes.contains(&("created", "read_only")));
    let Err(CoreError::Validation(errs)) = app
        .create_record(&anon(), "tasks", br#"{"title":"aaaaaaaaaaaaaaaaaaaaa"}"#)
        .await
    else {
        panic!()
    };
    assert_eq!(errs[0].code, "too_long");
    assert!(matches!(
        app.create_record(&anon(), "tasks", b"{}").await,
        Err(CoreError::Validation(_))
    ));
    assert!(matches!(
        app.create_record(&anon(), "tasks", b"[1]").await,
        Err(CoreError::BadRequest(_))
    ));

    // Update rule: only the owner, and @request.body is checked.
    let patch = br#"{"title":"Ride 2"}"#;
    assert!(matches!(
        app.update_record(&anon(), "tasks", &tid, patch).await,
        Err(CoreError::NotFound(_))
    ));
    let u = parse(
        &app.update_record(&user(ann_raw), "tasks", &tid, patch)
            .await
            .unwrap(),
    );
    assert_eq!(u["title"], "Ride 2");
    assert_eq!(u["priority"], 2, "unsent fields keep their value");
    assert!(u["_v"].as_i64() > t["_v"].as_i64());
    let r = app
        .update_record(&user(ann_raw), "tasks", &tid, br#"{"title":"blocked"}"#)
        .await;
    assert!(matches!(r, Err(CoreError::NotFound(_))));
    let r = app
        .update_record(&user(ann_raw), "tasks", &tid, br#"{"title":null}"#)
        .await;
    assert!(
        matches!(r, Err(CoreError::Validation(_))),
        "required field cannot be cleared"
    );

    // Delete rule is null: superusers only.
    assert!(matches!(
        app.delete_record(&user(ann_raw), "tasks", &tid).await,
        Err(CoreError::Forbidden(_))
    ));
    // Cascade: deleting the owner deletes the task.
    app.delete_record(&su(), "users", ann_id).await.unwrap();
    assert!(matches!(
        app.view_record(&su(), "tasks", &tid).await,
        Err(CoreError::NotFound(_))
    ));
    assert!(matches!(
        app.view_record(&su(), "tasks", "garbage").await,
        Err(CoreError::NotFound(_))
    ));

    // Client-generated id.
    let id = encode_string(&new_id());
    let r = parse(
        &app.create_record(
            &su(),
            "users",
            format!(r#"{{"id":"{id}","name":"bo"}}"#).as_bytes(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(r["id"], id);
    let dup = app
        .create_record(
            &su(),
            "users",
            format!(r#"{{"id":"{id}","name":"bo"}}"#).as_bytes(),
        )
        .await;
    assert!(matches!(dup, Err(CoreError::Conflict(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn keyset_pages_match_full_order() {
    let (_dir, reg) = setup().await;
    let app = reg.app("demo").await.unwrap();
    for i in 0..23 {
        let prio = if i % 4 == 0 { J::Null } else { json!(i % 3) };
        let body = json!({"title": format!("t{}", i % 5), "status": if i % 6 == 0 { "done" } else { "open" }, "priority": prio});
        app.create_record(&su(), "tasks", body.to_string().as_bytes())
            .await
            .unwrap();
    }
    for (sort, who) in [
        ("priority,-title", su()),
        ("-priority,title", anon()),
        ("-created", anon()),
        ("", su()),
    ] {
        let all = parse(
            &app.list_records(
                &who,
                "tasks",
                &ListParams {
                    sort: sort.into(),
                    limit: Some(1000),
                    ..Default::default()
                },
            )
            .await
            .unwrap(),
        );
        assert!(all["next"].is_null());
        let want: Vec<J> = all["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].clone())
            .collect();
        let mut got = Vec::new();
        let mut cursor = None;
        loop {
            let lp = ListParams {
                sort: sort.into(),
                limit: Some(4),
                cursor: cursor.clone(),
                ..Default::default()
            };
            let page = parse(&app.list_records(&who, "tasks", &lp).await.unwrap());
            got.extend(
                page["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| r["id"].clone()),
            );
            match page["next"].as_str() {
                Some(c) => cursor = Some(c.to_owned()),
                None => break,
            }
        }
        assert_eq!(got, want, "sort {sort:?}");
    }
    // Anonymous list rule hides done tasks; filter and projection work.
    let lp = ListParams {
        filter: "priority >= 1".into(),
        fields: Some("title".into()),
        limit: Some(1000),
        ..Default::default()
    };
    let page = parse(&app.list_records(&anon(), "tasks", &lp).await.unwrap());
    let items = page["items"].as_array().unwrap();
    assert!(!items.is_empty());
    assert!(
        items
            .iter()
            .all(|r| r.as_object().unwrap().keys().len() == 2 && r.get("title").is_some())
    );

    let bad = |lp: ListParams| {
        let app = app.clone();
        async move { app.list_records(&anon(), "tasks", &lp).await }
    };
    assert!(matches!(
        bad(ListParams {
            filter: "secret = 'x'".into(),
            ..Default::default()
        })
        .await,
        Err(CoreError::BadRequest(_))
    ));
    assert!(matches!(
        bad(ListParams {
            sort: "secret".into(),
            ..Default::default()
        })
        .await,
        Err(CoreError::BadRequest(_))
    ));
    assert!(matches!(
        bad(ListParams {
            limit: Some(0),
            ..Default::default()
        })
        .await,
        Err(CoreError::BadRequest(_))
    ));
    assert!(matches!(
        bad(ListParams {
            cursor: Some("00".into()),
            ..Default::default()
        })
        .await,
        Err(CoreError::BadRequest(_))
    ));
    // A cursor from one sort cannot be replayed with another.
    let p = parse(
        &app.list_records(
            &su(),
            "tasks",
            &ListParams {
                limit: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap(),
    );
    let c = p["next"].as_str().unwrap().to_owned();
    let r = app
        .list_records(
            &su(),
            "tasks",
            &ListParams {
                sort: "-created".into(),
                cursor: Some(c),
                ..Default::default()
            },
        )
        .await;
    assert!(matches!(r, Err(CoreError::BadRequest(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn batch_is_atomic() {
    let (_dir, reg) = setup().await;
    let app = reg.app("demo").await.unwrap();
    let missing = encode_string(&new_id());
    let good = br#"{"title":"a"}"#;
    let bad_fk = format!(r#"{{"title":"b","owner":"{missing}"}}"#);
    let items = [
        BatchItem {
            kind: BatchKind::Create,
            collection: "tasks",
            id: None,
            body: Some(good),
        },
        BatchItem {
            kind: BatchKind::Create,
            collection: "tasks",
            id: None,
            body: Some(bad_fk.as_bytes()),
        },
    ];
    assert!(matches!(
        app.batch(&su(), &items).await,
        Err(CoreError::Conflict(_))
    ));
    let page = parse(
        &app.list_records(&su(), "tasks", &ListParams::default())
            .await
            .unwrap(),
    );
    assert!(
        page["items"].as_array().unwrap().is_empty(),
        "nothing committed"
    );

    let ids = app.batch(&su(), &items[..1]).await.unwrap();
    let id = encode_string(&ids[0]);
    let upd = [
        BatchItem {
            kind: BatchKind::Update,
            collection: "tasks",
            id: Some(&id),
            body: Some(br#"{"status":"done"}"#),
        },
        BatchItem {
            kind: BatchKind::Delete,
            collection: "tasks",
            id: Some(&id),
            body: None,
        },
    ];
    app.batch(&su(), &upd).await.unwrap();
    assert!(matches!(
        app.view_record(&su(), "tasks", &id).await,
        Err(CoreError::NotFound(_))
    ));
    let Err(CoreError::Validation(e)) = app
        .batch(
            &su(),
            &[BatchItem {
                kind: BatchKind::Create,
                collection: "tasks",
                id: None,
                body: Some(b"{}"),
            }],
        )
        .await
    else {
        panic!()
    };
    assert_eq!(e[0].field, "ops[0].title");
}

#[tokio::test(flavor = "multi_thread")]
async fn migrations() {
    let (_dir, reg) = setup().await;
    let app = reg.app("demo").await.unwrap();
    app.create_record(&su(), "users", br#"{"name":"ann"}"#)
        .await
        .unwrap();
    // Add a field + unique index, drop nothing.
    app.put_collection(
        def(json!({"name":"users","fields":[{"name":"name","type":"text","required":true},{"name":"email","type":"email"}],
                   "rules":{"list":"","view":""},"indexes":[{"fields":["email"],"unique":true}]})),
        false,
    )
    .await
    .unwrap();
    app.create_record(&su(), "users", br#"{"name":"bo","email":"bo@x.io"}"#)
        .await
        .unwrap();
    let dup = app
        .create_record(&su(), "users", br#"{"name":"cy","email":"bo@x.io"}"#)
        .await;
    assert!(matches!(dup, Err(CoreError::Conflict(_))));
    // Type change is refused, schema unchanged.
    let v = app.schema().version;
    let r = app
        .put_collection(
            def(json!({"name":"users","fields":[{"name":"name","type":"number"}]})),
            false,
        )
        .await;
    assert!(matches!(r, Err(CoreError::BadRequest(_))));
    assert_eq!(app.schema().version, v);
    // Bad rule is refused at save time.
    let r = app
        .put_collection(def(json!({"name":"users","fields":[{"name":"name","type":"text"}],"rules":{"list":"nope = 1"}})), false)
        .await;
    assert!(matches!(r, Err(CoreError::BadRequest(_))));
    // Referenced collection cannot be dropped; tasks first.
    assert!(matches!(
        app.delete_collection("users").await,
        Err(CoreError::Conflict(_))
    ));
    app.delete_collection("tasks").await.unwrap();
    app.delete_collection("users").await.unwrap();
    assert!(app.schema().collections.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn filter_conformance_vectors() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../api/conformance/vectors/filters.json"
    );
    let v: J = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let (_dir, reg) = setup().await;
    reg.create_app("vec").await.unwrap();
    let app = reg.app("vec").await.unwrap();
    app.put_collection(def(json!({"name":"users","fields":[]})), true)
        .await
        .unwrap();
    app.put_collection(
        serde_json::from_value(v["collection"].clone()).unwrap(),
        true,
    )
    .await
    .unwrap();
    let mut def = app.schema().get("tasks").unwrap().def.clone();
    def.rules.list = Some(String::new());
    app.put_collection(def, false).await.unwrap();
    for f in v["valid"].as_array().unwrap() {
        let lp = ListParams {
            filter: f.as_str().unwrap().into(),
            ..Default::default()
        };
        app.list_records(&anon(), "tasks", &lp)
            .await
            .unwrap_or_else(|e| panic!("{f}: {e}"));
    }
    for f in v["invalid"].as_array().unwrap() {
        let lp = ListParams {
            filter: f.as_str().unwrap().into(),
            ..Default::default()
        };
        assert!(
            matches!(
                app.list_records(&anon(), "tasks", &lp).await,
                Err(CoreError::BadRequest(_))
            ),
            "{f}"
        );
    }
}
