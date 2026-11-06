# sdks/

Each SDK: generated protocol layer from `api/` + thin hand-written layer (sessions, realtime, sync).
All must pass `api/conformance/`.

| SDK | Registry | Priority |
|---|---|---|
| typescript | npm `@monolith/client` | P0 |
| kotlin | Maven `dev.monolith:*` (core + android AAR) | P0 |
| dart | pub.dev | P0 |
| python | PyPI | P1 |
| swift | SwiftPM | P1 |
| rust | crates.io | P2 |
