# 18 — Deployment (One Container)

## 1. Image

Everything — server, admin UI, SQLite, DuckDB analytics, WASM runtime — is one
statically linked binary in one image. If the bake-off selects the Postgres fallback
for any app, Postgres runs **inside the same container** as a supervised child process
(§5), keeping the one-container rule.

```dockerfile
# syntax=docker/dockerfile:1.7
FROM node:24-alpine AS ui
ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME:$PATH"
RUN corepack enable
WORKDIR /app
COPY package.json pnpm-workspace.yaml pnpm-lock.yaml ./
COPY ui/package.json ./ui/
COPY sdks/typescript/package.json ./sdks/typescript/
RUN pnpm install --frozen-lockfile
COPY ui/ ./ui/
COPY sdks/typescript/ ./sdks/typescript/
RUN pnpm --filter @monolith/ui build    # emits dist/ with .br and .gz siblings

FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
COPY --from=ui /app/ui/dist ui/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p monolith-server --target x86_64-unknown-linux-musl \
 && cp target/x86_64-unknown-linux-musl/release/monolith /monolith

FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=build /monolith /monolith
VOLUME ["/data"]
EXPOSE 8080
USER nonroot:nonroot
ENTRYPOINT ["/monolith", "serve", "--data", "/data", "--http", "0.0.0.0:8080"]
HEALTHCHECK NONE   # orchestrator probes /api/v1/system/health (no shell in distroless)
```

Notes:
- musl static build needs `mimalloc` (musl's malloc is slow under contention) — the
  allocator choice already covers this. Bench glibc (`distroless/cc`) vs musl images;
  pick the faster.
- DuckDB's C++ build makes musl static linking harder; if it fails, use the
  glibc `distroless/cc-debian12` base. Decided in Phase 0.
- Image variants: `latest` (x86-64 baseline), `latest-v3` (x86-64-v3), `latest-arm64`.

## 2. Run

```bash
docker run -d --name monolith \
  -p 8080:8080 \
  -v monolith-data:/data \
  -e MONOLITH_MASTER_KEY_FILE=/run/secrets/master_key \
  --ulimit nofile=1048576:1048576 \
  --restart unless-stopped \
  monolith:latest
```

First start prints a one-time setup URL for creating the first superuser
(or `monolith superuser create` via `docker exec`).

## 3. Config (`/data/monolith.toml`)

```toml
[server]
http = "0.0.0.0:8080"
public_url = "https://api.example.com"
trusted_proxies = ["10.0.0.0/8"]
max_body = "1MiB"

[storage]
engine = "sqlite"              # per-app override possible: "postgres" (fallback engine)
reader_threads = 0             # 0 = auto (min(cores*2, 16))
writer_max_batch = 256
writer_max_wait_us = 500
durability = "normal"          # normal | full

[realtime]
max_connections = 100000
queue_frames = 1024
heartbeat_s = 25

[analytics]
engine = "duckdb"
raw_retention_days = 90

[backup]
default_target = "local"       # or s3://...
```

Secrets via env or files: `MONOLITH_MASTER_KEY(_FILE)`, `MONOLITH_S3_*`,
`MONOLITH_SMTP_*`.

## 4. Host tuning (documented, applied by operator)

- `nofile` ulimit ≥ 1M for many realtime connections.
- `net.core.somaxconn=65535`, `net.ipv4.tcp_max_syn_backlog=65535`,
  `net.ipv4.ip_local_port_range` widened (if behind a proxy on the same host).
- Data volume on local NVMe (not network storage) — SQLite WAL relies on correct
  `fsync` and shared-memory locking; **never put `/data` on NFS/SMB**.
- Reverse proxy (Caddy/Traefik/nginx) for TLS: disable response buffering for
  `/realtime` (`X-Accel-Buffering: no` is sent), enable HTTP/2.

## 5. Optional Postgres inside the container (fallback engine only)

- Separate image tag `monolith:pg` includes Postgres binaries.
- The `monolith` binary spawns and supervises `postgres` (data at `/data/pg`), connects
  over a Unix socket, forwards signals, and stops it on shutdown. No init system needed.
- Health of the child process is part of `/health`.

## 6. Operations

| Task | How |
|------|-----|
| Upgrade | Pull new image, restart; migrations for system and app DBs run at startup with automatic pre-upgrade snapshot |
| Backup | Built-in per-app policies ([09](09-sync-and-backup.md)); `monolith backup create --app X` |
| Restore | Admin UI or `monolith backup restore --app X --to X-restored` |
| Logs | stdout JSON (container logs) + per-app request logs in `logs.db` |
| Metrics | `/api/v1/system/metrics` (Prometheus), optionally on a separate port |
| Graceful shutdown | SIGTERM → stop accepting, drain requests (30 s), flush writer queues, checkpoint WAL, close |
