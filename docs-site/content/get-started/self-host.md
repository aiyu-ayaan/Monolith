---
title: Self-hosting
description: Build and run the server, create the first superuser, set environment variables and put a reverse proxy in front.
group: Basics
order: 30
---

Monolith is one binary that serves the REST API, the realtime stream and the admin UI, and keeps all state in one data directory. This page shows how to run it, what to configure, and what to know before you expose it to the internet.

## Why one process and one volume

A single process means one thing to start, one log to read and one directory to back up. The admin UI is embedded in the binary and served at `/_/`. There is no separate database server, no queue and no cache to deploy. The tradeoff is that Monolith runs on one node; you scale it by giving the machine more CPU and RAM, not by adding nodes.

:::status partial
A Docker image and `compose.yaml` are designed (see the deployment design doc) but the repository does not contain a Dockerfile yet. Today you build the binary from source. The Docker steps below are the target layout, shown so you can plan, and marked as such.
:::

## Build and run from source

You need the Rust toolchain pinned in `rust-toolchain.toml`, and Node.js with `pnpm` for the admin UI. All project tasks run through root `pnpm` scripts.

```bash
pnpm install
pnpm build:release          # builds the admin UI and the release binary

MONOLITH_DATA=/var/lib/monolith \
MONOLITH_ADDR=0.0.0.0:8090 \
pnpm start                  # builds the UI, then runs the release server
```

For development, `pnpm dev` starts the backend on port 8090 and the admin UI dev server on 5173 together.

## Create the first superuser

While no superuser exists, the server prints a one-time setup link when it starts:

```text
  No superuser yet. Open this one-time link within 1 hour to create one:

  http://127.0.0.1:8090/_/#/install/<64 hex characters>

  (Restart the server for a new link.)
```

Open the link in a browser, choose an email and a password of at least 10 characters, and you are signed in to the admin UI.

How the link works:

- The token lives only in server memory. It is never written to disk or to the logs.
- It expires after 1 hour and is spent on first use.
- Once a superuser exists, any token is rejected. Restart the server to get a new link if you did not use it.
- Credentials never go through environment variables or config files.

### Headless setup

If you cannot open a browser, create a superuser from the command line. It is safe to run while the server is running, and it uses the same `MONOLITH_DATA`.

```bash
monolith superuser create admin@example.com
```

The command prompts for the password without echoing it. If standard input is not a terminal it reads one line from stdin. It never takes the password as an argument.

## Environment variables

These are the variables the server reads today.

| Variable | Default | Meaning |
|---|---|---|
| `MONOLITH_DATA` | `./monolith_data` | Data directory. Holds `system.db`, per-app folders and backups. |
| `MONOLITH_ADDR` | `0.0.0.0:8090` | Listen address (`host:port`). Plain HTTP. |
| `MONOLITH_LOG` | `info` | Log filter in `tracing` syntax, for example `info,monolith_core=debug`. |
| `MONOLITH_PUBLIC_URL` | derived from the request | The public origin (for example `https://api.example.com`). Used to build OAuth callback URLs. Set it when the server runs behind a proxy. |
| `MONOLITH_TRUST_PROXY` | off | Set to `1` or `true` to take the client address from the last `X-Forwarded-For` entry and the scheme from `X-Forwarded-Proto`. |

There is no config file yet. Settings such as the listen address and data directory come from the environment only.

## What lives in the data directory

```text
$MONOLITH_DATA/
├── system.db                  control plane: apps, superusers, app users, sessions, auth config, API keys, webhooks, plugins
├── apps/<app>/
│   ├── data.db (+ -wal, -shm) the app's collections, records and change log
│   └── files/                 blobs and file metadata (when storage is local)
└── backups/<app>/             snapshots taken through the backup API
```

SQLite uses write-ahead logging, so you will see `-wal` and `-shm` files next to each database. Keep the whole directory on a local disk. Do not put it on NFS or SMB: SQLite relies on correct file locking and `fsync`.

## Back up and upgrade

- Take a consistent snapshot of one app with `POST /api/v1/apps/{app}/backups` (superuser). It runs `VACUUM INTO` while the app keeps serving. Restore creates a **new** app; in-place restore is not available. Backups are stored on the same disk, so they do not protect you from losing the disk. Copy them elsewhere.
- Stop the server before copying the whole data directory by hand, or copy a snapshot instead.
- To upgrade, stop the server, replace the binary and start it again.

## Put a reverse proxy in front

The server speaks plain HTTP only. Terminate TLS in a reverse proxy such as Caddy, nginx or Traefik.

Checklist:

1. **Do not buffer responses** on the realtime path (`/api/v1/apps/<app>/realtime`). The server already sends `X-Accel-Buffering: no`, which nginx honors.
2. **Set `MONOLITH_PUBLIC_URL`** to the external HTTPS origin, so OAuth callbacks point at the right host.
3. **Set `MONOLITH_TRUST_PROXY=1`** only if the proxy appends the real client address as the last `X-Forwarded-For` entry and clients cannot reach the server directly. Otherwise a client could forge its address. It affects API-key IP allowlists and request logs.
4. **Allow long-lived responses.** A realtime connection stays open; the server sends a `: hb` comment every 25 seconds, so set the proxy's read timeout above that.
5. **Request size.** The server rejects JSON bodies over 1 MiB with `413`. Plugin uploads are limited to 8 MiB.

Caddy example:

```text title="Caddyfile"
api.example.com {
    reverse_proxy 127.0.0.1:8090 {
        flush_interval -1
    }
}
```

```bash
MONOLITH_PUBLIC_URL=https://api.example.com MONOLITH_TRUST_PROXY=1 monolith serve
```

## Check that it is up

```bash
curl http://127.0.0.1:8090/api/v1/system/health
```

```json
{"status":"ok"}
```

## Docker (target layout)

:::status planned
The following describes the intended image from the deployment design. It is not buildable from this repository yet.
:::

The plan is one image, one `/data` volume, and the same environment variables:

```bash
docker run -d --name monolith \
  -p 8080:8080 \
  -v monolith-data:/data \
  --restart unless-stopped \
  monolith:latest
docker logs monolith        # read the one-time setup link here
```

The headless command would then be `docker exec -it monolith monolith superuser create admin@example.com`.

## Limits and status

- No Dockerfile or published image yet.
- No config file, TLS, built-in rate limit for unauthenticated traffic, or metrics endpoint in Prometheus format.
- Backups are manual, local, and restore into a new app. No schedule, retention or off-site target yet.
- Single node only.

## Next steps

- [Server quickstart](/get-started/quickstart-server): create an app and call the API.
- [Concepts](/get-started/concepts): apps, records and IDs.
- [REST API reference](/reference/rest-api): every endpoint.
