"""Realtime over SSE: live events, filtered subscriptions and Firestore-style snapshots.

Plain subscriptions on a client share one connection (ref-counted, reopened when the
collection set changes). A `filter` or `ids` narrows a subscription and opens a dedicated
connection. All streams reconnect with exponential backoff + jitter; plain streams resume
with `since=<last seq>`, snapshot streams re-snapshot instead. Callbacks run on the stream's
background thread; an exception in a callback is swallowed so it cannot break the stream.
"""

from __future__ import annotations

import functools
import json
import random
import threading
import urllib.parse
from dataclasses import dataclass, field
from typing import Any, Callable

import httpx

from monolith.http import HttpClient

Record = dict[str, Any]
Unsubscribe = Callable[[], None]


@dataclass
class RealtimeEvent:
    """`action` is create|update|delete|leave. `leave`: record stopped matching, not deleted."""

    action: str
    record: Record
    collection: str
    seq: int | None = None


@dataclass
class DocumentChange:
    type: str  # "added" | "modified" | "removed"
    doc: Record


@dataclass
class QuerySnapshot:
    docs: list[Record] = field(default_factory=list)
    changes: list[DocumentChange] = field(default_factory=list)


def _safe(fn: Callable[[], None]) -> None:
    try:
        fn()
    except Exception:  # a listener must not break the stream
        pass


def _comparator(sort: str | None) -> Callable[[Record, Record], int] | None:
    """Client-side comparator for `-created,title` style sort strings (nulls first when ascending)."""
    if not sort:
        return None
    keys: list[tuple[str, int]] = []
    for part in sort.split(","):
        part = part.strip()
        if not part:
            continue
        if part.startswith("-"):
            keys.append((part[1:], -1))
        else:
            keys.append((part.lstrip("+"), 1))

    def cmp(a: Record, b: Record) -> int:
        for f, d in keys:
            x, y = a.get(f), b.get(f)
            if x == y:
                continue
            if x is None:
                return -d
            if y is None:
                return d
            try:
                lt = x < y
            except TypeError:  # mixed types: fall back to string order
                lt = str(x) < str(y)
            return (-1 if lt else 1) * d
        return 0

    return cmp


class _Stream:
    """One SSE connection on a background thread with reconnect, backoff and resume."""

    def __init__(
        self,
        service: RealtimeService,
        params: Callable[[], dict[str, str]],
        dispatch: Callable[[str, dict[str, Any]], None],
        snapshot: bool,
    ) -> None:
        self._service = service
        self._params = params
        self._dispatch = dispatch
        self._snapshot = snapshot  # re-snapshot on reconnect instead of `since` replay
        self._lock = threading.Lock()
        self._wake = threading.Event()
        self._resp: httpx.Response | None = None
        self._stopped = True
        self._restart_pending = False
        self._thread: threading.Thread | None = None
        self.last_seq = 0

    def start(self) -> None:
        self._stopped = False
        self._thread = threading.Thread(
            target=self._run, name="monolith-realtime", daemon=True
        )
        self._thread.start()

    def restart(self) -> None:
        """Reopen now (collection list changed). Keeps `last_seq` so nothing is missed."""
        with self._lock:
            self._restart_pending = True
            resp = self._resp
        self._wake.set()
        if resp is not None:
            _close_quietly(resp)

    def stop(self) -> None:
        with self._lock:
            self._stopped = True
            resp = self._resp
        self._wake.set()
        if resp is not None:
            _close_quietly(resp)

    def _run(self) -> None:
        attempts = 0
        while not self._stopped:
            with self._lock:
                self._restart_pending = False
            self._wake.clear()
            ok = self._connect()
            attempts = 0 if ok else attempts + 1
            if self._stopped:
                return
            with self._lock:
                immediate = self._restart_pending
            if immediate:
                continue
            delay = min(
                self._service.backoff_base * 2 ** max(attempts - 1, 0)
                + random.uniform(0, self._service.backoff_base * 0.4),
                self._service.backoff_max,
            )
            self._wake.wait(delay)

    def _connect(self) -> bool:
        """Runs one connection to its end. Returns True when the server accepted it."""
        http = self._service.http
        query: dict[str, str] = self._params()
        if not self._snapshot and self.last_seq > 0:
            query["since"] = str(self.last_seq)
        url = http.build_url(
            f"/api/v1/apps/{urllib.parse.quote(http.app, safe='')}/realtime", query
        )
        headers = dict(http.default_headers)
        headers["Accept"] = "text/event-stream"
        headers["Cache-Control"] = "no-cache"
        token = http.get_token() or http.get_api_key()
        if token and not any(k.lower() == "authorization" for k in headers):
            headers["Authorization"] = f"Bearer {token}"

        client = http._client
        resp: httpx.Response | None = None
        try:
            req = client.build_request(
                "GET", url, headers=headers, timeout=httpx.Timeout(10.0, read=None)
            )
            resp = client.send(req, stream=True)
            with self._lock:
                self._resp = resp
                if self._stopped or self._restart_pending:
                    return False
            if resp.status_code != 200:
                return False
            event, data = "message", []
            for line in resp.iter_lines():
                if self._stopped:
                    break
                if line == "":
                    if data:
                        self._handle(event, "\n".join(data))
                    event, data = "message", []
                elif line.startswith(":"):
                    continue
                elif line.startswith("event:"):
                    event = line[6:].strip()
                elif line.startswith("data:"):
                    data.append(line[5:].strip())
            return True
        except Exception:  # stream ended, network error or closed by stop()/restart()
            return resp is not None and resp.status_code == 200
        finally:
            with self._lock:
                self._resp = None
            if resp is not None:
                _close_quietly(resp)

    def _handle(self, event: str, data: str) -> None:
        try:
            payload = json.loads(data)
        except ValueError:
            return
        if not isinstance(payload, dict):
            return
        seq = payload.get("seq")
        if isinstance(seq, int) and seq > self.last_seq:
            self.last_seq = seq
        if event == "resync":
            # Missed more than the server keeps: reconnect from scratch.
            self.last_seq = 0
            self._dispatch(event, payload)
            self.restart()
            return
        self._dispatch(event, payload)


