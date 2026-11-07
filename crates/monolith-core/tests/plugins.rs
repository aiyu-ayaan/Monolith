use std::time::Duration;

use monolith_core::{CollectionDef, CoreError, PluginEdit, Principal, Registry};
use serde_json::json;

/// Echoes the first bytes of the payload to `monolith.log` and returns 0; `fail` makes it return 7;
/// `spin` loops forever.
fn module(mode: &str) -> Vec<u8> {
    let body = match mode {
        "ok" => "(call $log (i32.const 1) (local.get $p) (i32.const 40)) (i32.const 0)",
        "fail" => "(i32.const 7)",
        _ => "(loop $l (br $l)) (i32.const 0)",
    };
    wat::parse_str(format!(
        r#"(module
          (import "monolith" "log" (func $log (param i32 i32 i32)))
          (memory (export "memory") 2)
          (global $next (mut i32) (i32.const 1024))
          (func (export "alloc") (param $n i32) (result i32)
            (local $p i32) (local.set $p (global.get $next))
            (global.set $next (i32.add (global.get $next) (local.get $n)))
            (local.get $p))
          (func (export "on_event") (param $p i32) (param $n i32) (result i32) {body}))"#
    ))
    .unwrap()
}

fn edit(events: &[&str]) -> PluginEdit {
    PluginEdit {
        name: "p".into(),
        events: events.iter().map(|s| (*s).to_owned()).collect(),
        enabled: true,
        fuel: None,
    }
}

async fn setup(name: &str) -> (tempfile::TempDir, Registry) {
    let dir = tempfile::tempdir().unwrap();
    let reg = Registry::open(dir.path()).await.unwrap();
    reg.create_app(name).await.unwrap();
    reg.app(name)
        .await
        .unwrap()
        .put_collection(
            serde_json::from_value::<CollectionDef>(
                json!({"name":"t","fields":[{"name":"n","type":"number"}]}),
            )
            .unwrap(),
            true,
        )
        .await
        .unwrap();
    (dir, reg)
}

async fn until(f: impl Fn() -> bool) {
    for _ in 0..100 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("condition not reached");
}

#[tokio::test(flavor = "multi_thread")]
async fn plugin_runs_on_matching_events_and_logs() {
    let (_d, reg) = setup("pl").await;
    let p = reg
        .create_plugin("pl", edit(&["t.create"]), module("ok"))
        .await
        .unwrap();
    let app = reg.app("pl").await.unwrap();
    let su = Principal::superuser();
    app.create_record(&su, "t", br#"{"n":1}"#).await.unwrap();
    for _ in 0..100 {
        if reg.list_plugins("pl").await.unwrap()[0].calls >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let info = &reg.list_plugins("pl").await.unwrap()[0];
    assert_eq!((info.calls, info.errors), (1, 0));
    let logs = reg.plugin_logs("pl", &p.id).await.unwrap();
    assert!(logs[0].starts_with("[info] {\"id\":"), "got {logs:?}");
    // A delete does not match `t.create`.
    let id = {
        let r: serde_json::Value =
            serde_json::from_slice(&app.create_record(&su, "t", br#"{"n":2}"#).await.unwrap())
                .unwrap();
        r["id"].as_str().unwrap().to_owned()
    };
    app.delete_record(&su, "t", &id).await.unwrap();
    until(|| true).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(reg.list_plugins("pl").await.unwrap()[0].calls, 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn plugin_failures_and_fuel_are_contained() {
    let (_d, reg) = setup("pf").await;
    reg.create_plugin("pf", edit(&["*"]), module("fail"))
        .await
        .unwrap();
    let mut spin = edit(&["*"]);
    spin.name = "spin".into();
    spin.fuel = Some(100_000);
    reg.create_plugin("pf", spin, module("spin")).await.unwrap();
    let app = reg.app("pf").await.unwrap();
    app.create_record(&Principal::superuser(), "t", br#"{"n":1}"#)
        .await
        .unwrap();
    for _ in 0..100 {
        let l = reg.list_plugins("pf").await.unwrap();
        if l.iter().all(|p| p.calls >= 1) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let l = reg.list_plugins("pf").await.unwrap();
    assert_eq!(l[0].last_error.as_deref(), Some("on_event returned 7"));
    assert!(
        l[1].last_error.as_deref().unwrap().contains("out of fuel"),
        "{:?}",
        l[1].last_error
    );
    // The server is still fine: records keep working.
    app.create_record(&Principal::superuser(), "t", br#"{"n":2}"#)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_modules_and_events_are_refused() {
    let (_d, reg) = setup("pi").await;
    let bad = |r: Result<_, CoreError>| matches!(r, Err(CoreError::Validation(_)));
    assert!(bad(reg
        .create_plugin("pi", edit(&["*"]), b"not wasm".to_vec())
        .await));
    // Missing exports.
    let m = wat::parse_str("(module (memory (export \"memory\") 1))").unwrap();
    assert!(bad(reg.create_plugin("pi", edit(&["*"]), m).await));
    // Imports beyond `monolith.log` are not available in the sandbox.
    let m = wat::parse_str(
        r#"(module (import "env" "fetch" (func))
           (memory (export "memory") 1)
           (func (export "alloc") (param i32) (result i32) (i32.const 0))
           (func (export "on_event") (param i32 i32) (result i32) (i32.const 0)))"#,
    )
    .unwrap();
    assert!(bad(reg.create_plugin("pi", edit(&["*"]), m).await));
    assert!(bad(reg
        .create_plugin("pi", edit(&["t.bogus"]), module("ok"))
        .await));
    // Delete stops further calls.
    let p = reg
        .create_plugin("pi", edit(&["*"]), module("ok"))
        .await
        .unwrap();
    reg.delete_plugin("pi", &p.id).await.unwrap();
    assert!(reg.list_plugins("pi").await.unwrap().is_empty());
}
