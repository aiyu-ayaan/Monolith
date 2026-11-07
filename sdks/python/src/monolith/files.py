from __future__ import annotations

import urllib.parse
from typing import Any, BinaryIO

from monolith.http import HttpClient, StorageListOptions, StorageListResult, StorageObject


class FilesService:
    def __init__(self, http: HttpClient) -> None:
        self._http = http

    def bucket_path(self, bucket: str) -> str:
        app_encoded = urllib.parse.quote(self._http.app, safe="")
        bucket_encoded = urllib.parse.quote(bucket, safe="")
        return f"/api/v1/apps/{app_encoded}/storage/buckets/{bucket_encoded}/objects"

    def get_url(
        self,
        bucket: str,
        key: str,
        options: dict[str, Any] | None = None,
        *,
        thumb: str | None = None,
    ) -> str:
        key_encoded = urllib.parse.quote(key, safe="")
        path = f"{self.bucket_path(bucket)}/{key_encoded}"
        thumb_val = thumb or (options.get("thumb") if options else None)
        query = {"thumb": thumb_val} if thumb_val else None
        return self._http.build_url(path, query)

    # TypeScript compatibility alias
    getURL = get_url

    def upload(
        self,
        bucket: str,
        key: str,
        data: bytes | str | BinaryIO,
        options: dict[str, Any] | None = None,
        *,
        content_type: str | None = None,
    ) -> StorageObject:
        key_encoded = urllib.parse.quote(key, safe="")
        path = f"{self.bucket_path(bucket)}/{key_encoded}"

        ct = content_type or (options.get("contentType") if options else None)
        headers = {"Content-Type": ct} if ct else None

        if hasattr(data, "read"):
            data_bytes = data.read()
            if isinstance(data_bytes, str):
                data_bytes = data_bytes.encode("utf-8")
        elif isinstance(data, str):
            data_bytes = data.encode("utf-8")
        else:
            data_bytes = bytes(data)

        res = self._http.request("PUT", path, content=data_bytes, headers=headers)

        if isinstance(res, dict):
            return StorageObject(
                bucket=res.get("bucket", bucket),
                key=res.get("key", key),
                size=res.get("size", len(data_bytes)),
                content_type=res.get("contentType", ct or ""),
                blake3=res.get("blake3", ""),
                created=res.get("created", ""),
                updated=res.get("updated", ""),
                raw=res,
            )
        return StorageObject(
            bucket=bucket,
            key=key,
            size=len(data_bytes),
            content_type=ct or "",
            raw={},
        )

    def delete(self, bucket: str, key: str) -> bool:
        key_encoded = urllib.parse.quote(key, safe="")
        path = f"{self.bucket_path(bucket)}/{key_encoded}"
        self._http.request("DELETE", path)
        return True

    def list(
        self,
        bucket: str,
        options: StorageListOptions | dict[str, Any] | None = None,
        *,
        prefix: str | None = None,
        cursor: str | None = None,
        limit: int | None = None,
    ) -> StorageListResult:
        query: dict[str, Any] = {}
        if isinstance(options, StorageListOptions):
            if options.prefix is not None:
                query["prefix"] = options.prefix
            if options.cursor is not None:
                query["cursor"] = options.cursor
            if options.limit is not None:
                query["limit"] = options.limit
        elif isinstance(options, dict):
            for k, v in options.items():
                if v is not None:
                    query[k] = v

        if prefix is not None:
            query["prefix"] = prefix
        if cursor is not None:
            query["cursor"] = cursor
        if limit is not None:
            query["limit"] = limit

        res = self._http.request("GET", self.bucket_path(bucket), query=query if query else None)

        items: list[StorageObject] = []
        next_cursor = None
        raw_dict = {}

        if isinstance(res, dict):
            raw_dict = res
            next_cursor = res.get("nextCursor") or res.get("next_cursor")
            for item in res.get("items", []):
                if isinstance(item, dict):
                    items.append(
                        StorageObject(
                            bucket=item.get("bucket", bucket),
                            key=item.get("key", ""),
                            size=item.get("size", 0),
                            content_type=item.get("contentType", ""),
                            blake3=item.get("blake3", ""),
                            created=item.get("created", ""),
                            updated=item.get("updated", ""),
                            raw=item,
                        )
                    )

        return StorageListResult(
            items=items,
            next_cursor=next_cursor,
            raw=raw_dict,
        )
