//! Migration planner (doc 05 §9): diff two collection definitions into DDL.

use crate::error::{CoreError, CoreResult};
use crate::schema::{CollectionDef, FieldDef, IndexDef, OnDelete};

/// SQL string literal, for the few DDL/metadata statements that cannot bind params.
pub fn sql_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn column(f: &FieldDef) -> String {
    let mut c = format!("\"{}\" {}", f.name, f.sql_type());
    if let Some(target) = &f.collection {
        let action = match f.on_delete.unwrap_or_default() {
            OnDelete::Restrict => "RESTRICT",
            OnDelete::Cascade => "CASCADE",
            OnDelete::SetNull => "SET NULL",
        };
        c.push_str(&format!(
            " REFERENCES \"{target}\"(\"id\") ON DELETE {action}"
        ));
    }
    c
}

fn junction_ddl(table: &str, f: &FieldDef) -> [String; 2] {
    let j = f.junction_table(table);
    let target = f.collection.as_deref().unwrap_or("");
    let action = match f.on_delete.unwrap_or_default() {
        OnDelete::Restrict | OnDelete::SetNull => "RESTRICT",
        OnDelete::Cascade => "CASCADE",
    };
    [
        format!(
            "CREATE TABLE \"{j}\" (\
             \"owner\" BLOB NOT NULL REFERENCES \"{table}\"(\"id\") ON DELETE CASCADE, \
             \"target\" BLOB NOT NULL REFERENCES \"{target}\"(\"id\") ON DELETE {action}, \
             \"pos\" INTEGER NOT NULL, \
             PRIMARY KEY (\"owner\", \"target\")\
             ) STRICT, WITHOUT ROWID"
        ),
        format!("CREATE INDEX \"{j}_target\" ON \"{j}\" (\"target\", \"owner\")"),
    ]
}

/// Declared indexes plus one per single-relation (FK lookups, back-relations, cascades).
fn indexes(def: &CollectionDef) -> Vec<IndexDef> {
    let mut v = def.indexes.clone();
    for f in def
        .fields
        .iter()
        .filter(|f| f.collection.is_some() && !f.is_multi_rel())
    {
        let auto = IndexDef {
            fields: vec![f.name.clone()],
            unique: false,
        };
        if !v.contains(&auto) {
            v.push(auto);
        }
    }
    v
}

