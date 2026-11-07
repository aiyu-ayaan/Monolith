use monolith_core::{
    BatchItem, BatchKind, CollectionDef, CoreError, ExplainQueryReq, ExplainRuleReq, ListParams,
    Principal, ReadOpts, Registry, TotalMode, WriteOpts,
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

#[tokio::test(flavor = "multi_thread")]
async fn expand_total_count_upsert_if_match_and_dry_run() {
    let (_dir, reg) = setup().await;
    let app = reg.app("demo").await.unwrap();
    // Add a nested relation (teams -> users -> tasks) to test multi-hop expand.
    app.put_collection(
        def(json!({
            "name":"teams",
            "fields":[{"name":"title","type":"text","required":true},{"name":"secret","type":"text","hidden":true}],
            "rules":{"list":"","view":"title != 'private'"}
        })),
        true,
    )
    .await
    .unwrap();
    app.put_collection(
        def(json!({
            "name":"users",
            "fields":[
                {"name":"name","type":"text","required":true},
                {"name":"team","type":"relation","collection":"teams"}
            ],
            "rules":{"list":"","view":""}
        })),
        false,
    )
    .await
    .unwrap();

    let team_pub = parse(
        &app.create_record(&su(), "teams", br#"{"title":"core","secret":"top"}"#)
            .await
            .unwrap(),
    );
    let team_priv = parse(
        &app.create_record(&su(), "teams", br#"{"title":"private","secret":"shh"}"#)
            .await
            .unwrap(),
    );
    let u1 = parse(
        &app.create_record(
            &su(),
            "users",
            json!({"name":"ann","team":team_pub["id"]})
                .to_string()
                .as_bytes(),
        )
        .await
        .unwrap(),
    );
    let u2 = parse(
        &app.create_record(
            &su(),
            "users",
            json!({"name":"bo","team":team_priv["id"]})
                .to_string()
                .as_bytes(),
        )
        .await
        .unwrap(),
    );
    let u1_raw = monolith_store::id::decode(u1["id"].as_str().unwrap()).unwrap();

    let t1 = app
        .create_record_with(
            &anon(),
            "tasks",
            json!({"title":"t1","status":"open","owner":u1["id"]})
                .to_string()
                .as_bytes(),
            &WriteOpts {
                expand: Some("owner.team".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let t1_json = parse(&t1.json);
    assert_eq!(t1_json["expand"]["owner"]["name"], "ann");
    assert_eq!(
        t1_json["expand"]["owner"]["expand"]["team"]["title"],
        "core"
    );
    assert!(
        t1_json["expand"]["owner"]["expand"]["team"]
            .get("secret")
            .is_none(),
        "hidden fields on expanded targets stay hidden from non-superusers"
    );

    // Target view rule hides the private team from anonymous callers during expand.
    let t2 = app
        .create_record_with(
            &anon(),
            "tasks",
            json!({"title":"t2","status":"open","owner":u2["id"]})
                .to_string()
                .as_bytes(),
            &WriteOpts {
                expand: Some("owner.team".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let t2_json = parse(&t2.json);
    assert_eq!(t2_json["expand"]["owner"]["name"], "bo");
    assert!(t2_json["expand"]["owner"].get("expand").is_none());

    // List with expand + total=exact|estimate, and count_records.
    let page = parse(
        &app.list_records(
            &anon(),
            "tasks",
            &ListParams {
                expand: Some("owner.team".into()),
                total: TotalMode::Exact,
                ..Default::default()
            },
        )
        .await
        .unwrap(),
    );
    assert_eq!(page["total"], 2);
    assert_eq!(page["items"][0]["expand"]["owner"]["name"], "ann");
    let (cnt, exact) = app
        .count_records(&anon(), "tasks", "title = 't1'", TotalMode::Exact)
        .await
        .unwrap();
    assert_eq!((cnt, exact), (1, true));

    // If-None-Match returns not_modified when _v matches.
    let tid = t1_json["id"].as_str().unwrap();
    let view_hit = app
        .view_record_with(
            &anon(),
            "tasks",
            tid,
            &ReadOpts {
                if_none_match: Some(t1.version),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(view_hit.not_modified);

    // If-Match optimistic concurrency: stale _v -> PreconditionFailed (412), missing/forbidden -> NotFound (404).
    let stale = app
        .update_record_with(
            &user(u1_raw),
            "tasks",
            tid,
            br#"{"title":"stale"}"#,
            &WriteOpts {
                if_match: Some(t1.version + 99),
                ..Default::default()
            },
        )
        .await;
    assert!(matches!(stale, Err(CoreError::PreconditionFailed(_))));
    let wrong_user = app
        .update_record_with(
            &anon(),
            "tasks",
            tid,
            br#"{"title":"stale"}"#,
            &WriteOpts {
                if_match: Some(t1.version + 99),
                ..Default::default()
            },
        )
        .await;
    assert!(
        matches!(wrong_user, Err(CoreError::NotFound(_))),
        "must not leak existence via 412 when update rule rejects"
    );

    // Dry-run write validates and returns updated row without committing.
    let dry = app
        .update_record_with(
            &user(u1_raw),
            "tasks",
            tid,
            br#"{"title":"dry"}"#,
            &WriteOpts {
                dry_run: true,
                if_match: Some(t1.version),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(parse(&dry.json)["title"], "dry");
    assert_eq!(
        parse(&app.view_record(&anon(), "tasks", tid).await.unwrap())["title"],
        "t1"
    );

    // Upsert: creates when new (created=true), updates when existing (created=false).
    let up_id = encode_string(&new_id());
    let created = app
        .upsert_record_with(
            &anon(),
            "tasks",
            &up_id,
            json!({"title":"up1","owner":u1["id"]})
                .to_string()
                .as_bytes(),
            &WriteOpts::default(),
        )
        .await
        .unwrap();
    assert!(created.created);
    let updated = app
        .upsert_record_with(
            &user(u1_raw),
            "tasks",
            &up_id,
            br#"{"title":"up2"}"#,
            &WriteOpts {
                if_match: Some(created.version),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(!updated.created);
    assert_eq!(parse(&updated.json)["title"], "up2");

    // Debug helpers: diff_collection, explain_rule, explain_query.
    let diff = app
        .diff_collection(def(json!({
            "name":"teams",
            "fields":[
                {"name":"title","type":"text","required":true},
                {"name":"secret","type":"text","hidden":true},
                {"name":"slug","type":"text"}
            ]
        })))
        .unwrap();
    assert_eq!(diff, vec!["ALTER TABLE \"teams\" ADD COLUMN \"slug\" TEXT"]);

    let exp = app
        .explain_rule(ExplainRuleReq {
            collection: "tasks".into(),
            op: "update".into(),
            expr: None,
            auth: Some(Auth {
                id: u1_raw,
                collection: "users".into(),
                claims: vec![],
            }),
            record: Some(json!({"owner": u1["id"]}).to_string().into_bytes()),
            body: Some(br#"{"title":"allowed"}"#.to_vec()),
        })
        .unwrap();
    assert_eq!(exp.class, "rowLocal");
    assert_eq!(exp.verdict, Some(true));

    let qplan = app
        .explain_query(ExplainQueryReq {
            collection: "tasks".into(),
            filter: "status = 'open'".into(),
            sort: "-priority".into(),
            superuser: false,
        })
        .await
        .unwrap();
    assert!(!qplan.plan.is_empty());
    assert!(qplan.uses_scan);
    assert!(!qplan.suggested_indexes.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn multi_relations_junction_filters_and_expand() {
    let (_dir, reg) = setup().await;
    let app = reg.app("demo").await.unwrap();
    app.put_collection(
        def(json!({
            "name":"tags",
            "fields":[
                {"name":"name","type":"text","required":true},
                {"name":"weight","type":"number","onlyInt":true}
            ],
            "rules":{"list":"","view":"name != 'hidden'"}
        })),
        true,
    )
    .await
    .unwrap();
    app.put_collection(
        def(json!({
            "name":"articles",
            "fields":[
                {"name":"title","type":"text","required":true},
                {"name":"tags","type":"relation","collection":"tags","maxSelect":3,"onDelete":"cascade"},
                {"name":"watchers","type":"relation","collection":"users","maxSelect":5,"onDelete":"restrict"}
            ],
            "rules":{"list":"","view":"","create":"","update":"","delete":""}
        })),
        true,
    )
    .await
    .unwrap();

    let t_rust = parse(
        &app.create_record(&su(), "tags", br#"{"name":"rust","weight":10}"#)
            .await
            .unwrap(),
    );
    let t_db = parse(
        &app.create_record(&su(), "tags", br#"{"name":"db","weight":20}"#)
            .await
            .unwrap(),
    );
    let t_web = parse(
        &app.create_record(&su(), "tags", br#"{"name":"web","weight":5}"#)
            .await
            .unwrap(),
    );
    let t_hid = parse(
        &app.create_record(&su(), "tags", br#"{"name":"hidden","weight":99}"#)
            .await
            .unwrap(),
    );
    let u1 = parse(
        &app.create_record(&su(), "users", br#"{"name":"ann"}"#)
            .await
            .unwrap(),
    );

    // Create with duplicate IDs deduplicates while preserving order; expand returns JSON array.
    let a1 = parse(
        &app.create_record_with(
            &anon(),
            "articles",
            json!({
                "title":"one",
                "tags":[t_rust["id"], t_db["id"], t_rust["id"], t_hid["id"]],
                "watchers":[u1["id"]]
            })
            .to_string()
            .as_bytes(),
            &WriteOpts {
                expand: Some("tags,watchers".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .json,
    );
    assert_eq!(a1["tags"], json!([t_rust["id"], t_db["id"], t_hid["id"]]));
    // Non-superuser expand filters out 'hidden' tag via tags view rule.
    let exp_tags = a1["expand"]["tags"].as_array().unwrap();
    assert_eq!(exp_tags.len(), 2);
    assert_eq!(exp_tags[0]["name"], "rust");
    assert_eq!(exp_tags[1]["name"], "db");
    assert_eq!(a1["expand"]["watchers"][0]["name"], "ann");

    let a2 = parse(
        &app.create_record(
            &anon(),
            "articles",
            json!({"title":"two","tags":[t_rust["id"]]})
                .to_string()
                .as_bytes(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(a2["tags"], json!([t_rust["id"]]));
    let a3 = parse(
        &app.create_record(&anon(), "articles", br#"{"title":"three"}"#)
            .await
            .unwrap(),
    );
    assert_eq!(a3["tags"], json!([]));

    // maxSelect validation rejects > 3 distinct tags.
    let too_many = app
        .create_record(
            &anon(),
            "articles",
            json!({
                "title":"overflow",
                "tags":[t_rust["id"], t_db["id"], t_web["id"], t_hid["id"]]
            })
            .to_string()
            .as_bytes(),
        )
        .await;
    assert!(matches!(too_many, Err(CoreError::Validation(_))));

    // Any-of (?=) vs all-of (=) and empty check (= null / != '').
    let list_titles = |filter: &str| {
        let app = app.clone();
        let filter = filter.to_owned();
        async move {
            let page = parse(
                &app.list_records(
                    &su(),
                    "articles",
                    &ListParams {
                        filter,
                        sort: "title".into(),
                        ..Default::default()
                    },
                )
                .await
                .unwrap(),
            );
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["title"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(list_titles("tags.name ?= 'db'").await, vec!["one"]);
    assert_eq!(list_titles("tags.name ?= 'rust'").await, vec!["one", "two"]);
    // All-of: a1 has rust+db+hidden (not all 'rust'); a2 has only 'rust'; a3 has none.
    assert_eq!(list_titles("tags.name = 'rust'").await, vec!["two"]);
    assert_eq!(list_titles("tags.weight >= 10").await, vec!["one", "two"]);
    assert_eq!(list_titles("tags = null").await, vec!["three"]);
    assert_eq!(list_titles("tags != ''").await, vec!["one", "two"]);
    assert_eq!(
        list_titles(&format!("tags ?= '{}'", t_db["id"].as_str().unwrap())).await,
        vec!["one"]
    );

    // Update replaces junction rows and preserves new order.
    let a1_upd = parse(
        &app.update_record(
            &anon(),
            "articles",
            a1["id"].as_str().unwrap(),
            json!({"tags":[t_web["id"], t_rust["id"]]})
                .to_string()
                .as_bytes(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(a1_upd["tags"], json!([t_web["id"], t_rust["id"]]));

    // onDelete cascade on tags removes the tag from articles.tags; restrict on watchers blocks deleting u1.
    app.delete_record(&su(), "tags", t_web["id"].as_str().unwrap())
        .await
        .unwrap();
    let a1_after = parse(
        &app.view_record(&anon(), "articles", a1["id"].as_str().unwrap())
            .await
            .unwrap(),
    );
    assert_eq!(a1_after["tags"], json!([t_rust["id"]]));
    assert!(matches!(
        app.delete_record(&su(), "users", u1["id"].as_str().unwrap())
            .await,
        Err(CoreError::Conflict(_))
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn collection_joins_and_back_relations_via_and_expand() {
    let (_dir, reg) = setup().await;
    let app = reg.app("demo").await.unwrap();
    app.put_collection(
        def(json!({
            "name":"teams",
            "fields":[{"name":"name","type":"text","required":true}],
            "rules":{"list":"","view":""}
        })),
        true,
    )
    .await
    .unwrap();
    app.put_collection(
        def(json!({
            "name":"members",
            "fields":[
                {"name":"user","type":"relation","collection":"users","required":true},
                {"name":"team","type":"relation","collection":"teams","required":true},
                {"name":"role","type":"text","required":true}
            ],
            "rules":{"list":"","view":"role != 'secret'"}
        })),
        true,
    )
    .await
    .unwrap();
    app.put_collection(
        def(json!({
            "name":"projects",
            "fields":[
                {"name":"title","type":"text","required":true},
                {"name":"team","type":"relation","collection":"teams","required":true},
                {"name":"contributors","type":"relation","collection":"users","maxSelect":5}
            ],
            "rules":{
                "list":"@collection.members.user = @request.auth.id && @collection.members.team = team",
                "view":"@collection.members.user = @request.auth.id && @collection.members.team = team"
            }
        })),
        true,
    )
    .await
    .unwrap();

    let u1_raw = new_id();
    let u2_raw = new_id();
    let u1 = parse(
        &app.create_record(
            &su(),
            "users",
            json!({"id":encode_string(&u1_raw),"name":"ann"})
                .to_string()
                .as_bytes(),
        )
        .await
        .unwrap(),
    );
    let u2 = parse(
        &app.create_record(
            &su(),
            "users",
            json!({"id":encode_string(&u2_raw),"name":"bo"})
                .to_string()
                .as_bytes(),
        )
        .await
        .unwrap(),
    );
    let tm_core = parse(
        &app.create_record(&su(), "teams", br#"{"name":"core"}"#)
            .await
            .unwrap(),
    );
    let tm_ops = parse(
        &app.create_record(&su(), "teams", br#"{"name":"ops"}"#)
            .await
            .unwrap(),
    );
    app.create_record(
        &su(),
        "members",
        json!({"user":u1["id"],"team":tm_core["id"],"role":"admin"})
            .to_string()
            .as_bytes(),
    )
    .await
    .unwrap();
    app.create_record(
        &su(),
        "members",
        json!({"user":u2["id"],"team":tm_core["id"],"role":"secret"})
            .to_string()
            .as_bytes(),
    )
    .await
    .unwrap();
    app.create_record(
        &su(),
        "members",
        json!({"user":u2["id"],"team":tm_ops["id"],"role":"member"})
            .to_string()
            .as_bytes(),
    )
    .await
    .unwrap();

    let p_core = parse(
        &app.create_record(
            &su(),
            "projects",
            json!({"title":"p-core","team":tm_core["id"],"contributors":[u1["id"], u2["id"]]})
                .to_string()
                .as_bytes(),
        )
        .await
        .unwrap(),
    );
    let _p_ops = parse(
        &app.create_record(
            &su(),
            "projects",
            json!({"title":"p-ops","team":tm_ops["id"],"contributors":[u2["id"]]})
                .to_string()
                .as_bytes(),
        )
        .await
        .unwrap(),
    );

    // @collection rule on projects: u1 is in tm_core only, so u1 sees only p-core.
    let u1_projects = parse(
        &app.list_records(&user(u1_raw), "projects", &ListParams::default())
            .await
            .unwrap(),
    );
    assert_eq!(u1_projects["items"].as_array().unwrap().len(), 1);
    assert_eq!(u1_projects["items"][0]["id"], p_core["id"]);

    // Back-relation filter over single relation (members_via_team) and multi-relation (projects_via_contributors).
    let teams_filtered = parse(
        &app.list_records(
            &anon(),
            "teams",
            &ListParams {
                filter: "members_via_team.role ?= 'admin'".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap(),
    );
    assert_eq!(teams_filtered["items"].as_array().unwrap().len(), 1);
    assert_eq!(teams_filtered["items"][0]["name"], "core");

    let users_filtered = parse(
        &app.list_records(
            &su(),
            "users",
            &ListParams {
                filter: "projects_via_contributors.title ?= 'p-ops'".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap(),
    );
    assert_eq!(users_filtered["items"].as_array().unwrap().len(), 1);
    assert_eq!(users_filtered["items"][0]["name"], "bo");

    // Back-relation expand: single-rel (members_via_team.user) respects target view rule; multi-rel (projects_via_contributors).
    let tm_view = parse(
        &app.view_record_with(
            &anon(),
            "teams",
            tm_core["id"].as_str().unwrap(),
            &ReadOpts {
                expand: Some("members_via_team.user".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .json,
    );
    let exp_members = tm_view["expand"]["members_via_team"].as_array().unwrap();
    assert_eq!(
        exp_members.len(),
        1,
        "secret role member hidden by view rule"
    );
    assert_eq!(exp_members[0]["role"], "admin");
    assert_eq!(exp_members[0]["expand"]["user"]["name"], "ann");

    let u2_view = parse(
        &app.view_record_with(
            &su(),
            "users",
            u2["id"].as_str().unwrap(),
            &ReadOpts {
                expand: Some("projects_via_contributors".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .json,
    );
    let exp_projs = u2_view["expand"]["projects_via_contributors"]
        .as_array()
        .unwrap();
    assert_eq!(exp_projs.len(), 2);

    // :isset and :changed write rules on projects:
    // - create allows setting title + team, but forbids setting contributors (@request.body.contributors:isset = false)
    // - update allows updating title, but forbids changing team (@request.body.team:changed = false)
    app.put_collection(
        def(json!({
            "name":"projects",
            "fields":[
                {"name":"title","type":"text","required":true},
                {"name":"team","type":"relation","collection":"teams","required":true},
                {"name":"contributors","type":"relation","collection":"users","maxSelect":5}
            ],
            "rules":{
                "list":"",
                "view":"",
                "create":"@request.body.contributors:isset = false",
                "update":"@request.body.team:changed = false"
            }
        })),
        false,
    )
    .await
    .unwrap();

    assert!(matches!(
        app.create_record(
            &anon(),
            "projects",
            json!({"title":"bad","team":tm_core["id"],"contributors":[u1["id"]]})
                .to_string()
                .as_bytes()
        )
        .await,
        Err(CoreError::Forbidden(_))
    ));
    let p_new = parse(
        &app.create_record(
            &anon(),
            "projects",
            json!({"title":"ok","team":tm_core["id"]})
                .to_string()
                .as_bytes(),
        )
        .await
        .unwrap(),
    );
    // Updating title without sending team, or sending the SAME team id, is allowed.
    app.update_record(
        &anon(),
        "projects",
        p_new["id"].as_str().unwrap(),
        json!({"title":"ok-2","team":tm_core["id"]})
            .to_string()
            .as_bytes(),
    )
    .await
    .unwrap();
    // Changing team to tm_ops is rejected by @request.body.team:changed = false.
    assert!(matches!(
        app.update_record(
            &anon(),
            "projects",
            p_new["id"].as_str().unwrap(),
            json!({"team":tm_ops["id"]}).to_string().as_bytes()
        )
        .await,
        Err(CoreError::NotFound(_))
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn fields_projection_keeps_paging_and_trims_output() {
    let (_dir, reg) = setup().await;
    let app = reg.app("demo").await.unwrap();
    for i in 0..9 {
        let body =
            json!({"title": format!("t{i}"), "status": "open", "priority": i % 3, "secret": "s"});
        app.create_record(&su(), "tasks", body.to_string().as_bytes())
            .await
            .unwrap();
    }
    let list = |fields: Option<&str>, limit, cursor: Option<String>| {
        let app = app.clone();
        let lp = ListParams {
            sort: "-priority,title".into(),
            fields: fields.map(str::to_owned),
            limit: Some(limit),
            cursor,
            ..Default::default()
        };
        async move { parse(&app.list_records(&su(), "tasks", &lp).await.unwrap()) }
    };
    let ids = |p: &J| -> Vec<J> {
        p["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].clone())
            .collect()
    };
    let want = ids(&list(None, 100, None).await);
    let (mut got, mut cursor) = (Vec::new(), None);
    loop {
        let page = list(Some("title"), 4, cursor).await;
        for r in page["items"].as_array().unwrap() {
            let mut keys: Vec<_> = r.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            assert_eq!(keys, ["id", "title"]);
        }
        got.extend(ids(&page));
        match page["next"].as_str() {
            Some(c) => cursor = Some(c.to_owned()),
            None => break,
        }
    }
    assert_eq!(got, want);
}
