//! Rule compile, bind and in-memory eval (doc 17 §3: "rule predicate eval, filter compile").

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use monolith_rules::{Auth, Ctx, FieldInfo, FieldKind, Mode, SchemaView, Ty, Value, compile};

struct Schema;

impl SchemaView for Schema {
    fn field(&self, _: &str, name: &str) -> Option<FieldInfo> {
        let (slot, ty) = match name {
            "owner" => (0, Ty::Id),
            "status" => (1, Ty::Text),
            "title" => (2, Ty::Text),
            "priority" => (3, Ty::Number),
            _ => return None,
        };
        Some(FieldInfo {
            slot,
            kind: FieldKind::Scalar(ty),
            hidden: false,
        })
    }
}

const RULE: &str =
    "owner = @request.auth.id && status != 'draft' && (title:lower ~ 'ride' || priority >= 3)";

fn rules(c: &mut Criterion) {
    c.bench_function("rules/compile", |b| {
        b.iter(|| compile(&Schema, "tasks", black_box(RULE), Mode::Rule).unwrap())
    });
    let compiled = compile(&Schema, "tasks", RULE, Mode::Rule).unwrap();
    let auth = Auth {
        id: [7; 16],
        collection: "users".into(),
        claims: vec![],
    };
    let ctx = Ctx {
        auth: Some(&auth),
        body: None,
        now_us: 0,
    };
    c.bench_function("rules/bind", |b| b.iter(|| compiled.bind(black_box(&ctx))));
    let bound = compiled.bind(&ctx);
    let row = [
        Value::Blob([7; 16].into()),
        Value::Text("open".into()),
        Value::Text("Ride to airport".into()),
        Value::Int(1),
    ];
    c.bench_function("rules/eval_row_local", |b| {
        b.iter(|| compiled.eval(black_box(&row), black_box(&bound)))
    });
}

criterion_group!(benches, rules);
criterion_main!(benches);
