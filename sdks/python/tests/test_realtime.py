from __future__ import annotations

import json
import threading
import time
import unittest
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any, Callable

from monolith import MonolithClient, create_client
from monolith.realtime import _comparator


class FakeSse:
    """Local SSE server. Each connection serves the next body, then holds open (or ends)."""

    def __init__(self, bodies: list[str], end: bool = False) -> None:
        self.bodies = list(bodies)
        self.end = end
        self.requests: list[tuple[str, dict[str, str]]] = []
        outer = self

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:  # noqa: N802
                parsed = urllib.parse.urlparse(self.path)
                query = {
                    k: v[0] for k, v in urllib.parse.parse_qs(parsed.query).items()
                }
                outer.requests.append((parsed.path, query))
                outer.auth = self.headers.get("Authorization")
                body = outer.bodies.pop(0) if outer.bodies else ""
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.end_headers()
                if body:
                    self.wfile.write(body.encode())
                    self.wfile.flush()
                if outer.end and body:
                    return
                try:  # hold the connection until the client goes away
                    while self.wfile.write(b": hb\n\n") or True:
                        self.wfile.flush()
                        time.sleep(0.02)
                except OSError:
                    pass

            def log_message(self, *a: Any) -> None:
                pass

        self.auth: str | None = None
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"

    def close(self) -> None:
        self.server.shutdown()
        self.server.server_close()


def rec(seq: int, op: str, record: dict[str, Any]) -> str:
    data = json.dumps({"op": op, "collection": "tasks", "seq": seq, "record": record})
    return f"id: {seq}\nevent: record\ndata: {data}\n\n"


def wait_for(cond: Callable[[], bool], timeout: float = 3.0) -> bool:
    end = time.time() + timeout
    while time.time() < end:
        if cond():
            return True
        time.sleep(0.01)
    return cond()


