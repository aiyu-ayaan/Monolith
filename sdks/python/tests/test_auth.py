from __future__ import annotations

import json
import os
import tempfile
import threading
import time
import unittest
from datetime import datetime, timedelta, timezone
from typing import Any

import httpx

import monolith
from monolith import (
    AuthStatus,
    FileSessionStore,
    Monolith,
    MonolithError,
)

APP = "/api/v1/apps/shop/auth"


def iso(delta_seconds: float) -> str:
    t = datetime.now(timezone.utc) + timedelta(seconds=delta_seconds)
    return t.strftime("%Y-%m-%dT%H:%M:%S.%f") + "Z"


def problem(status: int, title: str = "x", **extra: Any) -> httpx.Response:
    body = {"type": "about:blank", "title": title, "status": status, **extra}
    return httpx.Response(status, json=body, headers={"content-type": "application/problem+json"})


class FakeServer:
    """Minimal app-user auth server. Access tokens are `at<N>`; `valid` is the set the server accepts."""

    def __init__(self) -> None:
        self.n = 0
        self.valid: set[str] = set()
        self.refresh_tokens: set[str] = set()
        self.calls: list[tuple[str, str]] = []
        self.refresh_delay = 0.0
        self.record_hits = 0
        self.lock = threading.Lock()
        self.user = {"id": "u1", "email": "a@b.co", "name": "A", "providers": ["password"]}

    def session(self, ttl: float = 900) -> dict[str, Any]:
        with self.lock:
            self.n += 1
            at, rt = f"at{self.n}", f"rt{self.n}"
            self.valid.add(at)
            self.refresh_tokens.add(rt)
        return {"token": at, "refreshToken": rt, "expiresAt": iso(ttl), "user": self.user}

    def handler(self, req: httpx.Request) -> httpx.Response:
        path = req.url.path
        self.calls.append((req.method, path))
        body = json.loads(req.content) if req.content else {}
        bearer = req.headers.get("authorization", "").removeprefix("Bearer ")
        if path == f"{APP}/providers":
            return httpx.Response(
                200,
                json={
                    "app": "shop",
                    "emailPassword": {"enabled": True, "allowSignup": True, "minPasswordLength": 8},
                    "providers": [{"id": "google", "name": "Google", "startPath": "x"}],
                },
            )
        if path == f"{APP}/register":
            if body["email"] == "taken@b.co":
                return problem(409, "Email taken")
            return httpx.Response(201, json=self.session())
        if path == f"{APP}/login":
            if body["password"] != "secret123":
                return problem(401, "Invalid credentials")
            return httpx.Response(200, json=self.session())
        if path == f"{APP}/refresh":
            time.sleep(self.refresh_delay)
            with self.lock:
                if body["refreshToken"] not in self.refresh_tokens:
                    return problem(401, "Invalid refresh token")
                self.refresh_tokens.discard(body["refreshToken"])  # single use
            return httpx.Response(200, json=self.session())
        if path == f"{APP}/logout":
            self.refresh_tokens.discard(body["refreshToken"])
            return httpx.Response(204)
        if path == f"{APP}/me":
            if bearer not in self.valid:
                return problem(401)
            if req.method == "PATCH":
                self.user = {**self.user, "name": body["name"]}
            return httpx.Response(200, json=self.user)
        if path == f"{APP}/password":
            if bearer not in self.valid:
                return problem(401)
            if body["newPassword"] == "short":
                return problem(400, "Bad password")
            self.refresh_tokens.clear()
            return httpx.Response(204)
        if path == "/api/v1/apps/shop/collections/tasks/records":
            self.record_hits += 1
            if bearer not in self.valid:
                return problem(401)
            return httpx.Response(200, json={"items": [], "bearer": bearer})
        return httpx.Response(404)

    def init(self, **kw: Any) -> monolith.MonolithClient:
        return Monolith.init(
            "https://api.example.com",
            "shop",
            client=httpx.Client(transport=httpx.MockTransport(self.handler)),
            **kw,
        )

    def count(self, path_tail: str) -> int:
        return sum(1 for _, p in self.calls if p == f"{APP}/{path_tail}")


