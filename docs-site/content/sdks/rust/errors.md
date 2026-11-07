---
title: Errors
description: Rust: error types and what they carry.
group: Rust
order: 90
---

Error types and what they carry.

`Error` variants: `Api { status, message, problem }`, `Network`, `Serialization`, `Url`, `Config`, `Realtime`. Use `err.status()` and `err.problem()` (with `get_field_error`). See [Errors](/reference/errors).

## Next steps

- [SDKs overview](/sdks/overview)
