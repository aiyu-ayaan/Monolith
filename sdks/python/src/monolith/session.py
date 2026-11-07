from __future__ import annotations

import json
import os
import tempfile
import threading
from typing import Any, Protocol


class SessionStore(Protocol):
    """Where the signed-in session lives between runs. Values are plain JSON-able dicts."""

    def load(self) -> dict[str, Any] | None: ...

    def save(self, session: dict[str, Any]) -> None: ...

    def clear(self) -> None: ...


class MemorySessionStore:
    """Default. The session lives as long as the process."""

    def __init__(self) -> None:
        self._value: dict[str, Any] | None = None

    def load(self) -> dict[str, Any] | None:
        return dict(self._value) if self._value else None

    def save(self, session: dict[str, Any]) -> None:
        self._value = dict(session)

    def clear(self) -> None:
        self._value = None


class FileSessionStore:
    """JSON file for scripts and CLIs. Written atomically with mode 0600 (it holds a refresh token)."""

    def __init__(self, path: str | os.PathLike[str]) -> None:
        self.path = os.fspath(path)
        self._lock = threading.Lock()

    def load(self) -> dict[str, Any] | None:
        with self._lock:
            try:
                with open(self.path, encoding="utf-8") as f:
                    data = json.load(f)
            except (OSError, ValueError):
                return None
        return data if isinstance(data, dict) and data.get("token") else None

    def save(self, session: dict[str, Any]) -> None:
        with self._lock:
            directory = os.path.dirname(os.path.abspath(self.path))
            os.makedirs(directory, exist_ok=True)
            fd, tmp = tempfile.mkstemp(dir=directory, prefix=".monolith-session-")
            try:
                with os.fdopen(fd, "w", encoding="utf-8") as f:  # mkstemp creates it 0600
                    json.dump(session, f)
                os.replace(tmp, self.path)
            except BaseException:
                if os.path.exists(tmp):
                    os.unlink(tmp)
                raise

    def clear(self) -> None:
        with self._lock:
            try:
                os.unlink(self.path)
            except FileNotFoundError:
                pass