class TestAppUserAuth(unittest.TestCase):
    def test_providers_sign_up_in_out_and_state(self) -> None:
        srv = FakeServer()
        mono = srv.init()
        auth = mono.auth()
        states: list[Any] = []
        auth.on_state_change(states.append)
        self.assertEqual(states[0].status, AuthStatus.SIGNED_OUT)

        p = auth.providers()
        self.assertEqual(p["providers"][0]["id"], "google")

        s = auth.sign_up("a@b.co", "secret123", name="A")
        self.assertEqual(s.user["id"], "u1")
        self.assertEqual(auth.user, srv.user)
        self.assertEqual(states[-1].status, AuthStatus.SIGNED_IN)
        self.assertEqual(auth.access_token(), s.token)

        auth.sign_out()
        self.assertEqual(states[-1].status, AuthStatus.SIGNED_OUT)
        self.assertIsNone(auth.access_token())
        self.assertEqual(srv.count("logout"), 1)
        self.assertNotIn(s.refresh_token, srv.refresh_tokens)

        s2 = auth.sign_in("a@b.co", "secret123")
        self.assertTrue(states[-1].is_signed_in)
        self.assertEqual(mono.collection("tasks").list()["bearer"], s2.token)

    def test_error_mapping(self) -> None:
        srv = FakeServer()
        auth = srv.init().auth()
        with self.assertRaises(MonolithError) as c:
            auth.sign_in("a@b.co", "wrong")
        self.assertEqual(c.exception.status, 401)
        self.assertEqual(auth.state.status, AuthStatus.SIGNED_OUT)
        with self.assertRaises(MonolithError) as c:
            auth.sign_up("taken@b.co", "secret123")
        self.assertEqual(c.exception.status, 409)

    def test_me_update_profile_change_password(self) -> None:
        srv = FakeServer()
        auth = srv.init().auth()
        auth.sign_in("a@b.co", "secret123")
        self.assertEqual(auth.update_profile("Zed")["name"], "Zed")
        self.assertEqual(auth.user["name"], "Zed")
        self.assertEqual(auth.me()["name"], "Zed")
        with self.assertRaises(MonolithError) as c:
            auth.change_password("secret123", "short")
        self.assertEqual(c.exception.status, 400)
        self.assertTrue(auth.state.is_signed_in)
        auth.change_password("secret123", "newsecret123")
        self.assertEqual(auth.state.status, AuthStatus.SIGNED_OUT)

    def test_refresh_rotation_and_proactive_refresh(self) -> None:
        srv = FakeServer()
        auth = srv.init().auth()
        first = auth.sign_in("a@b.co", "secret123")
        second = auth.refresh()
        self.assertNotEqual(first.token, second.token)
        self.assertNotEqual(first.refresh_token, second.refresh_token)
        self.assertNotIn(first.refresh_token, srv.refresh_tokens)  # rotated

        # Access token about to expire: access_token() refreshes before returning.
        auth._session.expires_at = datetime.now(timezone.utc).timestamp() + 5
        before = srv.count("refresh")
        token = auth.access_token()
        self.assertEqual(srv.count("refresh"), before + 1)
        self.assertNotEqual(token, second.token)

    def test_single_flight_refresh(self) -> None:
        srv = FakeServer()
        srv.refresh_delay = 0.2
        mono = srv.init()
        auth = mono.auth()
        auth.sign_in("a@b.co", "secret123")
        auth._session.expires_at = datetime.now(timezone.utc).timestamp() + 1
        results: list[Any] = []

        def work() -> None:
            results.append(auth.access_token())

        threads = [threading.Thread(target=work) for _ in range(8)]
        [t.start() for t in threads]
        [t.join() for t in threads]
        self.assertEqual(srv.count("refresh"), 1)
        self.assertEqual(len(set(results)), 1)

    def test_single_flight_after_401(self) -> None:
        srv = FakeServer()
        srv.refresh_delay = 0.2
        mono = srv.init()
        auth = mono.auth()
        auth.sign_in("a@b.co", "secret123")
        srv.valid.clear()  # server revoked the access token; refresh token still good
        out: list[Any] = []
        threads = [
            threading.Thread(target=lambda: out.append(mono.collection("tasks").list()))
            for _ in range(6)
        ]
        [t.start() for t in threads]
        [t.join() for t in threads]
        self.assertEqual(len(out), 6)
        self.assertEqual(srv.count("refresh"), 1)

    def test_401_retry_once_only(self) -> None:
        srv = FakeServer()
        mono = srv.init()
        auth = mono.auth()
        auth.sign_in("a@b.co", "secret123")
        srv.valid.clear()
        # Make every new token invalid too: the retry also gets 401, and we stop there.
        orig = srv.session

        def bad_session(ttl: float = 900) -> dict[str, Any]:
            s = orig(ttl)
            srv.valid.discard(s["token"])
            return s

        srv.session = bad_session  # type: ignore[method-assign]
        with self.assertRaises(MonolithError) as c:
            mono.collection("tasks").list()
        self.assertEqual(c.exception.status, 401)
        self.assertEqual(srv.record_hits, 2)
        self.assertEqual(srv.count("refresh"), 1)

    def test_dead_refresh_token_signs_out(self) -> None:
        srv = FakeServer()
        mono = srv.init()
        auth = mono.auth()
        auth.sign_in("a@b.co", "secret123")
        srv.valid.clear()
        srv.refresh_tokens.clear()
        with self.assertRaises(MonolithError) as c:
            mono.collection("tasks").list()
        self.assertEqual(c.exception.status, 401)
        self.assertEqual(auth.state.status, AuthStatus.SIGNED_OUT)

    def test_restore_from_file_store(self) -> None:
        srv = FakeServer()
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "sub", "session.json")
            auth = srv.init(storage=path).auth()
            s = auth.sign_in("a@b.co", "secret123")
            self.assertEqual(oct(os.stat(path).st_mode & 0o777), "0o600")

            # A fresh process: the token is restored without a network call.
            calls = len(srv.calls)
            mono2 = srv.init(storage=FileSessionStore(path))
            self.assertEqual(len(srv.calls), calls)
            self.assertEqual(mono2.auth().state.status, AuthStatus.SIGNED_IN)
            self.assertEqual(mono2.auth().user["id"], "u1")
            self.assertEqual(mono2.collection("tasks").list()["bearer"], s.token)

            # Expired access token + valid refresh token: refreshed on first use, store updated.
            with open(path) as f:
                stored = json.load(f)
            stored["expiresAt"] = iso(-10)
            with open(path, "w") as f:
                json.dump(stored, f)
            mono3 = srv.init(storage=path)
            res = mono3.collection("tasks").list()
            self.assertNotEqual(res["bearer"], s.token)
            with open(path) as f:
                self.assertEqual(json.load(f)["token"], res["bearer"])

            mono3.auth().sign_out()
            self.assertFalse(os.path.exists(path))
            self.assertEqual(srv.init(storage=path).auth().state.status, AuthStatus.SIGNED_OUT)

    def test_corrupt_file_store_is_signed_out(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "s.json")
            with open(path, "w") as f:
                f.write("{not json")
            self.assertEqual(FakeServer().init(storage=path).auth().state.status, AuthStatus.SIGNED_OUT)

    def test_provider_url_and_redirect(self) -> None:
        srv = FakeServer()
        auth = srv.init().auth()
        url = auth.provider_sign_in_url("google", "http://127.0.0.1:9000/cb")
        self.assertEqual(
            url,
            "https://api.example.com/api/v1/apps/shop/auth/oauth/google/start"
            "?redirectUri=http%3A%2F%2F127.0.0.1%3A9000%2Fcb",
        )
        valid = srv.session()
        frag = (
            f"token={valid['token']}&refreshToken={valid['refreshToken']}"
            f"&expiresAt={valid['expiresAt'].replace(':', '%3A')}"
        )
        for arg in (f"http://127.0.0.1:9000/cb#{frag}", f"#{frag}", frag):
            auth.sign_out()
            s = auth.handle_redirect(arg)
            self.assertEqual(s.token, valid["token"])
            self.assertEqual(s.refresh_token, valid["refreshToken"])
            self.assertIsNotNone(s.expires_at)
            self.assertEqual(auth.user["id"], "u1")  # fetched from /me
            self.assertTrue(auth.state.is_signed_in)

    def test_redirect_errors(self) -> None:
        auth = FakeServer().init().auth()
        with self.assertRaises(MonolithError) as c:
            auth.handle_redirect("http://x/cb#error=oauth_failed&error_description=email%20taken")
        self.assertEqual(c.exception.type, "oauth_failed")
        self.assertEqual(c.exception.detail, "email taken")
        with self.assertRaises(MonolithError) as c:
            auth.handle_redirect("http://x/cb")
        self.assertEqual(c.exception.type, "invalid_redirect")
        self.assertEqual(auth.state.status, AuthStatus.SIGNED_OUT)

    def test_module_level_init(self) -> None:
        self.assertTrue(callable(monolith.init))
        mono = monolith.init("https://x.test", "shop")
        self.assertEqual(mono.http.app, "shop")
        self.assertEqual(mono.auth().state.status, AuthStatus.SIGNED_OUT)


if __name__ == "__main__":
    unittest.main()
