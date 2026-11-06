//! Resolve + type-check the AST against a schema, then emit SQL (doc 06 §2–§3).

use std::fmt::Write;

use crate::parse::{self, CmpOp, Expr, Lit, Modifier, Operand};
use crate::{Class, Ctx, FieldKind, Mode, RuleError, SchemaView, Ty, Value};

pub const MAX_HOPS: usize = 4;
pub const MAX_COLLECTION_JOINS: usize = 4;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Source {
    Lit(Value),
    AuthId,
    AuthCollection,
    AuthClaim(Box<str>),
    Body(usize),
    Now,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Param {
    pub src: Source,
    pub ty: Ty,
    /// Bound as a `%…%` LIKE pattern with `\` escapes.
    pub like: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Root {
    /// The queried collection (`t`).
    Main,
    /// `@collection` join binding index `k` (`c{k + 1}`).
    Join(usize),
}

impl Root {
    fn alias(self) -> String {
        match self {
            Root::Main => "t".to_owned(),
            Root::Join(k) => format!("c{}", k + 1),
        }
    }

    fn mask(self) -> u8 {
        match self {
            Root::Main => 0,
            Root::Join(k) => 1 << k,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Term {
    /// `depth` 0 is the root table (`t` or `c{k}`), n is relation hop `rn` (or `j{n}` for a
    /// bare junction hop).
    Col {
        root: Root,
        depth: usize,
        junction: bool,
        column: String,
        slot: usize,
        lower: bool,
    },
    Param {
        idx: usize,
        lower: bool,
    },
}

impl Term {
    fn join_mask(&self) -> u8 {
        match self {
            Term::Col { root, .. } => root.mask(),
            Term::Param { .. } => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Hop {
    /// Single-relation hop: `table` (`r{n}`) joined via `via` on the previous alias.
    Single { via: String, table: String },
    /// Multi-relation hop with further field access: `junction` (`j{n}`) + `table` (`r{n}`).
    Multi { junction: String, table: String },
    /// Bare multi-relation hop (`tags ?= 'id'`): `junction` (`j{n}`) only, comparing `j{n}."target"`.
    Junction { junction: String },
    /// Back-relation hop over a single-relation field (`comments_via_post`):
    /// `table` (`r{n}`) where `r{n}."{via}" = {prev}."id"`.
    BackSingle {
        via: String,
        table: String,
        bare: bool,
    },
    /// Back-relation hop over a multi-relation field (`articles_via_tags.title`):
    /// `junction` (`j{n}`) where `j{n}."target" = {prev}."id"` + `table` (`r{n}`) on `r{n}."id" = j{n}."owner"`.
    BackMulti { junction: String, table: String },
    /// Bare back-relation hop over a multi-relation field (`articles_via_tags ?= 'id'`):
    /// `junction` (`j{n}`) where `j{n}."target" = {prev}."id"`, comparing `j{n}."owner"`.
    BackJunction { junction: String },
}

impl Hop {
    fn is_multi(&self) -> bool {
        matches!(
            self,
            Hop::Multi { .. }
                | Hop::Junction { .. }
                | Hop::BackSingle { .. }
                | Hop::BackMulti { .. }
                | Hop::BackJunction { .. }
        )
    }

    fn is_bare_multi(&self) -> bool {
        matches!(
            self,
            Hop::Junction { .. } | Hop::BackJunction { .. } | Hop::BackSingle { bare: true, .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node {
    True,
    And(Box<Node>, Box<Node>),
    Or(Box<Node>, Box<Node>),
    Cmp {
        l: Term,
        op: CmpOp,
        any: bool,
        r: Term,
        hop_root: Root,
        hops: Vec<Hop>,
    },
    In {
        l: Term,
        list: Vec<Term>,
        hop_root: Root,
        hops: Vec<Hop>,
    },
}

impl Node {
    fn join_mask(&self) -> u8 {
        match self {
            Node::True => 0,
            Node::And(a, b) | Node::Or(a, b) => a.join_mask() | b.join_mask(),
            Node::Cmp { l, r, hop_root, .. } => l.join_mask() | r.join_mask() | hop_root.mask(),
            Node::In {
                l, list, hop_root, ..
            } => list
                .iter()
                .fold(l.join_mask() | hop_root.mask(), |m, t| m | t.join_mask()),
        }
    }
}

/// A compiled rule or filter for one (schema version, collection).
#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    /// `WHERE` fragment over alias `t`, with anonymous `?` binds in `bind()` order.
    pub sql: String,
    pub class: Class,
    pub(crate) params: Vec<Param>,
    pub(crate) node: Node,
}

/// Compiles rule text for `collection`. Empty text means "allow everyone".
pub fn compile(
    schema: &dyn SchemaView,
    collection: &str,
    src: &str,
    mode: Mode,
) -> Result<Compiled, RuleError> {
    if src.trim().is_empty() {
        return Ok(Compiled {
            sql: "1".into(),
            class: Class::Const,
            params: vec![],
            node: Node::True,
        });
    }
    let ast = parse::parse(src)?;
    let mut c = Checker {
        schema,
        collection,
        mode,
        params: vec![],
        joins: vec![],
        class: Class::Const,
    };
    let mut node = c.expr(&ast)?;
    let mut sql = String::with_capacity(src.len() * 2);
    let mut order = Vec::with_capacity(c.params.len());
    emit(&node, &c.joins, 0, &c.params, &mut order, &mut sql);
    // Renumber params into SQL text order so `bind()` matches the anonymous `?`s.
    let mut pos = vec![0; c.params.len()];
    for (new, &old) in order.iter().enumerate() {
        pos[old] = new;
    }
    renumber(&mut node, &pos);
    let params = order.iter().map(|&i| c.params[i].clone()).collect();
    Ok(Compiled {
        sql,
        class: c.class,
        params,
        node,
    })
}

impl Compiled {
    /// Resolves bind values for one request. SQL binds and `eval` use the same slice.
    pub fn bind(&self, ctx: &Ctx<'_>) -> Vec<Value> {
        self.params.iter().map(|p| p.resolve(ctx)).collect()
    }
}

/// An operand before type unification.
enum Pre {
    Typed(Term, Ty, Root, Vec<Hop>),
    /// Literal or claim: takes the other side's type.
    Flex(Source, bool),
}

struct Checker<'a> {
    schema: &'a dyn SchemaView,
    collection: &'a str,
    mode: Mode,
    params: Vec<Param>,
    /// Distinct `@collection.<table>(:<alias>)?` bindings: `(alias_key, table_name)`.
    joins: Vec<(String, String)>,
    class: Class,
}

impl Checker<'_> {
    fn expr(&mut self, e: &Expr) -> Result<Node, RuleError> {
        Ok(match e {
            Expr::And(a, b) => Node::And(Box::new(self.expr(a)?), Box::new(self.expr(b)?)),
            Expr::Or(a, b) => Node::Or(Box::new(self.expr(a)?), Box::new(self.expr(b)?)),
            Expr::Cmp { l, op, any, r } => self.cmp(l, *op, *any, r)?,
            Expr::In(l, list) => self.in_list(l, list)?,
        })
    }

    fn see(&mut self, c: Class) {
        self.class = self.class.max(c);
    }

    fn cmp(&mut self, l: &Operand, op: CmpOp, any: bool, r: &Operand) -> Result<Node, RuleError> {
        let (l, r) = (self.operand(l)?, self.operand(r)?);
        if matches!(op, CmpOp::Like | CmpOp::NotLike)
            && !matches!(r, Pre::Flex(..) | Pre::Typed(Term::Param { .. }, ..))
        {
            return Err(RuleError::Unsupported(
                "the right side of ~ must be a value, not a field",
            ));
        }
        let ty = match (&l, &r) {
            (Pre::Typed(_, a, ..), Pre::Typed(_, b, ..)) if a != b => {
                return Err(RuleError::Type(format!("cannot compare {a:?} with {b:?}")));
            }
            (Pre::Typed(_, t, ..), _) | (_, Pre::Typed(_, t, ..)) => *t,
            (Pre::Flex(Source::Lit(v), _), _) | (_, Pre::Flex(Source::Lit(v), _)) => lit_ty(v)?,
            _ => {
                return Err(RuleError::Type(
                    "cannot infer the type of a claim compared with a claim".into(),
                ));
            }
        };
        check_op(op, ty)?;
        let like = matches!(op, CmpOp::Like | CmpOp::NotLike);
        let (l, lroot, lh) = self.finish(l, ty, false)?;
        let (r, rroot, rh) = self.finish(r, ty, like)?;
        let (hop_root, hops) = self.one_path((lroot, lh), (rroot, rh))?;
        let has_multi = hops.iter().any(Hop::is_multi);
        let has_join = (l.join_mask() | r.join_mask() | hop_root.mask()) != 0;
        if any && !has_multi && !has_join {
            return Err(RuleError::Type(
                "any-of operators (?=) require a multi-relation field".into(),
            ));
        }
        if (is_null(&l, &self.params) || is_null(&r, &self.params))
            && !matches!(op, CmpOp::Eq | CmpOp::Ne)
        {
            return Err(RuleError::Type("null only works with = and !=".into()));
        }
        Ok(Node::Cmp {
            l,
            op,
            any,
            r,
            hop_root,
            hops,
        })
    }

    fn in_list(&mut self, l: &Operand, list: &[Operand]) -> Result<Node, RuleError> {
        let l = self.operand(l)?;
        let Pre::Typed(_, ty, ..) = &l else {
            return Err(RuleError::Type(
                "the left side of 'in' must be a field or typed value".into(),
            ));
        };
        let ty = *ty;
        let (l, hop_root, hops) = self.finish(l, ty, false)?;
        let mut terms = Vec::with_capacity(list.len());
        for o in list {
            let pre = self.operand(o)?;
            if !matches!(pre, Pre::Flex(..) | Pre::Typed(Term::Param { .. }, ..)) {
                return Err(RuleError::Unsupported("'in' lists hold values, not fields"));
            }
            if let Pre::Typed(_, t, ..) = &pre
                && *t != ty
            {
                return Err(RuleError::Type(format!(
                    "'in' list item is {t:?}, expected {ty:?}"
                )));
            }
            let (t, _, _) = self.finish(pre, ty, false)?;
            if is_null(&t, &self.params) {
                return Err(RuleError::Type("null is not allowed in 'in' lists".into()));
            }
            terms.push(t);
        }
        Ok(Node::In {
            l,
            list: terms,
            hop_root,
            hops,
        })
    }

    fn one_path(
        &self,
        (ar, a): (Root, Vec<Hop>),
        (br, b): (Root, Vec<Hop>),
    ) -> Result<(Root, Vec<Hop>), RuleError> {
        match (a.is_empty(), b.is_empty()) {
            (_, true) => Ok((ar, a)),
            (true, false) => Ok((br, b)),
            _ => Err(RuleError::Unsupported(
                "relation paths on both sides of one comparison",
            )),
        }
    }

    /// Turns a pre-operand into a term of type `ty`, converting literals.
    fn finish(&mut self, p: Pre, ty: Ty, like: bool) -> Result<(Term, Root, Vec<Hop>), RuleError> {
        match p {
            Pre::Typed(Term::Param { idx, lower }, _, root, hops) => {
                self.params[idx].like = like;
                Ok((Term::Param { idx, lower }, root, hops))
            }
            Pre::Typed(t, _, root, hops) => Ok((t, root, hops)),
            Pre::Flex(src, lower) => {
                let src = match src {
                    Source::Lit(v) => Source::Lit(convert_lit(v, ty)?),
                    s => s,
                };
                if lower && ty != Ty::Text {
                    return Err(RuleError::Type(":lower only applies to text".into()));
                }
                self.params.push(Param { src, ty, like });
                Ok((
                    Term::Param {
                        idx: self.params.len() - 1,
                        lower,
                    },
                    Root::Main,
                    vec![],
                ))
            }
        }
    }

    fn operand(&mut self, o: &Operand) -> Result<Pre, RuleError> {
        let (parts, modifier) = match o {
            Operand::Lit(l) => {
                return Ok(Pre::Flex(
                    Source::Lit(match l {
                        Lit::Str(s) => Value::Text(s.as_str().into()),
                        Lit::Int(i) => Value::Int(*i),
                        Lit::Real(f) => Value::Real(*f),
                        Lit::Bool(b) => Value::Int(*b as i64),
                        Lit::Null => Value::Null,
                    }),
                    false,
                ));
            }
            Operand::Path { parts, modifier } => (parts, *modifier),
        };
        let lower = modifier == Some(Modifier::Lower);
        let typed_param = |this: &mut Self, src: Source, ty: Ty| {
            if lower && ty != Ty::Text {
                return Err(RuleError::Type(":lower only applies to text".into()));
            }
            this.params.push(Param {
                src,
                ty,
                like: false,
            });
            Ok(Pre::Typed(
                Term::Param {
                    idx: this.params.len() - 1,
                    lower,
                },
                ty,
                Root::Main,
                vec![],
            ))
        };
        let p: Vec<&str> = parts.iter().map(String::as_str).collect();
        let (root, start_coll, field_segs) = if p[0].starts_with('@') {
            match p.as_slice() {
                ["@now"] => {
                    self.see(Class::AuthOnly);
                    return typed_param(self, Source::Now, Ty::Date);
                }
                ["@request", "auth", "id"] => {
                    self.see(Class::AuthOnly);
                    return typed_param(self, Source::AuthId, Ty::Id);
                }
                ["@request", "auth", "collectionName"] => {
                    self.see(Class::AuthOnly);
                    return typed_param(self, Source::AuthCollection, Ty::Text);
                }
                ["@request", "auth", claim] => {
                    self.see(Class::AuthOnly);
                    return Ok(Pre::Flex(Source::AuthClaim((*claim).into()), lower));
                }
                ["@request", "body", name] => {
                    self.see(Class::AuthOnly);
                    let f = self
                        .schema
                        .field(self.collection, name)
                        .ok_or_else(|| RuleError::UnknownField(format!("@request.body.{name}")))?;
                    if matches!(f.kind, FieldKind::Relation { multi: true, .. }) {
                        return Err(RuleError::Unsupported(
                            "@request.body on multi-relation fields",
                        ));
                    }
                    return typed_param(self, Source::Body(f.slot), kind_ty(&f.kind));
                }
                ["@collection", raw_coll, rest @ ..] if !rest.is_empty() => {
                    let (coll_name, alias_key) = match raw_coll.split_once(':') {
                        Some((c, a)) if !c.is_empty() && !a.is_empty() => (c, format!("{c}:{a}")),
                        Some(_) => return Err(RuleError::UnknownField(parts.join("."))),
                        None => (*raw_coll, (*raw_coll).to_owned()),
                    };
                    if self.schema.field(coll_name, "id").is_none() {
                        return Err(RuleError::UnknownField(parts.join(".")));
                    }
                    self.see(Class::Relational);
                    let join_idx = match self.joins.iter().position(|(k, _)| k == &alias_key) {
                        Some(idx) => idx,
                        None => {
                            if self.joins.len() >= MAX_COLLECTION_JOINS {
                                return Err(RuleError::Limit("more than 4 @collection joins"));
                            }
                            self.joins.push((alias_key, coll_name.to_owned()));
                            self.joins.len() - 1
                        }
                    };
                    (Root::Join(join_idx), coll_name.to_owned(), rest)
                }
                _ => return Err(RuleError::UnknownField(parts.join("."))),
            }
        } else {
            (Root::Main, self.collection.to_owned(), p.as_slice())
        };

        if field_segs.len() > MAX_HOPS + 1 {
            return Err(RuleError::Limit("more than 4 relation hops"));
        }
        let mut coll = start_coll;
        let mut hops = Vec::new();
        for (i, name) in field_segs.iter().enumerate() {
            let last = i + 1 == field_segs.len();
            if let Some(f) = self.schema.field(&coll, name) {
                if f.hidden && self.mode == Mode::Filter {
                    return Err(RuleError::UnknownField(parts.join(".")));
                }
                match (&f.kind, last) {
                    (FieldKind::Relation { multi: true, .. }, true) => {
                        if lower {
                            return Err(RuleError::Type(":lower only applies to text".into()));
                        }
                        self.see(Class::Relational);
                        hops.push(Hop::Junction {
                            junction: format!("_rel_{coll}_{name}"),
                        });
                        let col = Term::Col {
                            root,
                            depth: hops.len(),
                            junction: true,
                            column: "target".to_owned(),
                            slot: f.slot,
                            lower: false,
                        };
                        return Ok(Pre::Typed(col, Ty::Id, root, hops));
                    }
                    (kind, true) => {
                        let ty = kind_ty(kind);
                        if lower && ty != Ty::Text {
                            return Err(RuleError::Type(":lower only applies to text".into()));
                        }
                        self.see(if hops.is_empty() && root == Root::Main {
                            Class::RowLocal
                        } else {
                            Class::Relational
                        });
                        let col = Term::Col {
                            root,
                            depth: hops.len(),
                            junction: false,
                            column: (*name).to_owned(),
                            slot: f.slot,
                            lower,
                        };
                        return Ok(Pre::Typed(col, ty, root, hops));
                    }
                    (FieldKind::Relation { target, multi }, false) => {
                        if *multi {
                            hops.push(Hop::Multi {
                                junction: format!("_rel_{coll}_{name}"),
                                table: target.clone(),
                            });
                        } else {
                            hops.push(Hop::Single {
                                via: (*name).to_owned(),
                                table: target.clone(),
                            });
                        }
                        coll = target.clone();
                    }
                    (FieldKind::Scalar(_), false) => {
                        return Err(RuleError::Type(format!("'{name}' is not a relation")));
                    }
                }
            } else if let Some((src_coll, rel_field)) = name.split_once("_via_")
                && let Some(f) = self.schema.field(src_coll, rel_field)
                && let FieldKind::Relation { target, multi } = &f.kind
                && target == &coll
                && !(f.hidden && self.mode == Mode::Filter)
            {
                self.see(Class::Relational);
                if last {
                    if lower {
                        return Err(RuleError::Type(":lower only applies to text".into()));
                    }
                    if *multi {
                        hops.push(Hop::BackJunction {
                            junction: format!("_rel_{src_coll}_{rel_field}"),
                        });
                        let col = Term::Col {
                            root,
                            depth: hops.len(),
                            junction: true,
                            column: "owner".to_owned(),
                            slot: 0,
                            lower: false,
                        };
                        return Ok(Pre::Typed(col, Ty::Id, root, hops));
                    } else {
                        hops.push(Hop::BackSingle {
                            via: rel_field.to_owned(),
                            table: src_coll.to_owned(),
                            bare: true,
                        });
                        let col = Term::Col {
                            root,
                            depth: hops.len(),
                            junction: false,
                            column: "id".to_owned(),
                            slot: 0,
                            lower: false,
                        };
                        return Ok(Pre::Typed(col, Ty::Id, root, hops));
                    }
                }
                if *multi {
                    hops.push(Hop::BackMulti {
                        junction: format!("_rel_{src_coll}_{rel_field}"),
                        table: src_coll.to_owned(),
                    });
                } else {
                    hops.push(Hop::BackSingle {
                        via: rel_field.to_owned(),
                        table: src_coll.to_owned(),
                        bare: false,
                    });
                }
                coll = src_coll.to_owned();
            } else {
                return Err(RuleError::UnknownField(parts.join(".")));
            }
        }
        unreachable!("loop returns on the last segment")
    }
}

fn kind_ty(k: &FieldKind) -> Ty {
    match k {
        FieldKind::Scalar(t) => *t,
        FieldKind::Relation { .. } => Ty::Id,
    }
}

fn lit_ty(v: &Value) -> Result<Ty, RuleError> {
    match v {
        Value::Text(_) => Ok(Ty::Text),
        Value::Int(_) | Value::Real(_) => Ok(Ty::Number),
        _ => Err(RuleError::Type("cannot infer a type from null".into())),
    }
}

fn check_op(op: CmpOp, ty: Ty) -> Result<(), RuleError> {
    let ok = match op {
        CmpOp::Eq | CmpOp::Ne => true,
        CmpOp::Gt | CmpOp::Ge | CmpOp::Lt | CmpOp::Le => {
            matches!(ty, Ty::Text | Ty::Number | Ty::Date)
        }
        CmpOp::Like | CmpOp::NotLike => ty == Ty::Text,
    };
    if ok {
        Ok(())
    } else {
        Err(RuleError::Type(format!(
            "operator {op:?} does not apply to {ty:?}"
        )))
    }
}

fn convert_lit(v: Value, ty: Ty) -> Result<Value, RuleError> {
    let bad = |v: &Value| Err(RuleError::Type(format!("{v:?} is not a valid {ty:?}")));
    match (ty, v) {
        (_, Value::Null) => Ok(Value::Null),
        (Ty::Text, v @ Value::Text(_)) => Ok(v),
        (Ty::Number, v @ (Value::Int(_) | Value::Real(_))) => Ok(v),
        (Ty::Bool, Value::Int(b @ (0 | 1))) => Ok(Value::Int(b)),
        (Ty::Date, Value::Int(us)) => Ok(Value::Int(us)),
        (Ty::Date, Value::Text(s)) => match crate::parse_datetime_us(&s) {
            Some(us) => Ok(Value::Int(us)),
            None => bad(&Value::Text(s)),
        },
        // PocketBase idiom: `@request.auth.id != ""` means "signed in".
        (Ty::Id, Value::Text(s)) if s.is_empty() => Ok(Value::Null),
        (Ty::Id, Value::Text(s)) => match monolith_store::id::decode(&s) {
            Some(id) => Ok(Value::Blob(id.into())),
            None => bad(&Value::Text(s)),
        },
        (_, v) => bad(&v),
    }
}

fn is_null(t: &Term, params: &[Param]) -> bool {
    matches!(t, Term::Param { idx, .. } if params[*idx].src == Source::Lit(Value::Null))
}

impl Param {
    pub(crate) fn resolve(&self, ctx: &Ctx<'_>) -> Value {
        let raw = match &self.src {
            Source::Lit(v) => v.clone(),
            Source::AuthId => ctx.auth.map_or(Value::Null, |a| Value::Blob(a.id.into())),
            Source::AuthCollection => ctx
                .auth
                .map_or(Value::Null, |a| Value::Text(a.collection.clone())),
            Source::AuthClaim(name) => ctx
                .auth
                .and_then(|a| a.claims.iter().find(|(k, _)| k == name))
                .map_or(Value::Null, |(_, v)| v.clone()),
            Source::Body(slot) => ctx
                .body
                .and_then(|b| b.get(*slot))
                .cloned()
                .unwrap_or(Value::Null),
            Source::Now => Value::Int(ctx.now_us),
        };
        // Coerce request values to the checked type so SQL and memory never see a
        // cross-type comparison; a mismatch becomes NULL.
        let v = match (self.ty, raw) {
            (Ty::Text, v @ Value::Text(_)) => v,
            (Ty::Number, v @ (Value::Int(_) | Value::Real(_))) => v,
            (Ty::Bool | Ty::Date, v @ Value::Int(_)) => v,
            (Ty::Id, Value::Blob(b)) if b.len() == 16 => Value::Blob(b),
            (Ty::Id, Value::Text(s)) => {
                monolith_store::id::decode(&s).map_or(Value::Null, |id| Value::Blob(id.into()))
            }
            _ => Value::Null,
        };
        match v {
            Value::Text(s) if self.like => Value::Text(like_pattern(&s).into()),
            v => v,
        }
    }
}

fn like_pattern(s: &str) -> String {
    let mut p = String::with_capacity(s.len() + 2);
    p.push('%');
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            p.push('\\');
        }
        p.push(c);
    }
    p.push('%');
    p
}

fn renumber(n: &mut Node, pos: &[usize]) {
    let fix = |t: &mut Term| {
        if let Term::Param { idx, .. } = t {
            *idx = pos[*idx];
        }
    };
    match n {
        Node::True => {}
        Node::And(a, b) | Node::Or(a, b) => {
            renumber(a, pos);
            renumber(b, pos);
        }
        Node::Cmp { l, r, .. } => {
            fix(l);
            fix(r);
        }
        Node::In { l, list, .. } => {
            fix(l);
            list.iter_mut().for_each(fix);
        }
    }
}

fn emit_cmp(l: &Term, op: CmpOp, r: &Term, order: &mut Vec<usize>, out: &mut String) {
    term(l, order, out);
    out.push_str(match op {
        CmpOp::Eq => " IS ",
        CmpOp::Ne => " IS NOT ",
        CmpOp::Gt => " > ",
        CmpOp::Ge => " >= ",
        CmpOp::Lt => " < ",
        CmpOp::Le => " <= ",
        CmpOp::Like => " LIKE ",
        CmpOp::NotLike => " NOT LIKE ",
    });
    term(r, order, out);
    if matches!(op, CmpOp::Like | CmpOp::NotLike) {
        out.push_str(" ESCAPE '\\'");
    }
}

fn emit(
    n: &Node,
    joins: &[(String, String)],
    open_mask: u8,
    params: &[Param],
    order: &mut Vec<usize>,
    out: &mut String,
) {
    let needed = n.join_mask() & !open_mask;
    let open_here = match n {
        Node::And(a, b) => needed & a.join_mask() & b.join_mask(),
        Node::Cmp { .. } | Node::In { .. } => needed,
        Node::True | Node::Or(..) => 0,
    };
    if open_here != 0 {
        out.push_str("EXISTS (SELECT 1 FROM ");
        let mut first = true;
        for (k, (_, table)) in joins.iter().enumerate() {
            if open_here & (1 << k) != 0 {
                if !first {
                    out.push_str(", ");
                }
                first = false;
                let _ = write!(out, "\"{table}\" c{}", k + 1);
            }
        }
        out.push_str(" WHERE ");
        emit_inner(n, joins, open_mask | open_here, params, order, out);
        out.push(')');
    } else {
        emit_inner(n, joins, open_mask, params, order, out);
    }
}

fn emit_inner(
    n: &Node,
    joins: &[(String, String)],
    open_mask: u8,
    params: &[Param],
    order: &mut Vec<usize>,
    out: &mut String,
) {
    match n {
        Node::True => out.push('1'),
        Node::And(a, b) | Node::Or(a, b) => {
            out.push('(');
            emit(a, joins, open_mask, params, order, out);
            out.push_str(if matches!(n, Node::And(..)) {
                " AND "
            } else {
                " OR "
            });
            emit(b, joins, open_mask, params, order, out);
            out.push(')');
        }
        Node::Cmp {
            l,
            op,
            any,
            r,
            hop_root,
            hops,
        } => {
            let root_alias = hop_root.alias();
            let has_multi = hops.iter().any(Hop::is_multi);
            let bare_multi_null = hops.last().is_some_and(Hop::is_bare_multi)
                && (is_null(l, params) || is_null(r, params));
            if bare_multi_null {
                // `tags = null` / `tags = ''` (or `?!=`) tests whether the multi/back-relation is empty;
                // `tags != null` / `tags != ''` (or `?=`) tests whether it is non-empty.
                let want_empty = matches!((op, *any), (CmpOp::Eq, false) | (CmpOp::Ne, true));
                if want_empty {
                    out.push_str("NOT ");
                }
                with_hops(&root_alias, hops, out, |out| out.push('1'));
            } else if has_multi && !*any {
                // Doc 06 §3: on a multi-relation path without `?`, all related rows must
                // satisfy the condition and the set must be non-empty.
                out.push('(');
                with_hops(&root_alias, hops, out, |out| out.push('1'));
                out.push_str(" AND NOT ");
                with_hops(&root_alias, hops, out, |out| {
                    out.push_str("NOT (");
                    emit_cmp(l, *op, r, order, out);
                    out.push_str(" IS TRUE)");
                });
                out.push(')');
            } else {
                with_hops(&root_alias, hops, out, |out| {
                    emit_cmp(l, *op, r, order, out)
                });
            }
        }
        Node::In {
            l,
            list,
            hop_root,
            hops,
        } => {
            let root_alias = hop_root.alias();
            with_hops(&root_alias, hops, out, |out| {
                term(l, order, out);
                out.push_str(" IN (");
                for (i, t) in list.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    term(t, order, out);
                }
                out.push(')');
            });
        }
    }
}

/// Relation paths become one `EXISTS` semi-join, so lists never need DISTINCT.
fn with_hops(root_alias: &str, hops: &[Hop], out: &mut String, body: impl FnOnce(&mut String)) {
    if hops.is_empty() {
        return body(out);
    }
    out.push_str("EXISTS (SELECT 1 FROM ");
    for (i, h) in hops.iter().enumerate() {
        let n = i + 1;
        let prev = if i == 0 {
            root_alias.to_owned()
        } else {
            format!("r{i}")
        };
        if i > 0 {
            out.push_str(" JOIN ");
        }
        match h {
            Hop::Single { via, table } => {
                let _ = write!(out, "\"{table}\" r{n}");
                if i > 0 {
                    let _ = write!(out, " ON r{n}.\"id\" = {prev}.\"{via}\"");
                }
            }
            Hop::Multi { junction, table } => {
                let _ = write!(out, "\"{junction}\" j{n}");
                if i > 0 {
                    let _ = write!(out, " ON j{n}.\"owner\" = {prev}.\"id\"");
                }
                let _ = write!(
                    out,
                    " JOIN \"{table}\" r{n} ON r{n}.\"id\" = j{n}.\"target\""
                );
            }
            Hop::Junction { junction } => {
                let _ = write!(out, "\"{junction}\" j{n}");
                if i > 0 {
                    let _ = write!(out, " ON j{n}.\"owner\" = {prev}.\"id\"");
                }
            }
            Hop::BackSingle { via, table, .. } => {
                let _ = write!(out, "\"{table}\" r{n}");
                if i > 0 {
                    let _ = write!(out, " ON r{n}.\"{via}\" = {prev}.\"id\"");
                }
            }
            Hop::BackMulti { junction, table } => {
                let _ = write!(out, "\"{junction}\" j{n}");
                if i > 0 {
                    let _ = write!(out, " ON j{n}.\"target\" = {prev}.\"id\"");
                }
                let _ = write!(out, " JOIN \"{table}\" r{n} ON r{n}.\"id\" = j{n}.");
                out.push_str("\"owner\"");
            }
            Hop::BackJunction { junction } => {
                let _ = write!(out, "\"{junction}\" j{n}");
                if i > 0 {
                    let _ = write!(out, " ON j{n}.\"target\" = {prev}.\"id\"");
                }
            }
        }
    }
    match &hops[0] {
        Hop::Single { via, .. } => {
            let _ = write!(out, " WHERE r1.\"id\" = {root_alias}.\"{via}\" AND ");
        }
        Hop::Multi { .. } | Hop::Junction { .. } => {
            let _ = write!(out, " WHERE j1.\"owner\" = {root_alias}.\"id\" AND ");
        }
        Hop::BackSingle { via, .. } => {
            let _ = write!(out, " WHERE r1.\"{via}\" = {root_alias}.\"id\" AND ");
        }
        Hop::BackMulti { .. } | Hop::BackJunction { .. } => {
            let _ = write!(out, " WHERE j1.\"target\" = {root_alias}.\"id\" AND ");
        }
    }
    body(out);
    out.push(')');
}

fn term(t: &Term, order: &mut Vec<usize>, out: &mut String) {
    let (lower, s) = match t {
        Term::Col {
            root,
            depth,
            junction,
            column,
            lower,
            ..
        } => {
            let alias = if *depth == 0 {
                root.alias()
            } else if *junction {
                format!("j{depth}")
            } else {
                format!("r{depth}")
            };
            (*lower, format!("{alias}.\"{column}\""))
        }
        Term::Param { idx, lower } => {
            order.push(*idx);
            (*lower, "?".to_owned())
        }
    };
    if lower {
        let _ = write!(out, "lower({s})");
    } else {
        out.push_str(&s);
    }
}
