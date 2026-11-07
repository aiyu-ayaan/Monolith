from __future__ import annotations

import random
import time
import urllib.parse
from dataclasses import dataclass, field
from typing import Any

import httpx

from monolith.errors import MonolithError, Problem


@dataclass
class ClientOptions:
    url: str
    app: str = "default"
    key: str | None = None
    token: str | None = None
    headers: dict[str, str] | None = None
    timeout: float = 30.0
    client: httpx.Client | None = None


@dataclass
class ListOptions:
    page: int | None = None
    per_page: int | None = None
    cursor: str | None = None
    limit: int | None = None
    filter: str | None = None
    sort: str | None = None
    expand: str | None = None
    fields: str | None = None
    total: str | None = None


@dataclass
class ListResult:
    items: list[Any]
    total: int | None = None
    cursor: str | None = None
    next_cursor: str | None = None
    prev_cursor: str | None = None
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


@dataclass
class CountOptions:
    filter: str | None = None


@dataclass
class BatchOperation:
    method: str
    path: str
    body: Any = None


@dataclass
class BatchResponseItem:
    status: int
    body: Any = None


@dataclass
class StorageObject:
    bucket: str
    key: str
    size: int
    content_type: str = ""
    blake3: str = ""
    created: str = ""
    updated: str = ""
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


@dataclass
class StorageListOptions:
    prefix: str | None = None
    cursor: str | None = None
    limit: int | None = None


@dataclass
class StorageListResult:
    items: list[StorageObject] = field(default_factory=list)
    next_cursor: str | None = None
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


class HttpClient:
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
        client: httpx.Client | None = None,
    ) -> None:
        # Set by AuthService for app users: ensure_fresh() and refresh_after_unauthorized(token).
        self.auth_hook: Any = None
        if options is not None:
            self.base_url = options.url.rstrip("/")
            self.app = options.app
            self.token = options.token
            self.api_key = options.key
            self.default_headers = dict(options.headers) if options.headers else {}
            self.timeout = options.timeout
            if options.client is not None:
                self._client = options.client
                self._owns_client = False
            else:
                self._client = httpx.Client(timeout=self.timeout)
                self._owns_client = True
        else:
            if url is None:
                raise ValueError("Must provide either ClientOptions or url parameter")
            self.base_url = url.rstrip("/")
            self.app = app
            self.token = token
            self.api_key = key
            self.default_headers = dict(headers) if headers else {}
            self.timeout = timeout
            if client is not None:
                self._client = client
                self._owns_client = False
            else:
                self._client = httpx.Client(timeout=self.timeout)
                self._owns_client = True

    def set_token(self, token: str | None) -> None:
        self.token = token

    def get_token(self) -> str | None:
        return self.token

    def set_api_key(self, key: str | None) -> None:
        self.api_key = key

    def get_api_key(self) -> str | None:
        return self.api_key

    def build_url(self, path: str, query: dict[str, Any] | None = None) -> str:
        clean_path = path if path.startswith("/") else f"/{path}"
        base = f"{self.base_url}{clean_path}"
        if not query:
            return base

        params: list[tuple[str, str]] = []
        for k, v in query.items():
            if v is None:
                continue
            if isinstance(v, (list, tuple)):
                for item in v:
                    if item is not None:
                        params.append((k, str(item)))
            elif isinstance(v, bool):
                params.append((k, "true" if v else "false"))
            else:
                params.append((k, str(v)))

        if params:
            qs = urllib.parse.urlencode(params)
            return f"{base}?{qs}"
        return base

    def request(
        self,
        method: str,
        path: str,
        *,
        query: dict[str, Any] | None = None,
        body: Any = None,
        headers: dict[str, str] | None = None,
        content: bytes | str | None = None,
        auth_retry: bool = True,
    ) -> Any:
        """With an app-user session: refreshes a near-expiry token first and retries once after a 401."""
        hook = self.auth_hook
        if hook is None or not auth_retry or (
            headers and any(k.lower() == "authorization" for k in headers)
        ):
            return self._send(method, path, query, body, headers, content)
        hook.ensure_fresh()
        used = self.token
        try:
            return self._send(method, path, query, body, headers, content)
        except MonolithError as err:
            if err.status == 401 and used and hook.refresh_after_unauthorized(used):
                return self._send(method, path, query, body, headers, content)
            raise

    def _send(
        self,
        method: str,
        path: str,
        query: dict[str, Any] | None,
        body: Any,
        headers: dict[str, str] | None,
        content: bytes | str | None,
    ) -> Any:
        url = self.build_url(path, query)
        req_headers = dict(self.default_headers)
        if headers:
            req_headers.update(headers)

        auth_token = self.token or self.api_key
        existing_auth = {k.lower(): k for k in req_headers}
        if auth_token and "authorization" not in existing_auth:
            req_headers["Authorization"] = f"Bearer {auth_token}"

        req_json: Any = None
        req_content: bytes | str | None = content

        if body is not None:
            if isinstance(body, (bytes, bytearray)):
                req_content = bytes(body)
            elif isinstance(body, str):
                req_content = body
            else:
                req_json = body
                if "content-type" not in existing_auth:
                    req_headers["Content-Type"] = "application/json"

        is_get = method.upper() == "GET"
        max_attempts = 4 if is_get else 1

        for attempt in range(max_attempts):
            res: httpx.Response
            try:
                res = self._client.request(
                    method=method,
                    url=url,
                    headers=req_headers,
                    json=req_json,
                    content=req_content,
                )
            except httpx.TransportError as err:
                if attempt < max_attempts - 1:
                    delay = min(0.1 * (2**attempt) + random.uniform(0, 0.05), 2.0)
                    time.sleep(delay)
                    continue
                raise MonolithError(
                    Problem(
                        type="network_error",
                        title="Network Error",
                        status=0,
                        detail=str(err),
                    )
                ) from err

            if res.status_code == 204:
                return None

            content_type = res.headers.get("content-type", "").lower()
            is_json = "application/json" in content_type or "problem+json" in content_type

            if res.is_error:
                # Retry 503 for GET
                if is_get and res.status_code == 503 and attempt < max_attempts - 1:
                    retry_after = res.headers.get("retry-after")
                    if retry_after and retry_after.isdigit():
                        delay = float(retry_after)
                    else:
                        delay = min(0.1 * (2**attempt) + random.uniform(0, 0.05), 2.0)
                    time.sleep(delay)
                    continue

                problem: Problem
                if is_json:
                    try:
                        data = res.json()
                        problem = Problem.from_dict(data, status_code=res.status_code)
                    except Exception:
                        problem = Problem(
                            type="http_error",
                            title=res.reason_phrase or f"HTTP {res.status_code}",
                            status=res.status_code,
                            detail=res.text or None,
                        )
                else:
                    problem = Problem(
                        type="http_error",
                        title=res.reason_phrase or f"HTTP {res.status_code}",
                        status=res.status_code,
                        detail=res.text or None,
                    )

                if not problem.request_id:
                    problem.request_id = res.headers.get("x-request-id")

                raise MonolithError(problem)

            if is_json:
                return res.json()

            return res.text

        raise MonolithError(
            Problem(
                type="request_failed",
                title="Request failed after max attempts",
                status=0,
            )
        )

    def close(self) -> None:
        if self._owns_client:
            self._client.close()

    def __enter__(self) -> HttpClient:
        return self

    def __exit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        self.close()
