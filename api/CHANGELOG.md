# API changelog
## v1 (draft)
- Initial contract, see development/docs/11-api-and-openapi.md and 07-realtime.md.
- Phase 1: OpenAPI for health, superuser login, superusers, apps, collections,
  records (list/create/view/update/delete, keyset cursor) and transactional batch.
- RFC 9457 problem schema with stable `type` codes and per-field `errors[]`.
- Conformance vectors: `ids.json` (wire id encoding), `filters.json` (filter validity).
- Phase 1 follow-ups: `expand`, `total=none|exact|estimate`, `records:count`, `PUT` upsert,
  `If-Match`/`ETag`/`If-None-Match` (`412`/`304`), `dry_run`, and debug endpoints
  (`collections:diff`, `rules:explain`, `query:explain`, `logs`, `logs/slow`).
