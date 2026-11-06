# 10 — Analytics

Firebase-Analytics-like product analytics for every app, self-hosted, privacy-friendly.

## 1. Scope

- Custom events (`task_started`, `screen_view`, `purchase`) with properties.
- Automatic events from SDKs: `session_start`, `app_open`, `screen_view`, `app_update`,
  first open, crash marker (optional).
- Server-side automatic metrics: API requests (reads/writes/4xx/5xx, bytes in/out, latency
  percentiles), auth events, record create/update/delete counts per collection (from the
  change bus), and SQLite writer/reader/queue counters (`GET /api/v1/system/stats` and
  `GET /api/v1/apps/{app}/stats`, kept in memory as 60 × 1-minute and 48 × 1-hour rings
  until the persisted rollup store lands in Phase 4).
- Dashboards: active users (DAU/WAU/MAU), sessions, events over time, top events,
  property breakdowns, funnels, retention cohorts, platform/version split.
- Query API for custom charts and export.

Out of scope v1: ad attribution, A/B testing (feature flags later), user-level
behavioural profiling beyond what the app owner defines.

## 2. Ingestion API

```
POST /api/v1/apps/{app}/analytics/events          (API key with analytics:ingest, or user token)
Content-Type: application/json   (or application/x-ndjson, or MessagePack)
{
  "device": "dev_01j9..", "user": "01j8..", "session": "s_01j9..",
  "ctx": {"platform":"android","app_version":"2.4.1","os":"14","locale":"en-IN"},
  "events": [
    {"name":"task_started","ts":1730883600123,"props":{"task_id":"01j9..","vehicle":"bike"}},
    {"name":"screen_view","ts":1730883601000,"props":{"screen":"map"}}
  ]
}
→ 202 {"accepted":2}
```

- `202` returned after the batch is validated and placed in the ingest buffer —
  the request never waits for disk.
- SDKs batch client-side (e.g. every 30 s or 50 events) and retry with idempotency key.
- Limits per app: events/sec, props per event (default 32), prop value size, event
  name cardinality cap (default 500 distinct names) to prevent explosion.
- IP is used only to derive coarse geo (country) if enabled, then dropped. No raw IP
  stored by default.

## 3. Pipeline

```
HTTP handler ─validate─▶ per-app ring buffer (bounded, lock-free MPSC)
                              │  flush every 1 s or 10k events
                              ▼
                    ingest thread ─▶ DuckDB Appender (columnar, batched)  [analytics-duckdb]
                                  └▶ or SQLite append + rollup upserts     [fallback]
                              │
                     rollup job (every minute) ─▶ pre-aggregated tables (minute/hour/day)
```

Overflow policy: if the buffer is full, return `429` with `Retry-After` (SDK keeps
events). Metric `analytics_dropped_total` stays at 0 in normal operation.

## 4. Storage schema (DuckDB)

```sql
CREATE TABLE events (
  ts          TIMESTAMP_MS NOT NULL,
  name        VARCHAR NOT NULL,        -- dictionary-encoded by DuckDB
  user_id     UUID,
  device_id   UUID,
  session_id  VARCHAR,
  platform    VARCHAR, app_version VARCHAR, country VARCHAR,
  props       JSON
);
-- Rollups
CREATE TABLE agg_events_hourly (hour TIMESTAMP, name VARCHAR, platform VARCHAR, cnt BIGINT, users HUGEINT /* HLL sketch */);
CREATE TABLE agg_active_daily  (day DATE, platform VARCHAR, dau_sketch BLOB);
```

- Unique users per period via HyperLogLog sketches (mergeable: day → week → month).
- Raw events retention per app (default 90 days); rollups kept 2+ years.
- Frequently filtered properties can be "promoted" to real columns (admin UI action),
  which DuckDB scans much faster than JSON.

## 5. Query API

```
POST /api/v1/apps/{app}/analytics/query
{ "metric":"events", "event":"task_started", "from":"2026-10-01", "to":"2026-11-01",
  "interval":"day", "group_by":["platform"], "filter":"props.vehicle = 'bike'" }
```

Preset endpoints: `/analytics/active-users`, `/analytics/retention`, `/analytics/funnel`,
`/analytics/top-events`. Queries hit rollups when the shape allows, raw events otherwise.
Superusers also get a read-only SQL console against the analytics store.

## 6. Engine decision

| | DuckDB (default) | SQLite rollups (fallback) |
|---|---|---|
| Ad-hoc breakdowns over raw events | Fast (columnar, vectorized) | Slow on large ranges; relies on pre-aggregates |
| Ingest | Batched appender, very fast | Batched insert + rollup upsert, fast enough at moderate volume |
| Binary/build cost | Large C++ dependency | None extra |
| Fit | Analytics-heavy apps | Small deployments, minimal binary |

Selected via Cargo feature `analytics-duckdb` (on in Docker image). The analytics
bake-off ([17](17-benchmark-plan.md) A1–A3) validates the choice.

## 7. Privacy

- Per-app switches: collect IP-geo, collect user id (or only device id), retention.
- `DELETE /analytics/users/{id}` purges a user's events (GDPR erasure), implemented as
  a DuckDB delete + rollup rebuild for affected periods.
- Consent flag from SDK: events dropped at ingest if consent is not granted.
