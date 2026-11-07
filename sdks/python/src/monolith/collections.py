from __future__ import annotations

import urllib.parse
from typing import Any, Generic, TypeVar

from monolith.http import CountOptions, HttpClient, ListOptions, ListResult

T = TypeVar("T")


class Collection(Generic[T]):
    def __init__(self, name: str, http: HttpClient) -> None:
        self.name = name
        self._http = http

    @property
    def base_path(self) -> str:
        app_encoded = urllib.parse.quote(self._http.app, safe="")
        name_encoded = urllib.parse.quote(self.name, safe="")
        return f"/api/v1/apps/{app_encoded}/collections/{name_encoded}/records"

    def list(
        self,
        options: ListOptions | dict[str, Any] | None = None,
        *,
        page: int | None = None,
        per_page: int | None = None,
        cursor: str | None = None,
        limit: int | None = None,
        filter: str | None = None,
        sort: str | None = None,
        expand: str | None = None,
        fields: str | None = None,
        total: str | None = None,
        **kwargs: Any,
    ) -> ListResult:
        query: dict[str, Any] = {}

        if isinstance(options, ListOptions):
            if options.page is not None:
                query["page"] = options.page
            if options.per_page is not None:
                query["perPage"] = options.per_page
            if options.cursor is not None:
                query["cursor"] = options.cursor
            if options.limit is not None:
                query["limit"] = options.limit
            if options.filter is not None:
                query["filter"] = options.filter
            if options.sort is not None:
                query["sort"] = options.sort
            if options.expand is not None:
                query["expand"] = options.expand
            if options.fields is not None:
                query["fields"] = options.fields
            if options.total is not None:
                query["total"] = options.total
        elif isinstance(options, dict):
            for k, v in options.items():
                if v is not None:
                    if k == "per_page":
                        query["perPage"] = v
                    else:
                        query[k] = v

        if page is not None:
            query["page"] = page
        if per_page is not None:
            query["perPage"] = per_page
        if cursor is not None:
            query["cursor"] = cursor
        if limit is not None:
            query["limit"] = limit
        if filter is not None:
            query["filter"] = filter
        if sort is not None:
            query["sort"] = sort
        if expand is not None:
            query["expand"] = expand
        if fields is not None:
            query["fields"] = fields
        if total is not None:
            query["total"] = total

        for k, v in kwargs.items():
            if v is not None:
                if k == "perPage" or k == "per_page":
                    query["perPage"] = v
                else:
                    query[k] = v

        res = self._http.request("GET", self.base_path, query=query)
        if isinstance(res, dict):
            items = res.get("items", [])
            total_count = res.get("total")
            cur = res.get("cursor")
            next_cur = res.get("nextCursor") or res.get("next_cursor")
            prev_cur = res.get("prevCursor") or res.get("prev_cursor")
            return ListResult(
                items=items,
                total=total_count,
                cursor=cur,
                next_cursor=next_cur,
                prev_cursor=prev_cur,
                raw=res,
            )
        return ListResult(items=res if isinstance(res, list) else [], raw={})

    def get_one(
        self,
        id: str,
        options: dict[str, Any] | None = None,
        *,
        expand: str | None = None,
        fields: str | None = None,
    ) -> Any:
        query: dict[str, Any] = {}
        if options:
            if "expand" in options and options["expand"] is not None:
                query["expand"] = options["expand"]
            if "fields" in options and options["fields"] is not None:
                query["fields"] = options["fields"]
        if expand is not None:
            query["expand"] = expand
        if fields is not None:
            query["fields"] = fields

        id_encoded = urllib.parse.quote(id, safe="")
        path = f"{self.base_path}/{id_encoded}"
        return self._http.request("GET", path, query=query if query else None)

    # Alias for get_one
    get = get_one

    def create(self, data: dict[str, Any]) -> Any:
        return self._http.request("POST", self.base_path, body=data)

    def update(self, id: str, data: dict[str, Any]) -> Any:
        id_encoded = urllib.parse.quote(id, safe="")
        path = f"{self.base_path}/{id_encoded}"
        return self._http.request("PATCH", path, body=data)

    def replace(self, id: str, data: dict[str, Any]) -> Any:
        id_encoded = urllib.parse.quote(id, safe="")
        path = f"{self.base_path}/{id_encoded}"
        return self._http.request("PUT", path, body=data)

    def delete(self, id: str) -> bool:
        id_encoded = urllib.parse.quote(id, safe="")
        path = f"{self.base_path}/{id_encoded}"
        self._http.request("DELETE", path)
        return True

    def count(
        self,
        options: CountOptions | dict[str, Any] | None = None,
        *,
        filter: str | None = None,
    ) -> int:
        query: dict[str, Any] = {}
        if isinstance(options, CountOptions):
            if options.filter is not None:
                query["filter"] = options.filter
        elif isinstance(options, dict):
            if "filter" in options and options["filter"] is not None:
                query["filter"] = options["filter"]

        if filter is not None:
            query["filter"] = filter

        path = f"{self.base_path}:count"
        res = self._http.request("GET", path, query=query if query else None)
        if isinstance(res, dict):
            return int(res.get("count", 0))
        return 0
