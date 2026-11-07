from __future__ import annotations

import os
import urllib.parse
from typing import Any

from monolith.analytics import AnalyticsService
from monolith.auth import AuthService
from monolith.collections import Collection
from monolith.files import FilesService
from monolith.realtime import RealtimeService
from monolith.session import FileSessionStore, MemorySessionStore, SessionStore
from monolith.http import (
    BatchOperation,
    BatchResponseItem,
    ClientOptions,
    HttpClient,
)


class MonolithClient:
    def __init__(
        self,
        options: ClientOptions | None = None,
        *,
        url: str | None = None,
        app: str = "default",
        key: str | None = None,
        token: str | None = None,
        headers: dict[str, str] | None = None,
        timeout: float = 30.0,
        client: Any = None,
        session_store: SessionStore | None = None,
    ) -> None:
        if options is None:
            if url is None:
                raise ValueError("Must provide either ClientOptions or url parameter")
            options = ClientOptions(
                url=url,
                app=app,
                key=key,
                token=token,
                headers=headers,
                timeout=timeout,
                client=client,
            )

        self.http = HttpClient(options)
        self.files = FilesService(self.http)
        self.analytics = AnalyticsService(self.http)
        self.realtime = RealtimeService(self.http)
        self._auth_services: dict[str, AuthService] = {}
        self._collections: dict[str, Collection[Any]] = {}
        self._session_store = session_store
        if session_store is not None:
            self.auth()  # restores the stored session and installs auto refresh

    def auth(self, collection: str = "users") -> AuthService:
        if collection not in self._auth_services:
            self._auth_services[collection] = AuthService(self.http, collection, self._session_store)
        return self._auth_services[collection]

    def collection(self, name: str) -> Collection[Any]:
        if name not in self._collections:
            self._collections[name] = Collection(name, self.http, self.realtime)
        return self._collections[name]

    def batch(self, operations: list[dict[str, Any] | BatchOperation]) -> list[Any]:
        app_encoded = urllib.parse.quote(self.http.app, safe="")
        path = f"/api/v1/apps/{app_encoded}/batch"

        ops: list[dict[str, Any]] = []
        for op in operations:
            if isinstance(op, BatchOperation):
                ops.append({"method": op.method, "path": op.path, "body": op.body})
            elif isinstance(op, dict):
                ops.append(op)

        res = self.http.request("POST", path, body={"operations": ops})
        if isinstance(res, list):
            return res
        return []

    def close(self) -> None:
        self.realtime.close()
        self.analytics.close()
        self.http.close()

    def __enter__(self) -> MonolithClient:
        return self

    def __exit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        self.close()


class Monolith:
    """One-line start: `mono = Monolith.init(url="https://api.example.com", app="shop")`."""

    @staticmethod
    def init(
        url: str,
        app: str,
        *,
        key: str | None = None,
        storage: SessionStore | str | os.PathLike[str] | None = None,
        headers: dict[str, str] | None = None,
        timeout: float = 30.0,
        client: Any = None,
    ) -> MonolithClient:
        """Ready client for app users. `storage` is a SessionStore or a file path
        (default: memory). A stored session is restored and refreshed automatically."""
        if isinstance(storage, (str, os.PathLike)):
            storage = FileSessionStore(storage)
        return MonolithClient(
            url=url,
            app=app,
            key=key,
            headers=headers,
            timeout=timeout,
            client=client,
            session_store=storage if storage is not None else MemorySessionStore(),
        )


def init(*args: Any, **kwargs: Any) -> MonolithClient:
    return Monolith.init(*args, **kwargs)


def create_client(
    options: ClientOptions | None = None,
    *,
    url: str | None = None,
    app: str = "default",
    key: str | None = None,
    token: str | None = None,
    headers: dict[str, str] | None = None,
    timeout: float = 30.0,
    client: Any = None,
    session_store: SessionStore | None = None,
) -> MonolithClient:
    return MonolithClient(
        options=options,
        url=url,
        app=app,
        key=key,
        token=token,
        headers=headers,
        timeout=timeout,
        client=client,
        session_store=session_store,
    )