class TestRealtime(unittest.TestCase):
    def setUp(self) -> None:
        self.servers: list[FakeSse] = []
        self.clients: list[MonolithClient] = []

    def tearDown(self) -> None:
        for c in self.clients:
            c.close()
        for s in self.servers:
            s.close()

    def make(
        self, bodies: list[str], end: bool = False
    ) -> tuple[FakeSse, MonolithClient]:
        srv = FakeSse(bodies, end)
        self.servers.append(srv)
        client = create_client(url=srv.url, app="app", token="tok")
        client.realtime.backoff_base = 0.01
        self.clients.append(client)
        return srv, client

    def test_filtered_subscription_maps_events_and_uses_dedicated_topic(self) -> None:
        srv, client = self.make([rec(5, "leave", {"id": "a"})])
        got: list[Any] = []
        off = client.collection("tasks").subscribe(
            got.append, filter="done = false", ids=["a", "b"]
        )
        self.assertTrue(wait_for(lambda: len(got) == 1))
        off()
        self.assertEqual(
            (got[0].action, got[0].record, got[0].collection, got[0].seq),
            ("leave", {"id": "a"}, "tasks", 5),
        )
        path, q = srv.requests[0]
        self.assertEqual(path, "/api/v1/apps/app/realtime")
        self.assertEqual(q["collections"], "tasks")
        self.assertEqual(q["filter.tasks"], "done = false")
        self.assertEqual(q["ids.tasks"], "a,b")
        self.assertEqual(srv.auth, "Bearer tok")

    def test_plain_subscriptions_share_one_connection(self) -> None:
        srv, client = self.make([""])
        got: list[Any] = []
        off1 = client.collection("tasks").subscribe(got.append)
        self.assertTrue(wait_for(lambda: len(srv.requests) == 1))
        off2 = client.collection("tasks").subscribe(got.append)
        time.sleep(0.1)
        self.assertEqual(len(srv.requests), 1)  # same collection: no reopen
        off1()
        off2()

    def test_new_collection_reopens_shared_connection(self) -> None:
        srv, client = self.make(["", ""])
        off1 = client.collection("tasks").subscribe(lambda e: None)
        self.assertTrue(wait_for(lambda: len(srv.requests) == 1))
        off2 = client.collection("notes").subscribe(lambda e: None)
        self.assertTrue(wait_for(lambda: len(srv.requests) == 2))
        self.assertEqual(
            sorted(srv.requests[1][1]["collections"].split(",")), ["notes", "tasks"]
        )
        off1()
        off2()

    def test_resumes_plain_subscription_with_since(self) -> None:
        srv, client = self.make([rec(9, "create", {"id": "z"}), ""], end=True)
        got: list[Any] = []
        off = client.collection("tasks").subscribe(got.append)
        self.assertTrue(wait_for(lambda: len(srv.requests) >= 2))
        off()
        self.assertEqual(len(got), 1)
        self.assertNotIn("since", srv.requests[0][1])
        self.assertEqual(srv.requests[1][1]["since"], "9")

    def test_resync_reconnects_without_since(self) -> None:
        resync = 'event: resync\ndata: {"reason":"since"}\n\n'
        srv, client = self.make([rec(9, "create", {"id": "z"}) + resync, ""])
        off = client.collection("tasks").subscribe(lambda e: None)
        self.assertTrue(wait_for(lambda: len(srv.requests) >= 2))
        off()
        self.assertNotIn("since", srv.requests[1][1])

    def test_on_snapshot_merges_snapshot_updates_leave_and_delete(self) -> None:
        snap = (
            "event: snapshot\ndata: "
            + json.dumps(
                {
                    "collection": "tasks",
                    "seq": 3,
                    "items": [{"id": "a", "n": 2}, {"id": "b", "n": 1}],
                    "next": None,
                }
            )
            + "\n\n"
        )
        body = (
            snap
            + rec(4, "create", {"id": "c", "n": 0})
            + rec(5, "leave", {"id": "a"})
            + rec(6, "delete", {"id": "b"})
        )
        srv, client = self.make([body])
        snaps: list[Any] = []
        off = client.collection("tasks").on_snapshot(
            snaps.append, filter="n >= 0", sort="n", limit=5
        )
        self.assertTrue(wait_for(lambda: len(snaps) == 4))
        off()
        self.assertEqual(
            [[d["id"] for d in s.docs] for s in snaps],
            [["b", "a"], ["c", "b", "a"], ["c", "b"], ["c"]],
        )
        self.assertEqual(
            [(c.type, c.doc["id"]) for c in snaps[1].changes], [("added", "c")]
        )
        self.assertEqual(snaps[2].changes[0].type, "removed")
        q = srv.requests[0][1]
        self.assertEqual(q["snapshot"], "5")
        self.assertEqual(q["sort.tasks"], "n")
        self.assertEqual(q["filter.tasks"], "n >= 0")

    def test_on_snapshot_resnapshots_on_reconnect(self) -> None:
        def snap(items: list[dict[str, Any]]) -> str:
            return (
                "event: snapshot\ndata: "
                + json.dumps({"collection": "tasks", "seq": 1, "items": items})
                + "\n\n"
            )

        srv, client = self.make(
            [snap([{"id": "a"}, {"id": "b"}]), snap([{"id": "b"}, {"id": "c"}])],
            end=True,
        )
        snaps: list[Any] = []
        off = client.collection("tasks").on_snapshot(snaps.append)
        self.assertTrue(wait_for(lambda: len(snaps) >= 2))
        off()
        self.assertNotIn("since", srv.requests[1][1])
        changes = sorted((c.type, c.doc["id"]) for c in snaps[1].changes)
        self.assertEqual(changes, [("added", "c"), ("modified", "b"), ("removed", "a")])

    def test_on_doc_snapshot_filters_by_id_and_nulls_after_delete(self) -> None:
        snap = (
            "event: snapshot\ndata: "
            + json.dumps(
                {"collection": "tasks", "seq": 1, "items": [{"id": "x", "t": 1}]}
            )
            + "\n\n"
        )
        srv, client = self.make([snap + rec(2, "delete", {"id": "x"})])
        docs: list[Any] = []
        off = client.collection("tasks").on_doc_snapshot("x'y", docs.append)
        self.assertTrue(wait_for(lambda: len(docs) == 2))
        off()
        self.assertEqual(docs, [{"id": "x", "t": 1}, None])
        q = srv.requests[0][1]
        self.assertEqual(q["filter.tasks"], "id = 'xy'")
        self.assertEqual(q["snapshot"], "1")

    def test_on_snapshot_reports_server_error(self) -> None:
        srv, client = self.make(['event: error\ndata: {"message":"boom"}\n\n'])
        errs: list[Exception] = []
        off = client.collection("tasks").on_snapshot(
            lambda s: None, on_error=errs.append
        )
        self.assertTrue(wait_for(lambda: len(errs) == 1))
        off()
        self.assertEqual(str(errs[0]), "boom")

    def test_unsubscribe_stops_reconnecting(self) -> None:
        srv, client = self.make(["", "", ""], end=False)
        off = client.collection("tasks").subscribe(lambda e: None, filter="x = 1")
        self.assertTrue(wait_for(lambda: len(srv.requests) == 1))
        off()
        time.sleep(0.2)
        self.assertEqual(len(srv.requests), 1)

    def test_comparator_multi_key_and_nulls(self) -> None:
        cmp = _comparator("-created,title")
        assert cmp is not None
        rows = [
            {"id": 1, "created": 1, "title": "b"},
            {"id": 2, "created": 2, "title": "z"},
            {"id": 3, "created": 1, "title": "a"},
            {"id": 4, "created": None, "title": "q"},
        ]
        import functools

        order = [r["id"] for r in sorted(rows, key=functools.cmp_to_key(cmp))]
        self.assertEqual(order, [2, 3, 1, 4])  # desc created; None last when descending


if __name__ == "__main__":
    unittest.main()
