# 06 — Rules Engine

Rules decide who can list, view, create, update and delete records. Filters (from
clients) use the same language. The engine is the hottest code after JSON encoding,
because it runs on every read, every write and every realtime fan-out.

## 1. Language (PocketBase-compatible + extensions)

```
rule      := expr
expr      := term (("&&" | "||") term)*
term      := "(" expr ")" | operand op operand | func_call
op        := "=" | "!=" | ">" | ">=" | "<" | "<=" | "~" | "!~"
           | "?=" | "?!=" | "?>" | "?>=" | "?<" | "?<=" | "?~" | "?!~"     (any-of over multi values)
operand   := literal | identifier | func_call
literal   := 'string' | "string" | number | true | false | null
identifier:= field ( "." field )* modifier?
           | "@request.auth." path | "@request.body." path | "@request.query." name
           | "@request.headers." name | "@request.method" | "@request.context"
           | "@collection." name (":" alias)? "." path
           | "@now" | "@yesterday" | "@tomorrow" | "@todayStart" | "@todayEnd"
           | "@monthStart" | "@monthEnd" | "@yearStart" | "@yearEnd"      (dates, UTC)
           | "@second" | "@minute" | "@hour" | "@day" | "@month" | "@weekday" | "@year"  (numbers)
modifier  := ":isset" | ":changed" | ":length" | ":each" | ":lower"
func_call := "geoDistance(" lonA "," latA "," lonB "," latB ")" | "strftime(" fmt "," v ")"
```

Text semantics follow PocketBase: `''` and NULL are one value for `=`/`!=`, and `~` wraps the
value in `%…%` (with `_` literal) unless it contains an unescaped `%`, in which case it is the
LIKE pattern as written. Vectors: `api/conformance/vectors/rules.json`.

Rule values: `null` = superusers only, `""` = public, expression otherwise.

**Extensions (Monolith):**

| Extension | Example | Purpose |
|-----------|---------|---------|
| API key scopes | `@request.key.scopes ?= "tasks:write"` | Rules for machine clients ([08](08-authentication.md)) |
| Roles helper | `@request.auth.role = "editor"` | Plain field access, documented pattern |
| `@request.device` | `@request.device.id = device` | Sync rules per device |
| `in` list | `status in ("open","assigned")` | Sugar for `||` chains; compiles to `IN (...)` |
| Named rule macros | `$isOwner` defined once per app | Reuse without copy-paste |

## 2. Pipeline

```
rule text ──parse──▶ AST ──resolve + type-check against Schema vN──▶ Typed IR
                                                                      │
              ┌───────────────────────────────────────────────────────┼─────────────────────────┐
              ▼                                                       ▼                         ▼
     SQL compiler (dialect)                               Predicate compiler              Static analysis
  WHERE fragment + param slots                 closure tree over (row, auth, request)   (classification §4)
```

- Rules compile **once per (schema version, collection, operation)** at schema load,
  not per request. Client filters compile once per distinct filter string per schema
  version (LRU cache keyed by `(schema_v, collection, filter_text)`, default 4096
  entries, bounded memory).
- Type-check errors surface at schema-save time in the admin UI (PocketBase surfaces
  many of them only at request time).
- Parameters (`@request.auth.id`, literals) become bind slots, so the SQL text is
  stable and `prepare_cached` hits every time (fixes B12).

## 3. SQL compilation rules

