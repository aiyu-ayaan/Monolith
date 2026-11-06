//! Lexer and parser for the rules/filter language (doc 06 §1).

use crate::RuleError;

pub const MAX_TEXT: usize = 4096;
pub const MAX_NODES: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub enum Lit {
    Str(String),
    Int(i64),
    Real(f64),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Lower,
    Isset,
    Changed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    Lit(Lit),
    /// `a.b.c`, `@request.auth.id`, `@now`; segments split on `.`.
    Path {
        parts: Vec<String>,
        modifier: Option<Modifier>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Like,
    NotLike,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Cmp {
        l: Operand,
        op: CmpOp,
        any: bool,
        r: Operand,
    },
    In(Operand, Vec<Operand>),
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    LParen,
    RParen,
    Comma,
    And,
    Or,
    In,
    Op(CmpOp),
    AnyOp(CmpOp),
    Lit(Lit),
    Path(String),
}

fn err(pos: usize, msg: impl Into<String>) -> RuleError {
    RuleError::Syntax {
        pos,
        msg: msg.into(),
    }
}

fn cmp_tok(b: &[u8], i: usize) -> Option<(CmpOp, usize)> {
    let two = b.get(i..i + 2).unwrap_or(&[]);
    Some(match two {
        b"!=" => (CmpOp::Ne, 2),
        b"!~" => (CmpOp::NotLike, 2),
        b">=" => (CmpOp::Ge, 2),
        b"<=" => (CmpOp::Le, 2),
        _ => match *b.get(i)? {
            b'=' => (CmpOp::Eq, 1),
            b'>' => (CmpOp::Gt, 1),
            b'<' => (CmpOp::Lt, 1),
            b'~' => (CmpOp::Like, 1),
            _ => return None,
        },
    })
}

fn lex(src: &str) -> Result<Vec<(usize, Tok)>, RuleError> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let start = i;
        let two = b.get(i..i + 2).unwrap_or(&[]);
        let tok = match c {
            b' ' | b'\t' | b'\n' | b'\r' => {
                i += 1;
                continue;
            }
            b'(' => {
                i += 1;
                Tok::LParen
            }
            b')' => {
                i += 1;
                Tok::RParen
            }
            b',' => {
                i += 1;
                Tok::Comma
            }
            _ if two == b"&&" => {
                i += 2;
                Tok::And
            }
            _ if two == b"||" => {
                i += 2;
                Tok::Or
            }
            b'?' => {
                let (op, n) = cmp_tok(b, i + 1)
                    .ok_or_else(|| err(i, "expected comparison operator after '?'"))?;
                i += 1 + n;
                Tok::AnyOp(op)
            }
            b'=' | b'!' | b'<' | b'>' | b'~' => {
                let (op, n) = cmp_tok(b, i).ok_or_else(|| err(i, "unexpected '!'"))?;
                i += n;
                Tok::Op(op)
            }
            b'\'' | b'"' => {
                i += 1;
                let mut s = String::new();
                loop {
                    let Some(&ch) = b.get(i) else {
                        return Err(err(start, "unterminated string"));
                    };
                    if ch == c {
                        i += 1;
                        break;
                    }
                    if ch == b'\\' && i + 1 < b.len() {
                        i += 1;
                    }
                    // Push whole UTF-8 chars; `i` is always on a char boundary here.
                    let ch_len = src[i..].chars().next().map_or(1, char::len_utf8);
                    s.push_str(&src[i..i + ch_len]);
                    i += ch_len;
                }
                Tok::Lit(Lit::Str(s))
            }
            b'0'..=b'9' | b'-' => {
                i += 1;
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                    i += 1;
                }
                let text = &src[start..i];
                if let Ok(n) = text.parse::<i64>() {
                    Tok::Lit(Lit::Int(n))
                } else {
                    match text.parse::<f64>() {
                        Ok(f) if f.is_finite() => Tok::Lit(Lit::Real(f)),
                        _ => return Err(err(start, format!("bad number '{text}'"))),
                    }
                }
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'@' => {
                i += 1;
                while i < b.len()
                    && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'_' | b'.' | b':'))
                {
                    i += 1;
                }
                match &src[start..i] {
                    "true" => Tok::Lit(Lit::Bool(true)),
                    "false" => Tok::Lit(Lit::Bool(false)),
                    "null" => Tok::Lit(Lit::Null),
                    "in" => Tok::In,
                    p => Tok::Path(p.to_owned()),
                }
            }
            _ => {
                return Err(err(
                    i,
                    format!(
                        "unexpected character '{}'",
                        src[i..].chars().next().unwrap_or('?')
                    ),
                ));
            }
        };
        out.push((start, tok));
    }
    Ok(out)
}

