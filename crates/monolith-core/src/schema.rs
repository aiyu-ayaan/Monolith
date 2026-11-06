//! Collection definitions (wire + persisted form) and the compiled per-version schema.

use std::collections::HashMap;
use std::sync::Arc;

use monolith_rules::{Compiled, FieldInfo, FieldKind, Mode, SchemaView, Ty, compile};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};

pub const MAX_FIELDS: usize = 61;
pub const MAX_TEXT_DEFAULT: usize = 65_536;
/// System columns that occupy the first row slots, in SELECT order.
pub const SYSTEM: [&str; 4] = ["id", "created", "updated", "_v"];
pub const FIRST_FIELD_SLOT: usize = SYSTEM.len();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    Text,
    Email,
    Url,
    Number,
    Bool,
    Date,
    Select,
    Relation,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OnDelete {
    #[default]
    Restrict,
    Cascade,
    SetNull,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FieldDef {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: FieldType,
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub only_int: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_delete: Option<OnDelete>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_select: Option<u32>,
}

impl FieldDef {
    /// Rules type, or `None` for fields the rules language cannot see (json).
    pub fn ty(&self) -> Option<Ty> {
        Some(match self.kind {
            FieldType::Text | FieldType::Email | FieldType::Url | FieldType::Select => Ty::Text,
            FieldType::Number => Ty::Number,
            FieldType::Bool => Ty::Bool,
            FieldType::Date => Ty::Date,
            FieldType::Relation => Ty::Id,
            FieldType::Json => return None,
        })
    }

    pub fn is_multi_rel(&self) -> bool {
        self.kind == FieldType::Relation && self.max_select.unwrap_or(1) > 1
    }

    pub fn junction_table(&self, owner_coll: &str) -> String {
        format!("_rel_{owner_coll}_{}", self.name)
    }

    pub fn sql_type(&self) -> &'static str {
        match self.kind {
            FieldType::Number if self.only_int => "INTEGER",
            FieldType::Number => "REAL",
            FieldType::Bool | FieldType::Date => "INTEGER",
            FieldType::Relation => "BLOB",
            // shortcut: json stored as validated TEXT; JSONB BLOB per doc 05 §5 later.
            _ => "TEXT",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    #[serde(default)]
    pub list: Option<String>,
    #[serde(default)]
    pub view: Option<String>,
    #[serde(default)]
    pub create: Option<String>,
    #[serde(default)]
    pub update: Option<String>,
    #[serde(default)]
    pub delete: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexDef {
    pub fields: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub unique: bool,
}

impl IndexDef {
    pub fn sql_name(&self, table: &str) -> String {
        format!(
            "idx_{table}_{}{}",
            self.fields.join("_"),
            if self.unique { "_u" } else { "" }
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CollectionType {
    #[default]
    Base,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionDef {
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: CollectionType,
    pub fields: Vec<FieldDef>,
    #[serde(default)]
    pub rules: Rules,
    #[serde(default)]
    pub indexes: Vec<IndexDef>,
}

impl CollectionDef {
    pub fn field(&self, name: &str) -> Option<&FieldDef> {
        self.fields.iter().find(|f| f.name == name)
    }
}

/// `^[a-z][a-z0-9_]{0,62}$`; safe to quote as an SQL identifier.
pub fn valid_name(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 63
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

fn bad(msg: impl Into<String>) -> CoreError {
    CoreError::BadRequest(msg.into())
}

/// Shape checks that need no other collections.
pub fn validate_def(def: &CollectionDef) -> CoreResult<()> {
    if !valid_name(&def.name) {
        return Err(bad(format!("invalid collection name '{}'", def.name)));
    }
    if def.fields.len() > MAX_FIELDS {
        return Err(bad(format!("at most {MAX_FIELDS} fields")));
    }
    let mut seen = std::collections::HashSet::new();
    for f in &def.fields {
        if !valid_name(&f.name) || SYSTEM.contains(&f.name.as_str()) {
            return Err(bad(format!("invalid or reserved field name '{}'", f.name)));
        }
        if !seen.insert(f.name.as_str()) {
            return Err(bad(format!("duplicate field '{}'", f.name)));
        }
        let is = |k: FieldType| f.kind == k;
        if f.only_int && !is(FieldType::Number) {
            return Err(bad(format!("{}: onlyInt applies to number fields", f.name)));
        }
        if is(FieldType::Select) == f.values.is_empty() {
            return Err(bad(format!(
                "{}: select fields need values; other types must not set them",
                f.name
            )));
        }
        if is(FieldType::Relation) != f.collection.is_some()
            || ((f.on_delete.is_some() || f.max_select.is_some()) && !is(FieldType::Relation))
        {
            return Err(bad(format!(
                "{}: collection/onDelete/maxSelect apply to relation fields only",
                f.name
            )));
        }
        if f.max_select == Some(0) || f.max_select.is_some_and(|m| m > 1000) {
            return Err(bad(format!(
                "{}: maxSelect must be within 1..=1000",
                f.name
            )));
        }
        if f.on_delete == Some(OnDelete::SetNull) && (f.required || f.is_multi_rel()) {
            return Err(bad(format!(
                "{}: onDelete setNull conflicts with required or multi-relation fields",
                f.name
            )));
        }
        if let (Some(a), Some(b)) = (f.min, f.max)
            && a > b
        {
            return Err(bad(format!("{}: min > max", f.name)));
        }
    }
    for ix in &def.indexes {
        if ix.fields.is_empty() {
            return Err(bad("index needs at least one field"));
        }
        for name in &ix.fields {
            if !SYSTEM.contains(&name.as_str()) {
                match def.field(name) {
                    None => return Err(bad(format!("index field '{name}' does not exist"))),
                    Some(f) if f.is_multi_rel() => {
                        return Err(bad(format!(
                            "multi-relation field '{name}' is already indexed by its junction table"
                        )));
                    }
                    Some(_) => {}
                }
            }
        }
    }
    Ok(())
}

/// Compiled rules; `None` means superusers only.
#[derive(Debug)]
pub struct CompiledRules {
    pub list: Option<Compiled>,
    pub view: Option<Compiled>,
    pub create: Option<Compiled>,
    pub update: Option<Compiled>,
    pub delete: Option<Compiled>,
}

/// SQL text fixed per schema version, so `prepare_cached` hits on every request.
#[derive(Debug)]
pub struct Templates {
    /// Quoted column list in slot order (no `_hlc`).
    pub cols: String,
    pub view: Arc<str>,
    pub view_su: Arc<str>,
    pub insert: Arc<str>,
    pub insert_su: Arc<str>,
    pub update: Arc<str>,
    pub update_su: Arc<str>,
    pub delete: Arc<str>,
    pub delete_su: Arc<str>,
}

#[derive(Debug)]
pub struct Collection {
    pub def: CollectionDef,
    /// Numeric id used in the change log.
    pub cid: u32,
    pub rules: CompiledRules,
    pub sql: Templates,
    /// `"name":` per slot, JSON-escaped once.
    pub json_keys: Vec<String>,
    /// Indices into `def.fields` for multi-relation fields (`max_select > 1`), with
    /// `(field_idx, slot, delete_sql, insert_sql)`.
    pub multi_rels: Vec<(usize, usize, Arc<str>, Arc<str>)>,
    by_name: HashMap<String, usize>,
}

impl Collection {
    /// Slot of a user field or system column.
    pub fn slot(&self, name: &str) -> Option<usize> {
        self.by_name.get(name).copied()
    }

    /// The user field at `slot`, `None` for system columns.
    pub fn field_at(&self, slot: usize) -> Option<&FieldDef> {
        slot.checked_sub(FIRST_FIELD_SLOT)
            .and_then(|i| self.def.fields.get(i))
    }

    pub fn slots(&self) -> usize {
        FIRST_FIELD_SLOT + self.def.fields.len()
    }
}

#[derive(Debug, Default)]
pub struct Schema {
    pub version: u64,
    pub collections: HashMap<String, Arc<Collection>>,
}

/// Definitions only: enough for the rules type checker.
struct Defs<'a>(&'a HashMap<String, (u32, CollectionDef)>);

fn field_info(def: &CollectionDef, name: &str) -> Option<FieldInfo> {
    let sys = |slot, ty| {
        Some(FieldInfo {
            slot,
            kind: FieldKind::Scalar(ty),
            hidden: false,
        })
    };
    match name {
        "id" => return sys(0, Ty::Id),
        "created" => return sys(1, Ty::Date),
        "updated" => return sys(2, Ty::Date),
        _ => {}
    }
    let (i, f) = def
        .fields
        .iter()
        .enumerate()
        .find(|(_, f)| f.name == name)?;
    let kind = match (&f.collection, f.ty()?) {
        (Some(target), _) => FieldKind::Relation {
            target: target.clone(),
            multi: f.is_multi_rel(),
        },
        (None, ty) => FieldKind::Scalar(ty),
    };
    Some(FieldInfo {
        slot: FIRST_FIELD_SLOT + i,
        kind,
        hidden: f.hidden,
    })
}

impl SchemaView for Defs<'_> {
    fn field(&self, collection: &str, name: &str) -> Option<FieldInfo> {
        field_info(&self.0.get(collection)?.1, name)
    }
}

impl SchemaView for Schema {
    fn field(&self, collection: &str, name: &str) -> Option<FieldInfo> {
        field_info(&self.collections.get(collection)?.def, name)
    }
}

impl Schema {
    /// Validates cross-collection references and compiles every rule and template.
    pub fn build(version: u64, defs: HashMap<String, (u32, CollectionDef)>) -> CoreResult<Self> {
        for (_, def) in defs.values() {
            validate_def(def)?;
            for f in &def.fields {
                if let Some(target) = &f.collection
                    && !defs.contains_key(target)
                {
                    return Err(bad(format!(
                        "{}.{}: unknown collection '{target}'",
                        def.name, f.name
                    )));
                }
            }
        }
        let view = Defs(&defs);
        let mut collections = HashMap::with_capacity(defs.len());
        for (name, (cid, def)) in &defs {
            let rule = |r: &Option<String>, what: &str| -> CoreResult<Option<Compiled>> {
                r.as_deref()
                    .map(|src| compile(&view, name, src, Mode::Rule))
                    .transpose()
                    .map_err(|e| bad(format!("{name} {what} rule: {e}")))
            };
            let rules = CompiledRules {
                list: rule(&def.rules.list, "list")?,
                view: rule(&def.rules.view, "view")?,
                create: rule(&def.rules.create, "create")?,
                update: rule(&def.rules.update, "update")?,
                delete: rule(&def.rules.delete, "delete")?,
            };
            let mut by_name: HashMap<String, usize> = SYSTEM
                .iter()
                .enumerate()
                .map(|(i, s)| ((*s).to_owned(), i))
                .collect();
            for (i, f) in def.fields.iter().enumerate() {
                by_name.insert(f.name.clone(), FIRST_FIELD_SLOT + i);
            }
            let names: Vec<&str> = SYSTEM
                .iter()
                .copied()
                .chain(def.fields.iter().map(|f| f.name.as_str()))
                .collect();
            let json_keys = names
                .iter()
                .map(|n| {
                    let mut k = Vec::new();
                    crate::codec::write_str(&mut k, n);
                    k.push(b':');
                    String::from_utf8(k).expect("escaped JSON is UTF-8")
                })
                .collect();
            let multi_rels = def
                .fields
                .iter()
                .enumerate()
                .filter(|(_, f)| f.is_multi_rel())
                .map(|(i, f)| {
                    let j = f.junction_table(name);
                    let del: Arc<str> = format!("DELETE FROM \"{j}\" WHERE \"owner\" = ?").into();
                    let ins: Arc<str> = format!(
                        "INSERT INTO \"{j}\" (\"owner\", \"target\", \"pos\") VALUES (?, ?, ?)"
                    )
                    .into();
                    (i, FIRST_FIELD_SLOT + i, del, ins)
                })
                .collect();
            let sql = templates(def, &rules);
            collections.insert(
                name.clone(),
                Arc::new(Collection {
                    def: def.clone(),
                    cid: *cid,
                    rules,
                    sql,
                    json_keys,
                    multi_rels,
                    by_name,
                }),
            );
        }
        Ok(Schema {
            version,
            collections,
        })
    }

    pub fn get(&self, name: &str) -> CoreResult<&Arc<Collection>> {
        self.collections
            .get(name)
            .ok_or(CoreError::NotFound("collection"))
    }
}

fn templates(def: &CollectionDef, rules: &CompiledRules) -> Templates {
    let t = &def.name;
    let q = |s: &str| format!("\"{s}\"");
    // Multi-relation fields live in `_rel_*` junction tables; in the main table SELECT
    // they occupy their slot as `NULL AS "<name>"` so slot indices stay aligned.
    let select_user: Vec<String> = def
        .fields
        .iter()
        .map(|f| {
            if f.is_multi_rel() {
                format!("NULL AS {}", q(&f.name))
            } else {
                q(&f.name)
            }
        })
        .collect();
    let cols = SYSTEM
        .iter()
        .map(|s| q(s))
        .chain(select_user)
        .collect::<Vec<_>>()
        .join(", ");
    let table_user: Vec<String> = def
        .fields
        .iter()
        .filter(|f| !f.is_multi_rel())
        .map(|f| q(&f.name))
        .collect();
    let rule_sql = |r: &Option<Compiled>| r.as_ref().map_or("0".to_owned(), |c| c.sql.clone());

    let view = |r: &str| -> Arc<str> {
        format!("SELECT {cols} FROM \"{t}\" AS t WHERE t.\"id\" = ? AND ({r})").into()
    };

    // Insert params: id, created, updated, _v, _hlc, single-value user fields..., then rule binds.
    let ins_cols = SYSTEM
        .iter()
        .map(|s| q(s))
        .chain([q("_hlc")])
        .chain(table_user.iter().cloned())
        .collect::<Vec<_>>();
    let ins_sel = ins_cols
        .iter()
        .map(|c| format!("? AS {c}"))
        .collect::<Vec<_>>()
        .join(", ");
    let insert = |r: &str| -> Arc<str> {
        format!(
            "INSERT INTO \"{t}\" ({}) SELECT * FROM (SELECT {ins_sel}) AS t WHERE ({r}) RETURNING {cols}",
            ins_cols.join(", ")
        )
        .into()
    };

    // Update params: updated, _v, _hlc, (provided flag, value) per single-value field, id, rule binds.
    let mut set = vec![
        "\"updated\" = ?".to_owned(),
        "\"_v\" = ?".to_owned(),
        "\"_hlc\" = ?".to_owned(),
    ];
    set.extend(table_user.iter().map(|c| format!("{c} = iif(?, ?, {c})")));
    let set = set.join(", ");
    let update = |r: &str| -> Arc<str> {
        format!("UPDATE \"{t}\" AS t SET {set} WHERE t.\"id\" = ? AND ({r}) RETURNING {cols}")
            .into()
    };
    let delete = |r: &str| -> Arc<str> {
        format!("DELETE FROM \"{t}\" AS t WHERE t.\"id\" = ? AND ({r})").into()
    };

    Templates {
        view: view(&rule_sql(&rules.view)),
        view_su: view("1"),
        insert: insert(&rule_sql(&rules.create)),
        insert_su: insert("1"),
        update: update(&rule_sql(&rules.update)),
        update_su: update("1"),
        delete: delete(&rule_sql(&rules.delete)),
        delete_su: delete("1"),
        cols,
    }
}
