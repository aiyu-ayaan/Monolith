//! SQL compiler vs predicate compiler must agree 100% (doc 06 §6, CLAUDE.md).

use monolith_rules::{
    Auth, Class, Ctx, FieldInfo, FieldKind, Mode, RuleError, SchemaView, Ty, Value, compile,
};
use monolith_store::id::encode_string;
use rusqlite::{Connection, params_from_iter};

struct Schema;

// Slots match column order of table t.
const T: [(&str, FieldKind, bool); 8] = [
    ("id", FieldKind::Scalar(Ty::Id), false),
    ("name", FieldKind::Scalar(Ty::Text), false),
    ("score", FieldKind::Scalar(Ty::Number), false),
    ("count", FieldKind::Scalar(Ty::Number), false),
    ("flag", FieldKind::Scalar(Ty::Bool), false),
    ("at", FieldKind::Scalar(Ty::Date), false),
    ("owner", FieldKind::Scalar(Ty::Id), false), // replaced below by a relation
    ("secret", FieldKind::Scalar(Ty::Text), true),
];

impl SchemaView for Schema {
    fn field(&self, coll: &str, name: &str) -> Option<FieldInfo> {
        match coll {
            "t" => {
                if name == "tags" {
                    return Some(FieldInfo {
                        slot: 8,
                        kind: FieldKind::Relation {
                            target: "tags".into(),
                            multi: true,
                        },
                        hidden: false,
                    });
                }
                T.iter()
                    .enumerate()
                    .find(|(_, f)| f.0 == name)
                    .map(|(slot, f)| FieldInfo {
                        slot,
                        kind: if name == "owner" {
                            FieldKind::Relation {
                                target: "users".into(),
                                multi: false,
                            }
                        } else {
                            f.1.clone()
                        },
                        hidden: f.2,
                    })
            }
            "users" => {
                let kind = match name {
                    "id" => FieldKind::Scalar(Ty::Id),
                    "name" | "role" => FieldKind::Scalar(Ty::Text),
                    "org" => FieldKind::Relation {
                        target: "orgs".into(),
                        multi: false,
                    },
                    _ => return None,
                };
                Some(FieldInfo {
                    slot: 0,
                    kind,
                    hidden: false,
                })
            }
            "orgs" => (name == "id" || name == "plan").then(|| FieldInfo {
                slot: 0,
                kind: FieldKind::Scalar(if name == "id" { Ty::Id } else { Ty::Text }),
                hidden: false,
            }),
            "tags" => (name == "id" || name == "label" || name == "weight").then_some(FieldInfo {
                slot: 0,
                kind: FieldKind::Scalar(match name {
                    "id" => Ty::Id,
                    "label" => Ty::Text,
                    _ => Ty::Number,
                }),
                hidden: false,
            }),
            _ => None,
        }
    }
}

/// xorshift64*: deterministic, no dependency.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

const NAMES: [&str; 9] = [
    "alice", "Alice", "bob", "a%b", "a_b", "x\\y", "Émile", "émile", "",
];
const IDS: [[u8; 16]; 3] = [[1; 16], [2; 16], [3; 16]];

fn random_row(rng: &mut Rng, n: usize) -> Vec<Value> {
    let maybe = |v: Value, rng: &mut Rng| if rng.below(5) == 0 { Value::Null } else { v };
    let mut id = [0u8; 16];
    id[..8].copy_from_slice(&(n as u64).to_be_bytes());
    vec![
        Value::Blob(id.into()),
        maybe(Value::Text((*rng.pick(&NAMES)).into()), rng),
        maybe(Value::Real((rng.below(41) as f64 - 20.0) / 4.0), rng),
        maybe(Value::Int(rng.below(11) as i64 - 5), rng),
        maybe(Value::Int(rng.below(2) as i64), rng),
        maybe(Value::Int(rng.below(5) as i64 * 86_400_000_000), rng),
        maybe(Value::Blob(rng.pick(&IDS).to_vec().into()), rng),
        maybe(Value::Text((*rng.pick(&NAMES)).into()), rng),
    ]
}

