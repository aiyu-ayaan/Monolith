from __future__ import annotations

import json
import unittest
from typing import Any

import httpx

from monolith import (
    BatchOperation,
    ClientOptions,
    MonolithClient,
    MonolithError,
    create_client,
)


class TestMonolithClient(unittest.TestCase):
    def test_client_initialization(self) -> None:
        client = create_client(
            url="https://api.example.com",
            app="testapp",
        )

        self.assertIsInstance(client, MonolithClient)
        self.assertEqual(client.http.base_url, "https://api.example.com")
        self.assertEqual(client.http.app, "testapp")
        self.assertIsNone(client.http.get_token())
        self.assertIsNone(client.http.get_api_key())

    def test_client_with_options(self) -> None:
        opts = ClientOptions(
            url="https://api.example.com/",
            app="customapp",
            key="mk_pub_123",
            token="tok_456",
            headers={"X-Custom": "header-val"},
        )
        client = create_client(options=opts)

        self.assertEqual(client.http.base_url, "https://api.example.com")
        self.assertEqual(client.http.app, "customapp")
        self.assertEqual(client.http.get_api_key(), "mk_pub_123")
        self.assertEqual(client.http.get_token(), "tok_456")
        self.assertEqual(client.http.default_headers.get("X-Custom"), "header-val")

    def test_build_url_with_query_parameters(self) -> None:
        client = create_client(
            url="https://api.example.com/",
            app="testapp",
        )

        url = client.http.build_url(
            "/test",
            {
                "filter": "status = 'active'",
                "limit": 10,
                "tags": ["a", "b"],
                "empty": None,
                "active": True,
            },
        )

        self.assertEqual(
            url,
            "https://api.example.com/test?filter=status+%3D+%27active%27&limit=10&tags=a&tags=b&active=true",
        )

    def test_collections_crud(self) -> None:
        def handler(request: httpx.Request) -> httpx.Response:
            method = request.method
            url = str(request.url)

            if "/collections/todos/records:count" in url:
                return httpx.Response(200, json={"count": 42})

            if "/collections/todos/records/rec_123" in url and method == "GET":
                return httpx.Response(
                    200,
                    json={
                        "id": "rec_123",
                        "title": "Task 1",
                        "created": "2026-01-01",
                        "updated": "2026-01-01",
                        "_v": 1,
                    },
                )

            if "/collections/todos/records" in url and method == "POST":
                body = json.loads(request.content.decode("utf-8"))
                return httpx.Response(
                    201,
                    json={
                        "id": "rec_new",
                        "created": "2026-01-01",
                        "updated": "2026-01-01",
                        "_v": 1,
                        **body,
                    },
                )

            if "/collections/todos/records/rec_123" in url and method == "PATCH":
                body = json.loads(request.content.decode("utf-8"))
                return httpx.Response(
                    200,
                    json={
                        "id": "rec_123",
                        "updated": "2026-01-02",
                        "_v": 2,
                        **body,
                    },
                )

            if "/collections/todos/records/rec_123" in url and method == "PUT":
                body = json.loads(request.content.decode("utf-8"))
                return httpx.Response(
                    200,
                    json={
                        "id": "rec_123",
                        "updated": "2026-01-02",
                        "_v": 2,
                        **body,
                    },
                )

            if "/collections/todos/records/rec_123" in url and method == "DELETE":
                return httpx.Response(204)

            if "/collections/todos/records" in url and method == "GET":
                return httpx.Response(
                    200,
                    json={
                        "items": [{"id": "rec_1", "title": "Item 1"}],
                        "total": 1,
                        "cursor": "cur_1",
                        "nextCursor": "cur_2",
                    },
                )

            return httpx.Response(404, text="Not found")

        mock_client = httpx.Client(transport=httpx.MockTransport(handler))
        client = create_client(
            url="https://api.example.com",
            app="testapp",
            client=mock_client,
        )

        todos = client.collection("todos")

        # List
        list_res = todos.list(filter="done = false", limit=5)
        self.assertEqual(len(list_res.items), 1)
        self.assertEqual(list_res.items[0]["id"], "rec_1")
        self.assertEqual(list_res.total, 1)
        self.assertEqual(list_res.cursor, "cur_1")
        self.assertEqual(list_res.next_cursor, "cur_2")

        # Count
        count = todos.count()
        self.assertEqual(count, 42)

        # GetOne
        item = todos.get_one("rec_123")
        self.assertEqual(item["id"], "rec_123")
        self.assertEqual(item["title"], "Task 1")

        # Create
        created = todos.create({"title": "New Task"})
        self.assertEqual(created["id"], "rec_new")
        self.assertEqual(created["title"], "New Task")

        # Update
        updated = todos.update("rec_123", {"title": "Updated Task"})
        self.assertEqual(updated["id"], "rec_123")
        self.assertEqual(updated["title"], "Updated Task")

        # Replace
        replaced = todos.replace("rec_123", {"title": "Replaced Task"})
        self.assertEqual(replaced["id"], "rec_123")
        self.assertEqual(replaced["title"], "Replaced Task")

        # Delete
        deleted = todos.delete("rec_123")
        self.assertTrue(deleted)

    def test_problem_json_error(self) -> None:
        def handler(request: httpx.Request) -> httpx.Response:
            return httpx.Response(
                400,
                json={
                    "type": "validation_error",
                    "title": "Validation Failed",
                    "status": 400,
                    "detail": "The title field is required",
                    "requestId": "req_xyz",
                    "errors": [
                        {
                            "field": "title",
                            "code": "required",
                            "message": "must not be empty",
                        }
                    ],
                },
                headers={
                    "content-type": "application/problem+json",
                    "x-request-id": "req_xyz",
                },
            )

        mock_client = httpx.Client(transport=httpx.MockTransport(handler))
        client = create_client(
            url="https://api.example.com",
            app="testapp",
            client=mock_client,
        )

        with self.assertRaises(MonolithError) as cm:
            client.collection("todos").create({})

        err = cm.exception
        self.assertEqual(err.status, 400)
        self.assertEqual(err.type, "validation_error")
        self.assertEqual(err.request_id, "req_xyz")
        self.assertEqual(err.requestId, "req_xyz")
        self.assertEqual(err.detail, "The title field is required")
        self.assertEqual(err.get_field_error("title"), "must not be empty")
        self.assertIsNone(err.get_field_error("nonexistent"))

    def test_auth_service(self) -> None:
        def handler(request: httpx.Request) -> httpx.Response:
            url = str(request.url)
            body = json.loads(request.content.decode("utf-8"))

            if "/system/login" in url:
                self.assertEqual(body["email"], "admin@example.com")
                return httpx.Response(200, json={"token": "admin_jwt_999"})

            if "/system/install" in url:
                self.assertEqual(body["token"], "setup_token")
                return httpx.Response(200, json={"token": "installed_jwt_000"})

            if "/auth/login" in url:
                self.assertEqual(body["email"], "test@example.com")
                self.assertEqual(body["password"], "secret123")
                return httpx.Response(
                    200,
                    json={"token": "jwt_token_123", "user": {"id": "u_1"}},
                )

            return httpx.Response(404)

        mock_client = httpx.Client(transport=httpx.MockTransport(handler))
        client = create_client(
            url="https://api.example.com",
            app="testapp",
            client=mock_client,
        )

        tokens_received: list[str | None] = []

        def on_change(token: str | None) -> None:
            tokens_received.append(token)

        auth = client.auth("users")
        unsub = auth.on_change(on_change)

        # Sign in
        session = auth.sign_in_with_password("test@example.com", "secret123")
        self.assertEqual(session.token, "jwt_token_123")
        self.assertEqual(session.user, {"id": "u_1"})
        self.assertEqual(client.http.get_token(), "jwt_token_123")
        self.assertEqual(tokens_received, ["jwt_token_123"])

        # Sign out
        auth.sign_out()
        self.assertIsNone(client.http.get_token())
        self.assertEqual(tokens_received, ["jwt_token_123", None])

        # Unsubscribe
        unsub()
        auth.set_token("another_token")
        self.assertEqual(tokens_received, ["jwt_token_123", None])

        # System login
        system_auth = client.auth("system")
        sys_session = system_auth.sign_in_with_password("admin@example.com", "pass")
        self.assertEqual(sys_session.token, "admin_jwt_999")

        # Install
        install_session = system_auth.install("setup_token", "admin@example.com", "pass")
        self.assertEqual(install_session.token, "installed_jwt_000")

    def test_files_service(self) -> None:
        def handler(request: httpx.Request) -> httpx.Response:
            url = str(request.url)
            method = request.method

            if method == "PUT" and "/storage/buckets/avatars/objects/user1.png" in url:
                self.assertEqual(request.headers.get("content-type"), "image/png")
                self.assertEqual(request.content, b"fake-png-data")
                return httpx.Response(
                    200,
                    json={
                        "bucket": "avatars",
                        "key": "user1.png",
                        "size": 13,
                        "contentType": "image/png",
                        "blake3": "hash123",
                        "created": "2026-01-01",
                        "updated": "2026-01-01",
                    },
                )

            if method == "DELETE" and "/storage/buckets/avatars/objects/user1.png" in url:
                return httpx.Response(204)

            if method == "GET" and "/storage/buckets/avatars/objects" in url:
                self.assertIn("prefix=u", url)
                return httpx.Response(
                    200,
                    json={
                        "items": [
                            {
                                "bucket": "avatars",
                                "key": "user1.png",
                                "size": 13,
                                "contentType": "image/png",
                                "blake3": "hash123",
                                "created": "2026-01-01",
                                "updated": "2026-01-01",
                            }
                        ],
                        "nextCursor": None,
                    },
                )

            return httpx.Response(404)

        mock_client = httpx.Client(transport=httpx.MockTransport(handler))
        client = create_client(
            url="https://api.example.com",
            app="testapp",
            client=mock_client,
        )

        # get_url
        url = client.files.get_url("avatars", "user1.png", thumb="100x100")
        self.assertEqual(
            url,
            "https://api.example.com/api/v1/apps/testapp/storage/buckets/avatars/objects/user1.png?thumb=100x100",
        )

        # upload
        obj = client.files.upload("avatars", "user1.png", b"fake-png-data", content_type="image/png")
        self.assertEqual(obj.bucket, "avatars")
        self.assertEqual(obj.key, "user1.png")
        self.assertEqual(obj.size, 13)
        self.assertEqual(obj.content_type, "image/png")

        # list
        list_res = client.files.list("avatars", prefix="u")
        self.assertEqual(len(list_res.items), 1)
        self.assertEqual(list_res.items[0].key, "user1.png")

        # delete
        deleted = client.files.delete("avatars", "user1.png")
        self.assertTrue(deleted)

    def test_analytics_service(self) -> None:
        captured_events: list[dict[str, Any]] = []

        def handler(request: httpx.Request) -> httpx.Response:
            url = str(request.url)
            if "/analytics/events" in url:
                body = json.loads(request.content.decode("utf-8"))
                captured_events.extend(body["events"])
                return httpx.Response(204)
            return httpx.Response(404)

        mock_client = httpx.Client(transport=httpx.MockTransport(handler))
        client = create_client(
            url="https://api.example.com",
            app="testapp",
            client=mock_client,
        )

        client.analytics.track("user_signup", {"method": "email"})
        client.analytics.track("button_click", {"button": "submit"})
        client.analytics.flush()

        self.assertEqual(len(captured_events), 2)
        self.assertEqual(captured_events[0]["event"], "user_signup")
        self.assertEqual(captured_events[0]["properties"], {"method": "email"})
        self.assertEqual(captured_events[1]["event"], "button_click")
        self.assertEqual(captured_events[1]["properties"], {"button": "submit"})

    def test_batch_operations(self) -> None:
        def handler(request: httpx.Request) -> httpx.Response:
            url = str(request.url)
            if "/batch" in url:
                body = json.loads(request.content.decode("utf-8"))
                self.assertEqual(len(body["operations"]), 2)
                return httpx.Response(
                    200,
                    json=[
                        {"status": 201, "body": {"id": "1"}},
                        {"status": 204, "body": None},
                    ],
                )
            return httpx.Response(404)

        mock_client = httpx.Client(transport=httpx.MockTransport(handler))
        client = create_client(
            url="https://api.example.com",
            app="testapp",
            client=mock_client,
        )

        res = client.batch(
            [
                BatchOperation(method="POST", path="/collections/todos/records", body={"title": "Task A"}),
                {"method": "DELETE", "path": "/collections/todos/records/rec_old"},
            ]
        )

        self.assertEqual(len(res), 2)
        self.assertEqual(res[0]["status"], 201)
        self.assertEqual(res[1]["status"], 204)

    def test_client_context_manager(self) -> None:
        client = create_client(url="https://api.example.com", app="testapp")
        with client as c:
            self.assertEqual(c.http.app, "testapp")


if __name__ == "__main__":
    unittest.main()
