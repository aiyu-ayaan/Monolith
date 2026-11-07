---
title: Rules syntax
description: Operators, variables and macros for rules and filters.
order: 50
---

The same language is used in collection rules and in the `filter` parameter.

## Operators

| Operator | Meaning |
|---|---|
| `=` `!=` `>` `>=` `<` `<=` | Compare. `=` and `!=` are NULL-safe. |
| `~` `!~` | Case-insensitive (ASCII) match. Without `%` it means "contains"; with an unescaped `%` the value is a `LIKE` pattern. |
| `?=` `?!=` `?>` `?>=` `?<` `?<=` `?~` `?!~` | Any-of, for multi-value paths |
| `&&` `\|\|` `( )` | And, or, grouping |
| `in (…)` | Membership |
| `:lower`, `:isset`, `:changed` | Modifiers |

Strings use single or double quotes. Dates compare as RFC 3339 strings; relations and ids compare as base32 id strings. On text, `''` and NULL are the same value, and `<`, `>`, `~` on NULL are false.

## Variables

| Variable | Value |
|---|---|
| `@request.auth.id`, `.collectionName`, `.<claim>` | The caller |
| `@request.body.<field>` | Request body field |
| `@now` | Current time |
| `@yesterday` `@tomorrow` `@todayStart` `@todayEnd` `@monthStart` `@monthEnd` `@yearStart` `@yearEnd` | Date macros (UTC; ends are `23:59:59.999`) |
| `@second` `@minute` `@hour` `@day` `@month` `@weekday` `@year` | Number macros |
| `@collection.<name>(:<alias>).<path>` | Join, up to 4 aliases |

Paths follow relations up to 4 hops (`owner.org.plan = 'pro'`, `tags.name ?= 'rust'`). `<coll>_via_<field>` follows a back-relation. `tags = null` tests for an empty multi-relation.

## Not built yet

`:length`, `:each`, `~` with a field on the right, `@request.query.*`, `@request.key.*`. Types are checked on save: `title = 1` or an unknown field is a `400`.

## Next steps

- [Rules](/build/database/rules)