def _close_quietly(resp: httpx.Response) -> None:
    try:
        resp.close()
    except Exception:
        pass


def _to_event(p: dict[str, Any]) -> RealtimeEvent:
    return RealtimeEvent(
        action=p.get("op", ""),
        record=p.get("record") or {},
        collection=p.get("collection", ""),
        seq=p.get("seq"),
    )


class RealtimeService:
    def __init__(self, http: HttpClient) -> None:
        self.http = http
        self.backoff_base = 0.5  # seconds; doubles per failed attempt
        self.backoff_max = 15.0
        self._lock = threading.RLock()
        self._shared: _Stream | None = None
        self._listeners: dict[str, list[Callable[[RealtimeEvent], None]]] = {}
        self._streams: set[_Stream] = set()

    def subscribe(
        self,
        collection: str,
        listener: Callable[[RealtimeEvent], None],
        *,
        filter: str | None = None,
        ids: list[str] | None = None,
    ) -> Unsubscribe:
        """Live events for a collection. Returns the unsubscribe function.

        Without `filter`/`ids` all subscriptions share one connection; narrowing opens a
        dedicated one. After a reconnect the stream resumes from the last seen change.
        """
        if filter or ids:

            def params() -> dict[str, str]:
                q = {"collections": collection}
                if filter:
                    q[f"filter.{collection}"] = filter
                if ids:
                    q[f"ids.{collection}"] = ",".join(ids)
                return q

            def dispatch(ev: str, p: dict[str, Any]) -> None:
                if ev == "record":
                    _safe(lambda: listener(_to_event(p)))

            return self._open(params, dispatch, snapshot=False)

        with self._lock:
            listeners = self._listeners.get(collection)
            fresh = listeners is None
            if listeners is None:
                listeners = self._listeners[collection] = []
            listeners.append(listener)
            if self._shared is None:

                def shared_dispatch(ev: str, p: dict[str, Any]) -> None:
                    if ev != "record":
                        return
                    with self._lock:
                        current = list(self._listeners.get(p.get("collection", ""), ()))
                    event = _to_event(p)
                    for fn in current:
                        _safe(lambda fn=fn: fn(event))

                self._shared = _Stream(
                    self,
                    self._shared_params,
                    shared_dispatch,
                    False,
                )
                self._shared.start()
            elif fresh:
                self._shared.restart()

        def unsubscribe() -> None:
            with self._lock:
                cur = self._listeners.get(collection)
                if cur is None or listener not in cur:
                    return
                cur.remove(listener)
                emptied = not cur
                if emptied:
                    del self._listeners[collection]
                if not self._listeners:
                    if self._shared is not None:
                        self._shared.stop()
                    self._shared = None
                elif emptied and self._shared is not None:
                    self._shared.restart()

        return unsubscribe

    def _shared_params(self) -> dict[str, str]:
        with self._lock:
            return {"collections": ",".join(self._listeners.keys())}

    def on_snapshot(
        self,
        collection: str,
        callback: Callable[[QuerySnapshot], None],
        *,
        filter: str | None = None,
        sort: str | None = None,
        limit: int | None = None,
        on_error: Callable[[Exception], None] | None = None,
    ) -> Unsubscribe:
        """Firestore-style query listener.

        Calls back with the current result set, then again after every change that affects
        it. Reconnects re-snapshot, so the view always converges. Runs on a background thread.
        """
        docs: dict[str, Record] = {}
        cmp = _comparator(sort)

        def ordered() -> list[Record]:
            items = list(docs.values())
            return sorted(items, key=functools.cmp_to_key(cmp)) if cmp else items

        def params() -> dict[str, str]:
            q = {
                "collections": collection,
                "snapshot": str(limit if limit is not None else 100),
            }
            if filter:
                q[f"filter.{collection}"] = filter
            if sort:
                q[f"sort.{collection}"] = sort
            return q

        def dispatch(ev: str, p: dict[str, Any]) -> None:
            changes: list[DocumentChange] = []
            if ev == "snapshot":
                nxt = {d["id"]: d for d in p.get("items") or []}
                for i, d in nxt.items():
                    changes.append(
                        DocumentChange("modified" if i in docs else "added", d)
                    )
                for i, d in docs.items():
                    if i not in nxt:
                        changes.append(DocumentChange("removed", d))
                docs.clear()
                docs.update(nxt)
            elif ev == "record":
                rec = p.get("record") or {}
                rid = rec.get("id")
                if rid is None:
                    return
                if p.get("op") in ("delete", "leave"):
                    old = docs.pop(rid, None)
                    if old is None:
                        return
                    changes.append(DocumentChange("removed", old))
                else:
                    changes.append(
                        DocumentChange("modified" if rid in docs else "added", rec)
                    )
                    docs[rid] = rec
            elif ev == "error":
                if on_error:
                    _safe(
                        lambda: on_error(
                            Exception(p.get("message") or "realtime error")
                        )
                    )
                return
            else:
                return
            snap = QuerySnapshot(docs=ordered(), changes=changes)
            _safe(lambda: callback(snap))

        return self._open(params, dispatch, snapshot=True)

    def on_doc_snapshot(
        self,
        collection: str,
        id: str,
        callback: Callable[[Record | None], None],
        *,
        on_error: Callable[[Exception], None] | None = None,
    ) -> Unsubscribe:
        """Listens to one record: calls back with it, or None once deleted or no longer visible."""
        return self.on_snapshot(
            collection,
            lambda snap: callback(snap.docs[0] if snap.docs else None),
            filter=f"id = '{id.replace(chr(39), '')}'",
            limit=1,
            on_error=on_error,
        )

    def close(self) -> None:
        """Stops every stream this service opened."""
        with self._lock:
            streams = list(self._streams)
            self._streams.clear()
            shared, self._shared = self._shared, None
            self._listeners.clear()
        for s in streams:
            s.stop()
        if shared is not None:
            shared.stop()

    def _open(
        self,
        params: Callable[[], dict[str, str]],
        dispatch: Callable[[str, dict[str, Any]], None],
        snapshot: bool,
    ) -> Unsubscribe:
        stream = _Stream(self, params, dispatch, snapshot)
        with self._lock:
            self._streams.add(stream)
        stream.start()

        def stop() -> None:
            with self._lock:
                self._streams.discard(stream)
            stream.stop()

        return stop
