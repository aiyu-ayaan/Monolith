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