| Construct | SQLite output |
|-----------|---------------|
| `field = 'x'` | `t."field" = ?1` |
| `rel.name = 'x'` (single relation) | `EXISTS (SELECT 1 FROM "users" r WHERE r.id = t."rel" AND r."name" = ?1)`, or `LEFT JOIN` when the planner benefits (sort by related field) |
| `tags.name ?= 'x'` (multi relation, any) | `EXISTS (SELECT 1 FROM "_rel_posts_tags" j JOIN "tags" r ON r.id = j.target WHERE j.owner = t.id AND r."name" = ?1)` |
| `tags.name = 'x'` (multi relation, all) | `NOT EXISTS (SELECT 1 FROM "_rel_posts_tags" j JOIN "tags" r ON r.id=j.target WHERE j.owner = t.id AND NOT (r."name" = ?1))` and non-empty |
| `comments_via_post.author = @request.auth.id` | `EXISTS (SELECT 1 FROM "comments" c WHERE c."post" = t.id AND c."author" = ?1)` (uses FK index) |
| `@collection.members.user = @request.auth.id && @collection.members.team = team` | `EXISTS (SELECT 1 FROM "members" m WHERE m."user" = ?1 AND m."team" = t."team")` |
| `@request.auth.id != ""` | constant-folded per request (no SQL at all) |
| `select_multi ?= 'a'` (bitmask) | `(t."flags" & 4) != 0` |
| `title ~ 'abc'` | `t."title" LIKE ?1 ESCAPE '\'` with `%abc%`; uses FTS5 if the field has a search index |

Semi-joins (`EXISTS`) never multiply rows, so list queries need **no `DISTINCT`**
(fixes B11). The compiler emits joins only when a sort key needs a related column.

## 4. Static classification (drives realtime and caching)

Each compiled rule gets a class:

| Class | Depends on | Evaluation in realtime / post-write | Example |
|-------|-----------|-------------------------------------|---------|
| `Const` | nothing | constant true/false | `""`, `null` |
| `AuthOnly` | only `@request.*` | once per subscriber, cached on the connection | `@request.auth.role = "admin"` |
| `RowLocal` | row columns + `@request.*` | **in memory** on the changed row, no SQL | `owner = @request.auth.id && status != "draft"` |
| `Relational` | other tables | SQL, **one batched query per change** for all subscribers sharing the rule (§5) | `team.members ?= @request.auth.id` |

Most real-world rules are `RowLocal` (ownership, status, visibility flags). The
admin UI shows each rule's class and warns when a rule is `Relational` on a
high-traffic collection.

## 5. Batched relational checks (realtime)

For a `Relational` rule and K subscribers with distinct auth ids, instead of K queries:

```sql
SELECT a.value
FROM json_each(?1) a                       -- K auth ids, bound once
WHERE EXISTS (SELECT 1 FROM "_rel_teams_members" j
              WHERE j.owner = ?2 /* row.team */ AND j.target = a.value)
```

One query per change per distinct rule, returning the set of allowed subscribers.
A short-lived (per change) memo avoids recomputing for identical auth ids.

## 6. Predicate compiler (in-memory)

- Produces a tree of small closures (`Box<dyn Fn(&RowView, &AuthCtx) -> Tri>`) or, in a
  later iteration, a flat bytecode interpreted by a tight loop (profile first).
- `RowView` reads typed column values straight from the change event (no JSON
  re-parse).
- Semantics match SQLite exactly for `=`, comparisons, `LIKE` (case-insensitive ASCII),
  `NULL` handling, and number/text coercion. A **differential test suite** runs every
  rule against both compilers on random rows and must agree 100%.

## 7. Write-time rules

- `createRule` / `updateRule` evaluate on the **proposed** row plus `@request.body`
  fields. `RowLocal` rules run in memory inside the writer before the INSERT/UPDATE.
- `Relational` write rules run as a SQL check in the same transaction (savepoint),
  so they see a consistent snapshot.
- `:changed` / `:isset` compare against the old row that the writer already loaded.
- `updateRule` and `deleteRule` include the old row's visibility (PocketBase semantics:
  you can only update what you can view, unless the rule says otherwise).

## 8. Limits and safety

| Limit | Default |
|-------|---------|
| Filter text length | 4096 chars |
| AST nodes | 200 |
| Relation hops in one path | 4 |
| `@collection` references per rule | 4 |
| Compiled SQL query timeout | `read_timeout` (2 s); SQLite progress handler interrupts long queries |

Superuser-only fields (password hashes, token secrets, hidden fields) cannot be
referenced from client filters; the type checker rejects them.
