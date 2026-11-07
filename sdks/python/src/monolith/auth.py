from __future__ import annotations

import urllib.parse
from dataclasses import dataclass, field
from typing import Any, Callable

from monolith.http import HttpClient

AuthChangeListener = Callable[[str | None], None]


@dataclass
class AuthSession:
    token: str
    user: dict[str, Any] | None = None
    raw: dict[str, Any] = field(default_factory=dict)

    def __getitem__(self, item: str) -> Any:
        if hasattr(self, item):
            return getattr(self, item)
        return self.raw[item]

    def get(self, item: str, default: Any = None) -> Any:
        if hasattr(self, item):
            val = getattr(self, item)
            return val if val is not None else default
        return self.raw.get(item, default)


class AuthService:
    def __init__(self, http: HttpClient, collection: str = "users") -> None:
        self._http = http
        self.auth_collection = collection
        self._listeners: set[AuthChangeListener] = set()

    @property
    def token(self) -> str | None:
        return self._http.get_token()

    def set_token(self, token: str | None) -> None:
        self._http.set_token(token)
        self._notify(token)

    def on_change(self, listener: AuthChangeListener) -> Callable[[], None]:
        self._listeners.add(listener)

        def unsubscribe() -> None:
            self._listeners.discard(listener)

        return unsubscribe

    def _notify(self, token: str | None) -> None:
        for listener in list(self._listeners):
            try:
                listener(token)
            except Exception:
                pass

    def sign_in_with_password(self, email: str, password: str) -> AuthSession:
        is_system = self.auth_collection in ("_superusers", "system")
        path = (
            "/api/v1/system/login"
            if is_system
            else f"/api/v1/apps/{urllib.parse.quote(self._http.app, safe='')}/auth/login"
        )

        res = self._http.request(
            "POST",
            path,
            body={"email": email, "password": password},
        )

        token = ""
        user = None
        raw_dict = {}
        if isinstance(res, dict):
            raw_dict = res
            token = res.get("token", "")
            user = res.get("user")

        if token:
            self.set_token(token)

        return AuthSession(token=token, user=user, raw=raw_dict)

    def install(self, token: str, email: str, password: str) -> AuthSession:
        res = self._http.request(
            "POST",
            "/api/v1/system/install",
            body={"token": token, "email": email, "password": password},
        )

        new_token = ""
        user = None
        raw_dict = {}
        if isinstance(res, dict):
            raw_dict = res
            new_token = res.get("token", "")
            user = res.get("user")

        if new_token:
            self.set_token(new_token)

        return AuthSession(token=new_token, user=user, raw=raw_dict)

    def sign_out(self) -> None:
        self.set_token(None)