fn random_operand(rng: &mut Rng) -> String {
    let lits = [
        "'alice'",
        "'ALICE'",
        "'a%b'",
        "'_'",
        "'x\\\\y'",
        "'émile'",
        "''",
        "0",
        "1",
        "-2.5",
        "3",
        "2.0",
        "true",
        "false",
        "null",
        "'1970-01-02'",
        "'1970-01-03T00:00:00Z'",
    ];
    let fields = [
        "name",
        "name:lower",
        "score",
        "count",
        "flag",
        "at",
        "owner",
        "id",
        "@request.auth.id",
        "@request.auth.role",
        "@request.auth.collectionName",
        "@request.body.name",
        "@request.body.count",
        "@now",
    ];
    if rng.below(2) == 0 {
        (*rng.pick(&lits)).to_owned()
    } else {
        (*rng.pick(&fields)).to_owned()
    }
}

fn random_expr(rng: &mut Rng, depth: u32) -> String {
    if depth > 0 && rng.below(3) == 0 {
        let op = if rng.below(2) == 0 { "&&" } else { "||" };
        return format!(
            "({} {op} {})",
            random_expr(rng, depth - 1),
            random_expr(rng, depth - 1)
        );
    }
    if rng.below(8) == 0 {
        let l = random_operand(rng);
        let items: Vec<String> = (0..1 + rng.below(3)).map(|_| random_operand(rng)).collect();
        return format!("{l} in ({})", items.join(", "));
    }
    let op = *rng.pick(&["=", "!=", ">", ">=", "<", "<=", "~", "!~"]);
    format!("{} {op} {}", random_operand(rng), random_operand(rng))
}

fn db(rows: &[Vec<Value>]) -> Connection {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch(
        "CREATE TABLE t (id BLOB PRIMARY KEY, name TEXT, score REAL, count INTEGER, flag INTEGER,
                         at INTEGER, owner BLOB, secret TEXT) STRICT;
         CREATE TABLE users (id BLOB PRIMARY KEY, name TEXT, role TEXT, org BLOB) STRICT;
         CREATE TABLE orgs (id BLOB PRIMARY KEY, plan TEXT) STRICT;",
    )
    .unwrap();
    for r in rows {
        c.execute(
            "INSERT INTO t VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params_from_iter(r.iter()),
        )
        .unwrap();
    }
    c
}

fn sql_ids(c: &Connection, sql: &str, bound: &[Value]) -> Vec<Vec<u8>> {
    let q = format!("SELECT id FROM t WHERE {sql} ORDER BY id");
    let mut st = c.prepare(&q).unwrap_or_else(|e| panic!("{q}: {e}"));
    st.query_map(params_from_iter(bound.iter()), |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn mem_ids(c: &monolith_rules::Compiled, rows: &[Vec<Value>], bound: &[Value]) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = rows
        .iter()
        .filter(|r| c.eval(r, bound).unwrap())
        .map(|r| match &r[0] {
            Value::Blob(b) => b.to_vec(),
            _ => unreachable!(),
        })
        .collect();
    out.sort();
    out
}

#[test]
fn sql_and_predicate_agree_on_random_rules() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let rows: Vec<_> = (0..300).map(|n| random_row(&mut rng, n)).collect();
    let conn = db(&rows);

    let admin = Auth {
        id: IDS[0],
        collection: "users".into(),
        claims: vec![("role".into(), Value::Text("admin".into()))],
    };
    let odd = Auth {
        id: IDS[1],
        collection: "staff".into(),
        claims: vec![("role".into(), Value::Int(5))], // wrong type: coerced to NULL
    };
    let body = [
        Value::Null,
        Value::Text("Alice".into()),
        Value::Null,
        Value::Int(2),
        Value::Null,
        Value::Null,
        Value::Null,
        Value::Null,
    ];
    let ctxs = [
        Ctx {
            auth: None,
            body: None,
            now_us: 2 * 86_400_000_000,
        },
        Ctx {
            auth: Some(&admin),
            body: Some(&body),
            now_us: 0,
        },
        Ctx {
            auth: Some(&odd),
            body: None,
            now_us: 3 * 86_400_000_000,
        },
    ];

    let (mut compiled, mut rejected) = (0, 0);
    for _ in 0..12_000 {
        let src = random_expr(&mut rng, 3);
        let c = match compile(&Schema, "t", &src, Mode::Rule) {
            Ok(c) => c,
            Err(_) => {
                rejected += 1;
                continue;
            }
        };
        compiled += 1;
        assert!(c.class <= Class::RowLocal, "{src}");
        for ctx in &ctxs {
            let bound = c.bind(ctx);
            assert_eq!(
                sql_ids(&conn, &c.sql, &bound),
                mem_ids(&c, &rows, &bound),
                "rule: {src}\nsql: {}\nbound: {bound:?}",
                c.sql
            );
        }
    }
    // Guard against a generator that only produces type errors.
    assert!(compiled > 2000, "compiled {compiled}, rejected {rejected}");
}

