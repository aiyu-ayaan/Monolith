from __future__ import annotations

import threading
import urllib.parse
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any

from monolith.http import HttpClient


@dataclass
class AnalyticsEvent:
    event: str
    properties: dict[str, Any] | None = None
    timestamp: str = ""


class AnalyticsService:
    def __init__(
        self,
        http: HttpClient,
        flush_interval_sec: float = 5.0,
        max_batch_size: int = 20,
    ) -> None:
        self._http = http
        self.flush_interval_sec = flush_interval_sec
        self.max_batch_size = max_batch_size
        self._queue: list[dict[str, Any]] = []
        self._lock = threading.Lock()
        self._timer: threading.Timer | None = None
        self._is_flushing = False

    def track(self, event: str, properties: dict[str, Any] | None = None) -> None:
        payload = {
            "event": event,
            "properties": properties if properties is not None else {},
            "timestamp": datetime.now(timezone.utc).isoformat(),
        }

        should_flush = False
        with self._lock:
            self._queue.append(payload)
            if len(self._queue) >= self.max_batch_size:
                should_flush = True
            elif self._timer is None and self.flush_interval_sec > 0:
                self._timer = threading.Timer(self.flush_interval_sec, self._on_timer)
                self._timer.daemon = True
                self._timer.start()

        if should_flush:
            self.flush()

    def _on_timer(self) -> None:
        with self._lock:
            self._timer = None
        self.flush()

    def flush(self) -> None:
        with self._lock:
            if self._is_flushing or not self._queue:
                return
            self._is_flushing = True
            if self._timer is not None:
                self._timer.cancel()
                self._timer = None
            batch = self._queue[: self.max_batch_size]
            del self._queue[: len(batch)]

        app_encoded = urllib.parse.quote(self._http.app, safe="")
        path = f"/api/v1/apps/{app_encoded}/analytics/events"

        try:
            self._http.request("POST", path, body={"events": batch})
        except Exception:
            # Re-queue on failure if within bounds
            with self._lock:
                if len(self._queue) < 500:
                    self._queue = batch + self._queue
        finally:
            with self._lock:
                self._is_flushing = False
                if self._queue and self._timer is None and self.flush_interval_sec > 0:
                    self._timer = threading.Timer(self.flush_interval_sec, self._on_timer)
                    self._timer.daemon = True
                    self._timer.start()

    def close(self) -> None:
        with self._lock:
            if self._timer is not None:
                self._timer.cancel()
                self._timer = None
        self.flush()
