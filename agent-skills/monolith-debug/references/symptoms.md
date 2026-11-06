# Symptom reference

Each entry: cause, how to confirm, fix. Expand as real incidents occur.

- **401 after ~15 min**: access token expired, refresh not wired. Confirm `exp` claim. Fix single-flight refresh.
- **403 on create**: createRule evaluated against `@request.body`; confirm with `rules:explain` using the same body.
- **Realtime duplicates after reconnect**: client resubscribed without `since` and also refetched. Dedupe by `seq`.
- **Writes slow under load**: `writer_batch_size` histogram near 1 and queue growing means each commit is not batching; check `writer_max_wait_us`, long migrations, or a huge transaction in the batch endpoint.
- **Startup slow**: schema migration `RebuildTable` in progress, see `/system/health` app state.