fn create_index(table: &str, ix: &IndexDef) -> String {
    let cols = ix
        .fields
        .iter()
        .map(|f| format!("\"{f}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "CREATE {}INDEX \"{}\" ON \"{table}\" ({cols})",
        if ix.unique { "UNIQUE " } else { "" },
        ix.sql_name(table)
    )
}

/// DDL to go from `old` to `new` (`old = None` creates the table).
pub fn plan(old: Option<&CollectionDef>, new: &CollectionDef) -> CoreResult<Vec<String>> {
    let t = &new.name;
    let Some(old) = old else {
        let mut cols = vec![
            "\"id\" BLOB PRIMARY KEY".to_owned(),
            "\"created\" INTEGER NOT NULL".to_owned(),
            "\"updated\" INTEGER NOT NULL".to_owned(),
            "\"_v\" INTEGER NOT NULL".to_owned(),
            "\"_hlc\" INTEGER NOT NULL".to_owned(),
        ];
        cols.extend(new.fields.iter().filter(|f| !f.is_multi_rel()).map(column));
        // shortcut: WITHOUT ROWID is the provisional layout; the bake-off decides (doc 05 §2).
        let mut out = vec![format!(
            "CREATE TABLE \"{t}\" ({}) STRICT, WITHOUT ROWID",
            cols.join(", ")
        )];
        out.extend(indexes(new).iter().map(|ix| create_index(t, ix)));
        for f in new.fields.iter().filter(|f| f.is_multi_rel()) {
            out.extend(junction_ddl(t, f));
        }
        return Ok(out);
    };
    if old.name != new.name {
        return Err(CoreError::BadRequest(
            "renaming a collection is not supported yet".into(),
        ));
    }
    let mut out = Vec::new();
    let (old_ix, new_ix) = (indexes(old), indexes(new));
    let dropped: Vec<&FieldDef> = old
        .fields
        .iter()
        .filter(|f| new.field(&f.name).is_none())
        .collect();
    // Indexes go first: SQLite refuses to drop an indexed column.
    for ix in &old_ix {
        if !new_ix.contains(ix) {
            out.push(format!("DROP INDEX \"{}\"", ix.sql_name(t)));
        }
    }
    for f in &dropped {
        if f.is_multi_rel() {
            out.push(format!("DROP TABLE \"{}\"", f.junction_table(t)));
        } else {
            out.push(format!("ALTER TABLE \"{t}\" DROP COLUMN \"{}\"", f.name));
        }
    }
    for f in &new.fields {
        match old.field(&f.name) {
            None if f.is_multi_rel() => out.extend(junction_ddl(t, f)),
            None => out.push(format!("ALTER TABLE \"{t}\" ADD COLUMN {}", column(f))),
            Some(o)
                if o.kind != f.kind
                    || o.only_int != f.only_int
                    || o.collection != f.collection
                    || o.is_multi_rel() != f.is_multi_rel()
                    || o.on_delete.unwrap_or_default() != f.on_delete.unwrap_or_default() =>
            {
                return Err(CoreError::BadRequest(format!(
                    "changing the type of '{}' needs the online table rebuild (not supported yet); add a new field instead",
                    f.name
                )));
            }
            Some(_) => {}
        }
    }
    for ix in &new_ix {
        if !old_ix.contains(ix) {
            out.push(create_index(t, ix));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(json: &str) -> CollectionDef {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn create_and_diff() {
        let v1 = def(
            r#"{"name":"tasks","fields":[{"name":"title","type":"text"},{"name":"owner","type":"relation","collection":"users","onDelete":"cascade"},{"name":"tags","type":"relation","collection":"tags","maxSelect":10}],"indexes":[{"fields":["title"],"unique":true}]}"#,
        );
        let create = plan(None, &v1).unwrap();
        assert!(create[0].starts_with("CREATE TABLE \"tasks\" (\"id\" BLOB PRIMARY KEY"));
        assert!(
            create[0].contains("\"owner\" BLOB REFERENCES \"users\"(\"id\") ON DELETE CASCADE")
        );
        assert!(!create[0].contains("\"tags\""));
        assert!(create.contains(
            &"CREATE UNIQUE INDEX \"idx_tasks_title_u\" ON \"tasks\" (\"title\")".to_owned()
        ));
        assert!(
            create
                .contains(&"CREATE INDEX \"idx_tasks_owner\" ON \"tasks\" (\"owner\")".to_owned())
        );
        assert!(
            create
                .iter()
                .any(|s| s.starts_with("CREATE TABLE \"_rel_tasks_tags\""))
        );
        assert!(create.contains(
            &"CREATE INDEX \"_rel_tasks_tags_target\" ON \"_rel_tasks_tags\" (\"target\", \"owner\")"
                .to_owned()
        ));

        let v2 = def(
            r#"{"name":"tasks","fields":[{"name":"owner","type":"relation","collection":"users","onDelete":"cascade"},{"name":"done","type":"bool"}]}"#,
        );
        assert_eq!(
            plan(Some(&v1), &v2).unwrap(),
            vec![
                "DROP INDEX \"idx_tasks_title_u\"",
                "ALTER TABLE \"tasks\" DROP COLUMN \"title\"",
                "DROP TABLE \"_rel_tasks_tags\"",
                "ALTER TABLE \"tasks\" ADD COLUMN \"done\" INTEGER",
            ]
        );
        let v3 = def(
            r#"{"name":"tasks","fields":[{"name":"owner","type":"relation","collection":"users"},{"name":"done","type":"text"}]}"#,
        );
        assert!(plan(Some(&v2), &v3).is_err());
    }
}
