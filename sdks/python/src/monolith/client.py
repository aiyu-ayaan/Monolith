from __future__ import annotations

import urllib.parse
from typing import Any

from monolith.analytics import AnalyticsService
from monolith.auth import AuthService
from monolith.collections import Collection
from monolith.files import FilesService
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
        self._auth_services: dict[str, AuthService] = {}
        self._collections: dict[str, Collection[Any]] = {}

    def auth(self, collection: str = "users") -> AuthService:
        if collection not in self._auth_services:
            self._auth_services[collection] = AuthService(self.http, collection)
        return self._auth_services[collection]

    def collection(self, name: str) -> Collection[Any]:
        if name not in self._collections:
            self._collections[name] = Collection(name, self.http)
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
        self.analytics.close()
        self.http.close()

    def __enter__(self) -> MonolithClient:
        return self

    def __exit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        self.close()


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
    )
