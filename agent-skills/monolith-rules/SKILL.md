---
name: monolith-rules
description: Write and review Monolith access rules and filters (PocketBase-compatible syntax with extensions). Use when creating collections, setting list/view/create/update/delete rules, or diagnosing 403/404 and empty lists.
---

# Rules

- `null` rule = superuser only, `""` = public, otherwise an expression.
- Operators `= != > >= < <= ~ !~`, any-of `?=`..., `&&`, `||`, parentheses, `in (...)`.
- Variables: `@request.auth.*`, `@request.body.*`, `@request.query.*`, `@request.key.scopes`, `@collection.<name>.*`, `@now`, `@today`.
- Classes: RowLocal (fast, in memory) is preferred. Relational rules cost a SQL check per change; avoid on hot collections.
- Typical ownership: `owner = @request.auth.id`. Public read, owner write: listRule `""`, updateRule `owner = @request.auth.id`.
- Empty list but no error usually means the listRule filtered everything. A 404 on view means viewRule denied (existence is hidden).
- Always verify with the explain tool (see `monolith-debug`): `POST /rules:explain`.

## Implemented today (Phase 1 rules engine)

- Works: `= != > >= < <= ~ !~`, `&&`, `||`, parentheses, `in (...)`, `:lower`,
  `@request.auth.id`, `@request.auth.collectionName`, `@request.auth.<claim>`, `@request.body.<field>`,
  `@now`, single-relation paths up to 4 hops (`owner.org.plan = 'pro'`).
- Not yet: `?=` any-of operators and multi-relations, `@collection.*`, back-relations (`_via_`),
  `:isset`/`:changed`/`:length`/`:each`, `@today` and friends, `@request.query.*`, `@request.key.*`,
  `rules:explain`.
- Types are checked when the collection is saved: `title = 1` or an unknown field is a 400.
  Dates compare with RFC 3339 strings; relations and ids compare with base32 id strings.
  `@request.auth.id != ""` means "signed in".
- `=`/`!=` are NULL-safe (`IS`/`IS NOT`). `<`, `>`, `~` on NULL are false.
- `~` is a case-insensitive (ASCII) substring match; `%` and `_` in the value are literal.
- Client `filter` cannot reference `hidden` fields; rules can.