#[test]
fn handwritten_rules_agree() {
    let mut rng = Rng(42);
    let rows: Vec<_> = (0..300).map(|n| random_row(&mut rng, n)).collect();
    let conn = db(&rows);
    let me = Auth {
        id: IDS[2],
        collection: "users".into(),
        claims: vec![],
    };
    let owner = encode_string(&IDS[1]);
    let rules = [
        "owner = @request.auth.id".to_owned(),
        "@request.auth.id != '' && (owner = @request.auth.id || flag = true)".into(),
        format!("owner = '{owner}' || owner = null"),
        "name:lower ~ 'ali' && score >= 0".into(),
        "name !~ '%' && count in (1, 2, 3)".into(),
        "at > '1970-01-02' && at <= @now".into(),
        "count > score || score = 2".into(),
        "".into(),
    ];
    for src in &rules {
        let c = compile(&Schema, "t", src, Mode::Rule).unwrap_or_else(|e| panic!("{src}: {e}"));
        for ctx in [
            Ctx::default(),
            Ctx {
                auth: Some(&me),
                body: None,
                now_us: 3 * 86_400_000_000,
            },
        ] {
            let bound = c.bind(&ctx);
            assert_eq!(
                sql_ids(&conn, &c.sql, &bound),
                mem_ids(&c, &rows, &bound),
                "{src}"
            );
        }
    }
}