struct Parser {
    toks: Vec<(usize, Tok)>,
    pos: usize,
    nodes: usize,
    end: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|(_, t)| t)
    }

    fn at(&self) -> usize {
        self.toks.get(self.pos).map_or(self.end, |(p, _)| *p)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).map(|(_, t)| t.clone());
        self.pos += 1;
        t
    }

    fn node(&mut self) -> Result<(), RuleError> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(RuleError::Limit("too many expression nodes"));
        }
        Ok(())
    }

    // || binds looser than &&.
    fn or(&mut self) -> Result<Expr, RuleError> {
        let mut l = self.and()?;
        while self.peek() == Some(&Tok::Or) {
            self.next();
            self.node()?;
            l = Expr::Or(Box::new(l), Box::new(self.and()?));
        }
        Ok(l)
    }

    fn and(&mut self) -> Result<Expr, RuleError> {
        let mut l = self.term()?;
        while self.peek() == Some(&Tok::And) {
            self.next();
            self.node()?;
            l = Expr::And(Box::new(l), Box::new(self.term()?));
        }
        Ok(l)
    }

    fn term(&mut self) -> Result<Expr, RuleError> {
        self.node()?;
        if self.peek() == Some(&Tok::LParen) {
            self.next();
            let e = self.or()?;
            return match self.next() {
                Some(Tok::RParen) => Ok(e),
                _ => Err(err(self.at(), "expected ')'")),
            };
        }
        let l = self.operand()?;
        let at = self.at();
        match self.next() {
            Some(Tok::Op(op)) => Ok(Expr::Cmp {
                l,
                op,
                any: false,
                r: self.operand()?,
            }),
            Some(Tok::AnyOp(op)) => Ok(Expr::Cmp {
                l,
                op,
                any: true,
                r: self.operand()?,
            }),
            Some(Tok::In) => {
                if self.next() != Some(Tok::LParen) {
                    return Err(err(self.at(), "expected '(' after in"));
                }
                let mut list = vec![self.operand()?];
                loop {
                    match self.next() {
                        Some(Tok::Comma) => list.push(self.operand()?),
                        Some(Tok::RParen) => break,
                        _ => return Err(err(self.at(), "expected ',' or ')'")),
                    }
                }
                Ok(Expr::In(l, list))
            }
            _ => Err(err(at, "expected comparison operator")),
        }
    }

    fn operand(&mut self) -> Result<Operand, RuleError> {
        self.node()?;
        let at = self.at();
        match self.next() {
            Some(Tok::Lit(l)) => Ok(Operand::Lit(l)),
            Some(Tok::Path(p)) => {
                let mut parts: Vec<String> = p.split('.').map(str::to_owned).collect();
                if parts.iter().any(String::is_empty) {
                    return Err(err(at, format!("bad identifier '{p}'")));
                }
                let is_coll = parts.first().is_some_and(|s| s == "@collection");
                // Extract trailing `:modifier` from the last segment (unless it is `@collection.name:alias` with no field).
                let modifier = if !(is_coll && parts.len() == 2)
                    && let Some((base, m)) = parts.last().unwrap().clone().split_once(':')
                {
                    if base.is_empty() {
                        return Err(err(at, format!("bad identifier '{p}'")));
                    }
                    let modif = match m {
                        "lower" => Modifier::Lower,
                        "isset" => Modifier::Isset,
                        "changed" => Modifier::Changed,
                        _ => return Err(RuleError::Unsupported(leak_modifier(m))),
                    };
                    *parts.last_mut().unwrap() = base.to_owned();
                    Some(modif)
                } else {
                    None
                };
                for (i, seg) in parts.iter().enumerate() {
                    if seg.contains(':') && !(is_coll && i == 1) {
                        return Err(err(at, format!("bad identifier '{p}'")));
                    }
                }
                Ok(Operand::Path { parts, modifier })
            }
            _ => Err(err(at, "expected a value or field")),
        }
    }
}

