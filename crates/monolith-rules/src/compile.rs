//! Resolve + type-check the AST against a schema, then emit SQL (doc 06 §2–§3).

use std::fmt::Write;

use crate::parse::{self, CmpOp, Expr, Lit, Modifier, Operand};
use crate::{Class, Ctx, FieldKind, Mode, RuleError, SchemaView, Ty, Value};

pub const MAX_HOPS: usize = 4;

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

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Term {
    /// `depth` 0 is the queried table (alias `t`), n is relation hop `rn`.
    Col {
        depth: usize,
        column: String,
        slot: usize,
        lower: bool,
    },
    Param {
        idx: usize,
        lower: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Hop {
    /// Column on the previous alias holding the related id.
    pub via: String,
    pub table: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node {
    True,
    And(Box<Node>, Box<Node>),
    Or(Box<Node>, Box<Node>),
    Cmp {
        l: Term,
        op: CmpOp,
        r: Term,
        hops: Vec<Hop>,
    },
    In {
        l: Term,
        list: Vec<Term>,
        hops: Vec<Hop>,
    },
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
        class: Class::Const,
    };
    let mut node = c.expr(&ast)?;
    let mut sql = String::with_capacity(src.len() * 2);
    let mut order = Vec::with_capacity(c.params.len());
    emit(&node, &mut order, &mut sql);
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
    Typed(Term, Ty, Vec<Hop>),
    /// Literal or claim: takes the other side's type.
    Flex(Source, bool),
}

struct Checker<'a> {
    schema: &'a dyn SchemaView,
    collection: &'a str,
    mode: Mode,
    params: Vec<Param>,
    class: Class,
}

impl Checker<'_> {
    fn expr(&mut self, e: &Expr) -> Result<Node, RuleError> {
        Ok(match e {
            Expr::And(a, b) => Node::And(Box::new(self.expr(a)?), Box::new(self.expr(b)?)),
            Expr::Or(a, b) => Node::Or(Box::new(self.expr(a)?), Box::new(self.expr(b)?)),
            Expr::Cmp(l, op, r) => self.cmp(l, *op, r)?,
            Expr::In(l, list) => self.in_list(l, list)?,
        })
    }

    fn see(&mut self, c: Class) {
        self.class = self.class.max(c);
    }

    fn cmp(&mut self, l: &Operand, op: CmpOp, r: &Operand) -> Result<Node, RuleError> {
        let (l, r) = (self.operand(l)?, self.operand(r)?);
        if matches!(op, CmpOp::Like | CmpOp::NotLike)
            && !matches!(r, Pre::Flex(..) | Pre::Typed(Term::Param { .. }, ..))
        {
            return Err(RuleError::Unsupported(
                "the right side of ~ must be a value, not a field",
            ));
        }
        let ty = match (&l, &r) {
            (Pre::Typed(_, a, _), Pre::Typed(_, b, _)) if a != b => {
                return Err(RuleError::Type(format!("cannot compare {a:?} with {b:?}")));
            }
            (Pre::Typed(_, t, _), _) | (_, Pre::Typed(_, t, _)) => *t,
            (Pre::Flex(Source::Lit(v), _), _) | (_, Pre::Flex(Source::Lit(v), _)) => lit_ty(v)?,
            _ => {
                return Err(RuleError::Type(
                    "cannot infer the type of a claim compared with a claim".into(),
                ));
            }
        };
        check_op(op, ty)?;
        let like = matches!(op, CmpOp::Like | CmpOp::NotLike);
        let (l, lh) = self.finish(l, ty, false)?;
        let (r, rh) = self.finish(r, ty, like)?;
        if (is_null(&l, &self.params) || is_null(&r, &self.params))
            && !matches!(op, CmpOp::Eq | CmpOp::Ne)
        {
            return Err(RuleError::Type("null only works with = and !=".into()));
        }
        Ok(Node::Cmp {
            l,
            op,
            r,
            hops: self.one_path(lh, rh)?,
        })
    }

    fn in_list(&mut self, l: &Operand, list: &[Operand]) -> Result<Node, RuleError> {
        let l = self.operand(l)?;
        let Pre::Typed(_, ty, _) = &l else {
            return Err(RuleError::Type(
                "the left side of 'in' must be a field or typed value".into(),
            ));
        };
        let ty = *ty;
        let (l, hops) = self.finish(l, ty, false)?;
        let mut terms = Vec::with_capacity(list.len());
        for o in list {
            let pre = self.operand(o)?;
            if !matches!(pre, Pre::Flex(..) | Pre::Typed(Term::Param { .. }, ..)) {
                return Err(RuleError::Unsupported("'in' lists hold values, not fields"));
            }
            if let Pre::Typed(_, t, _) = &pre
                && *t != ty
            {
                return Err(RuleError::Type(format!(
                    "'in' list item is {t:?}, expected {ty:?}"
                )));
            }
            let (t, _) = self.finish(pre, ty, false)?;
            if is_null(&t, &self.params) {
                return Err(RuleError::Type("null is not allowed in 'in' lists".into()));
            }
            terms.push(t);
        }
        Ok(Node::In {
            l,
            list: terms,
            hops,
        })
    }

    fn one_path(&self, a: Vec<Hop>, b: Vec<Hop>) -> Result<Vec<Hop>, RuleError> {
        match (a.is_empty(), b.is_empty()) {
            (_, true) => Ok(a),
            (true, false) => Ok(b),
            _ => Err(RuleError::Unsupported(
                "relation paths on both sides of one comparison",
            )),
        }
    }

    /// Turns a pre-operand into a term of type `ty`, converting literals.
    fn finish(&mut self, p: Pre, ty: Ty, like: bool) -> Result<(Term, Vec<Hop>), RuleError> {
        match p {
            Pre::Typed(Term::Param { idx, lower }, _, hops) => {
                self.params[idx].like = like;
                Ok((Term::Param { idx, lower }, hops))
            }
            Pre::Typed(t, _, hops) => Ok((t, hops)),
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
                vec![],
            ))
        };
        let p: Vec<&str> = parts.iter().map(String::as_str).collect();
        if p[0].starts_with('@') {
            self.see(Class::AuthOnly);
            return match p.as_slice() {
                ["@now"] => typed_param(self, Source::Now, Ty::Date),
                ["@request", "auth", "id"] => typed_param(self, Source::AuthId, Ty::Id),
                ["@request", "auth", "collectionName"] => {
                    typed_param(self, Source::AuthCollection, Ty::Text)
                }
                ["@request", "auth", claim] => {
                    Ok(Pre::Flex(Source::AuthClaim((*claim).into()), lower))
                }
                ["@request", "body", name] => {
                    let f = self
                        .schema
                        .field(self.collection, name)
                        .ok_or_else(|| RuleError::UnknownField(format!("@request.body.{name}")))?;
                    typed_param(self, Source::Body(f.slot), kind_ty(&f.kind))
                }
                ["@collection", ..] => {
                    Err(RuleError::Unsupported("@collection joins (next stage)"))
                }
                _ => Err(RuleError::UnknownField(parts.join("."))),
            };
        }
        if p.len() > MAX_HOPS + 1 {
            return Err(RuleError::Limit("more than 4 relation hops"));
        }
        let mut coll = self.collection.to_owned();
        let mut hops = Vec::new();
        for (i, name) in p.iter().enumerate() {
            let f = self
                .schema
                .field(&coll, name)
                .ok_or_else(|| RuleError::UnknownField(parts[..=i].join(".")))?;
            if f.hidden && self.mode == Mode::Filter {
                return Err(RuleError::UnknownField(parts[..=i].join(".")));
            }
            let last = i + 1 == p.len();
            match (&f.kind, last) {
                (kind, true) => {
                    let ty = kind_ty(kind);
                    if lower && ty != Ty::Text {
                        return Err(RuleError::Type(":lower only applies to text".into()));
                    }
                    self.see(if hops.is_empty() {
                        Class::RowLocal
                    } else {
                        Class::Relational
                    });
                    let col = Term::Col {
                        depth: hops.len(),
                        column: (*name).to_owned(),
                        slot: f.slot,
                        lower,
                    };
                    return Ok(Pre::Typed(col, ty, hops));
                }
                (FieldKind::Relation { target }, false) => {
                    hops.push(Hop {
                        via: (*name).to_owned(),
                        table: target.clone(),
                    });
                    coll = target.clone();
                }
                (FieldKind::Scalar(_), false) => {
                    return Err(RuleError::Type(format!("'{name}' is not a relation")));
                }
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

fn emit(n: &Node, order: &mut Vec<usize>, out: &mut String) {
    match n {
        Node::True => out.push('1'),
        Node::And(a, b) | Node::Or(a, b) => {
            out.push('(');
            emit(a, order, out);
            out.push_str(if matches!(n, Node::And(..)) {
                " AND "
            } else {
                " OR "
            });
            emit(b, order, out);
            out.push(')');
        }
        Node::Cmp { l, op, r, hops } => with_hops(hops, out, |out| {
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
        }),
        Node::In { l, list, hops } => with_hops(hops, out, |out| {
            term(l, order, out);
            out.push_str(" IN (");
            for (i, t) in list.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                term(t, order, out);
            }
            out.push(')');
        }),
    }
}

/// Relation paths become one `EXISTS` semi-join, so lists never need DISTINCT.
fn with_hops(hops: &[Hop], out: &mut String, body: impl FnOnce(&mut String)) {
    if hops.is_empty() {
        return body(out);
    }
    out.push_str("EXISTS (SELECT 1 FROM ");
    for (i, h) in hops.iter().enumerate() {
        let n = i + 1;
        if i > 0 {
            out.push_str(" JOIN ");
        }
        let _ = write!(out, "\"{}\" r{n}", h.table);
        if i > 0 {
            let _ = write!(out, " ON r{n}.\"id\" = r{}.\"{}\"", i, h.via);
        }
    }
    let _ = write!(out, " WHERE r1.\"id\" = t.\"{}\" AND ", hops[0].via);
    body(out);
    out.push(')');
}

fn term(t: &Term, order: &mut Vec<usize>, out: &mut String) {
    let (lower, s) = match t {
        Term::Col {
            depth,
            column,
            lower,
            ..
        } => {
            let alias = if *depth == 0 {
                "t".to_owned()
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