#[test]
fn sql_shape_and_classes() {
    let c = compile(
        &Schema,
        "t",
        "owner = @request.auth.id && 'x' = name",
        Mode::Rule,
    )
    .unwrap();
    assert_eq!(c.sql, "(t.\"owner\" IS ? AND ? IS t.\"name\")");
    assert_eq!(c.class, Class::RowLocal);
    let me = Auth {
        id: IDS[0],
        collection: "users".into(),
        claims: vec![],
    };
    // Bind order follows the SQL text, not the order operands were checked.
    let b = compile(
        &Schema,
        "t",
        "'x' = @request.auth.collectionName",
        Mode::Rule,
    )
    .unwrap()
    .bind(&Ctx {
        auth: Some(&me),
        ..Ctx::default()
    });
    assert_eq!(
        b,
        vec![Value::Text("x".into()), Value::Text("users".into())]
    );

    assert_eq!(
        compile(&Schema, "t", "", Mode::Rule).unwrap().class,
        Class::Const
    );
    assert_eq!(
        compile(&Schema, "t", "1 = 1", Mode::Rule).unwrap().class,
        Class::Const
    );
    assert_eq!(
        compile(&Schema, "t", "@request.auth.role = 'admin'", Mode::Rule)
            .unwrap()
            .class,
        Class::AuthOnly
    );

    let rel = compile(
        &Schema,
        "t",
        "owner.org.plan = 'pro' && owner.name ~ 'a'",
        Mode::Rule,
    )
    .unwrap();
    assert_eq!(rel.class, Class::Relational);
    assert_eq!(
        rel.sql,
        "(EXISTS (SELECT 1 FROM \"users\" r1 JOIN \"orgs\" r2 ON r2.\"id\" = r1.\"org\" WHERE r1.\"id\" = t.\"owner\" AND r2.\"plan\" IS ?) \
         AND EXISTS (SELECT 1 FROM \"users\" r1 WHERE r1.\"id\" = t.\"owner\" AND r1.\"name\" LIKE ? ESCAPE '\\'))"
    );
    assert_eq!(rel.eval(&[], &[]), None);
    // Relational SQL must run.
    let conn = db(&[]);
    sql_ids(&conn, &rel.sql, &rel.bind(&Ctx::default()));

    // Multi-relation any-of (`?=`) vs all (`=`) and bare junction checks.
    conn.execute_batch(
        "CREATE TABLE tags (id BLOB PRIMARY KEY, label TEXT, weight INTEGER) STRICT;
         CREATE TABLE _rel_t_tags (owner BLOB NOT NULL, target BLOB NOT NULL, pos INTEGER NOT NULL, PRIMARY KEY (owner, target)) STRICT, WITHOUT ROWID;",
    )
    .unwrap();
    let p1 = [10u8; 16];
    let p2 = [11u8; 16];
    let p3 = [12u8; 16];
    for p in [p1, p2, p3] {
        conn.execute(
            "INSERT INTO t VALUES (?, 'n', 0.0, 0, 0, 0, NULL, NULL)",
            [&p[..]],
        )
        .unwrap();
    }
    let tag_a = [21u8; 16];
    let tag_b = [22u8; 16];
    conn.execute(
        "INSERT INTO tags VALUES (?, 'rust', 10), (?, 'go', 5)",
        (&tag_a[..], &tag_b[..]),
    )
    .unwrap();
    // p1 has [rust, go], p2 has [rust], p3 has []
    conn.execute(
        "INSERT INTO _rel_t_tags VALUES (?, ?, 0), (?, ?, 1), (?, ?, 0)",
        (
            &p1[..],
            &tag_a[..],
            &p1[..],
            &tag_b[..],
            &p2[..],
            &tag_a[..],
        ),
    )
    .unwrap();

    let tag_a_s = encode_string(&tag_a);
    let any_id = compile(&Schema, "t", &format!("tags ?= '{tag_a_s}'"), Mode::Filter).unwrap();
    assert_eq!(any_id.class, Class::Relational);
    assert_eq!(
        sql_ids(&conn, &any_id.sql, &any_id.bind(&Ctx::default())),
        vec![p1.to_vec(), p2.to_vec()]
    );

    let all_label = compile(&Schema, "t", "tags.label = 'rust'", Mode::Filter).unwrap();
    assert_eq!(
        sql_ids(&conn, &all_label.sql, &all_label.bind(&Ctx::default())),
        vec![p2.to_vec()]
    );

    let any_label = compile(&Schema, "t", "tags.label ?= 'go'", Mode::Filter).unwrap();
    assert_eq!(
        sql_ids(&conn, &any_label.sql, &any_label.bind(&Ctx::default())),
        vec![p1.to_vec()]
    );

    let empty_tags = compile(&Schema, "t", "tags = null", Mode::Filter).unwrap();
    assert_eq!(
        sql_ids(&conn, &empty_tags.sql, &empty_tags.bind(&Ctx::default())),
        vec![p3.to_vec()]
    );

    let non_empty_tags = compile(&Schema, "t", "tags != ''", Mode::Filter).unwrap();
    assert_eq!(
        sql_ids(
            &conn,
            &non_empty_tags.sql,
            &non_empty_tags.bind(&Ctx::default())
        ),
        vec![p1.to_vec(), p2.to_vec()]
    );
}

#[test]
fn type_and_access_errors() {
    let err = |src: &str, mode| compile(&Schema, "t", src, mode).unwrap_err();
    assert!(matches!(err("name = 1", Mode::Rule), RuleError::Type(_)));
    assert!(matches!(err("name ?= 'a'", Mode::Rule), RuleError::Type(_)));
    assert!(matches!(err("flag > true", Mode::Rule), RuleError::Type(_)));
    assert!(matches!(err("count ~ 1", Mode::Rule), RuleError::Type(_)));
    assert!(matches!(
        err("count > null", Mode::Rule),
        RuleError::Type(_)
    ));
    assert!(matches!(
        err("at = 'yesterday'", Mode::Rule),
        RuleError::Type(_)
    ));
    assert!(matches!(
        err("owner = 'not-an-id'", Mode::Rule),
        RuleError::Type(_)
    ));
    assert!(matches!(
        err("nope = 1", Mode::Rule),
        RuleError::UnknownField(_)
    ));
    assert!(matches!(err("name.x = 1", Mode::Rule), RuleError::Type(_)));
    assert!(matches!(
        err("name ~ name", Mode::Rule),
        RuleError::Unsupported(_)
    ));
    assert!(matches!(
        err("@request.auth.a = @request.auth.b", Mode::Rule),
        RuleError::Type(_)
    ));
    // Hidden fields: allowed in admin rules, invisible to client filters.
    assert!(compile(&Schema, "t", "secret = 'x'", Mode::Rule).is_ok());
    assert!(matches!(
        err("secret = 'x'", Mode::Filter),
        RuleError::UnknownField(_)
    ));
}