fn leak_modifier(m: &str) -> &'static str {
    match m {
        "length" | "each" => "modifiers :length and :each",
        _ => "unknown modifier",
    }
}

/// Parses a non-empty rule. Callers handle `""` (public) and `null` (superusers only).
pub fn parse(src: &str) -> Result<Expr, RuleError> {
    if src.len() > MAX_TEXT {
        return Err(RuleError::Limit("rule text longer than 4096 bytes"));
    }
    let toks = lex(src)?;
    let mut p = Parser {
        toks,
        pos: 0,
        nodes: 0,
        end: src.len(),
    };
    let e = p.or()?;
    if p.pos < p.toks.len() {
        return Err(err(p.at(), "unexpected trailing input"));
    }
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(p: &str) -> Operand {
        Operand::Path {
            parts: p.split('.').map(str::to_owned).collect(),
            modifier: None,
        }
    }

    #[test]
    fn precedence_and_literals() {
        let e = parse("a = 1 || b ?!= 'x' && c >= -2.5").unwrap();
        assert_eq!(
            e,
            Expr::Or(
                Box::new(Expr::Cmp {
                    l: path("a"),
                    op: CmpOp::Eq,
                    any: false,
                    r: Operand::Lit(Lit::Int(1)),
                }),
                Box::new(Expr::And(
                    Box::new(Expr::Cmp {
                        l: path("b"),
                        op: CmpOp::Ne,
                        any: true,
                        r: Operand::Lit(Lit::Str("x".into())),
                    }),
                    Box::new(Expr::Cmp {
                        l: path("c"),
                        op: CmpOp::Ge,
                        any: false,
                        r: Operand::Lit(Lit::Real(-2.5)),
                    }),
                )),
            )
        );
    }

    #[test]
    fn strings_paths_in_modifiers() {
        let e =
            parse(r#"(@request.auth.id = owner) && status in ("a", 'b\'c') && name:lower ~ "é""#)
                .unwrap();
        let Expr::And(l, r) = e else { panic!() };
        let Expr::And(_, inn) = *l else { panic!() };
        assert_eq!(
            *inn,
            Expr::In(
                path("status"),
                vec![
                    Operand::Lit(Lit::Str("a".into())),
                    Operand::Lit(Lit::Str("b'c".into()))
                ]
            )
        );
        assert_eq!(
            *r,
            Expr::Cmp {
                l: Operand::Path {
                    parts: vec!["name".into()],
                    modifier: Some(Modifier::Lower)
                },
                op: CmpOp::Like,
                any: false,
                r: Operand::Lit(Lit::Str("é".into()))
            }
        );
        assert_eq!(
            parse("@collection.members:m1.name:lower = 'x'").unwrap(),
            Expr::Cmp {
                l: Operand::Path {
                    parts: vec!["@collection".into(), "members:m1".into(), "name".into()],
                    modifier: Some(Modifier::Lower)
                },
                op: CmpOp::Eq,
                any: false,
                r: Operand::Lit(Lit::Str("x".into()))
            }
        );
        assert_eq!(
            parse("@request.body.role:changed = false").unwrap(),
            Expr::Cmp {
                l: Operand::Path {
                    parts: vec!["@request".into(), "body".into(), "role".into()],
                    modifier: Some(Modifier::Changed)
                },
                op: CmpOp::Eq,
                any: false,
                r: Operand::Lit(Lit::Bool(false))
            }
        );
    }

    #[test]
    fn errors() {
        for bad in [
            "a =",
            "a = 'x",
            "(a = 1",
            "a = 1 b",
            "a ! 1",
            "a ? 1",
            "a = 1 &&",
            "a.. = 1",
            "a:length = 1",
            "a:b.c = 1",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        assert!(matches!(
            parse("a = 1 && ".repeat(100).trim_end_matches(" && ")),
            Err(RuleError::Limit(_))
        ));
    }
}
