from __future__ import annotations

import enum
import threading
import urllib.parse
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any, Callable

from monolith.errors import MonolithError, Problem
from monolith.http import HttpClient
from monolith.session import MemorySessionStore, SessionStore

# Legacy listener: receives the access token (or None).
AuthChangeListener = Callable[[str | None], None]

# Refresh this long before the access token expires.
REFRESH_SKEW_SECONDS = 30.0


class AuthStatus(str, enum.Enum):
    UNKNOWN = "unknown"
    SIGNED_OUT = "signed_out"
    SIGNED_IN = "signed_in"


@dataclass(frozen=True)
class AuthState:
    status: AuthStatus
    user: dict[str, Any] | None = None

    @property
    def is_signed_in(self) -> bool:
        return self.status is AuthStatus.SIGNED_IN


AuthStateListener = Callable[[AuthState], None]


@dataclass
class AuthSession:
    token: str
    user: dict[str, Any] | None = None
    raw: dict[str, Any] = field(default_factory=dict)
    refresh_token: str | None = None
    expires_at: float | None = None  # unix seconds

    def __getitem__(self, item: str) -> Any:
        if hasattr(self, item):
            return getattr(self, item)
        return self.raw[item]

    def get(self, item: str, default: Any = None) -> Any:
        if hasattr(self, item):
            val = getattr(self, item)
            return val if val is not None else default
        return self.raw.get(item, default)


def _parse_time(value: Any) -> float | None:
    if not isinstance(value, str) or not value:
        return None
    try:
        dt = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return dt.timestamp()


def _now() -> float:
    return datetime.now(timezone.utc).timestamp()


def _session_from_response(res: Any) -> AuthSession:
    raw = res if isinstance(res, dict) else {}
    return AuthSession(
        token=raw.get("token") or "",
        user=raw.get("user"),
        raw=raw,
        refresh_token=raw.get("refreshToken") or None,
        expires_at=_parse_time(raw.get("expiresAt")),
    )


class AuthService:
    def __init__(
        self,
        http: HttpClient,
        collection: str = "users",
        session_store: SessionStore | None = None,
    ) -> None:
        self._http = http
        self.auth_collection = collection
        self._listeners: set[AuthChangeListener] = set()
        self._state_listeners: set[AuthStateListener] = set()
        self._is_system = collection in ("_superusers", "system")
        self._store: SessionStore = session_store or MemorySessionStore()
        self._lock = threading.Lock()  # single-flight refresh
        self._session: AuthSession | None = None
        self._state = AuthState(AuthStatus.UNKNOWN)
        if not self._is_system:
            http.auth_hook = self
            self._restore()
        else:
            self._state = AuthState(AuthStatus.SIGNED_OUT)

    # ---- paths ------------------------------------------------------------

    def _path(self, tail: str) -> str:
        return f"/api/v1/apps/{urllib.parse.quote(self._http.app, safe='')}/auth/{tail}"

    # ---- state ------------------------------------------------------------

    @property
    def token(self) -> str | None:
        return self._http.get_token()

    @property
    def state(self) -> AuthState:
        return self._state

    @property
    def user(self) -> dict[str, Any] | None:
        return self._session.user if self._session else None

    def set_token(self, token: str | None) -> None:
        """Use a bare access token (no refresh). `None` signs out locally."""
        if self._is_system:
            self._http.set_token(token)
            self._notify(token)
        elif token is None:
            self._clear()
        else:
            self._apply(AuthSession(token=token))

    def on_change(self, listener: AuthChangeListener) -> Callable[[], None]:
        """Legacy: listener gets the access token (or None). See `on_state_change` for AuthState."""
        self._listeners.add(listener)
        return lambda: self._listeners.discard(listener)

    def on_state_change(
        self, listener: AuthStateListener, *, emit_current: bool = True
    ) -> Callable[[], None]:
        """Listener gets an AuthState on every change (and the current one at once). Returns unsubscribe."""
        self._state_listeners.add(listener)
        if emit_current:
            self._call(listener, self._state)
        return lambda: self._state_listeners.discard(listener)

    @staticmethod
    def _call(listener: Callable[[Any], None], value: Any) -> None:
        try:
            listener(value)
        except Exception:
            pass

    def _notify(self, token: str | None) -> None:
        for listener in list(self._listeners):
            self._call(listener, token)

    def _emit(self, state: AuthState) -> None:
        self._state = state
        for listener in list(self._state_listeners):
            self._call(listener, state)

    def _restore(self) -> None:
        data = self._store.load()
        if data and data.get("token"):
            self._session = AuthSession(
                token=data["token"],
                user=data.get("user"),
                refresh_token=data.get("refreshToken"),
                expires_at=_parse_time(data.get("expiresAt")),
            )
            self._http.set_token(self._session.token)
            self._state = AuthState(AuthStatus.SIGNED_IN, self._session.user)
        else:
            self._state = AuthState(AuthStatus.SIGNED_OUT)

    def _apply(self, session: AuthSession) -> None:
        """Make `session` the current one: http token, store, listeners."""
        if not session.token:
            return
        self._session = session
        self._http.set_token(session.token)
        data: dict[str, Any] = {"token": session.token, "user": session.user}
        if session.refresh_token:
            data["refreshToken"] = session.refresh_token
        if session.expires_at is not None:
            data["expiresAt"] = datetime.fromtimestamp(session.expires_at, timezone.utc).isoformat()
        self._store.save(data)
        self._notify(session.token)
        self._emit(AuthState(AuthStatus.SIGNED_IN, session.user))

    def _clear(self) -> None:
        self._session = None
        self._http.set_token(None)
        self._store.clear()
        self._notify(None)
        self._emit(AuthState(AuthStatus.SIGNED_OUT))

    # ---- system (superuser) ---------------------------------------------------

    def _login_system(self, email: str, password: str) -> AuthSession:
        session = _session_from_response(
            self._http.request(
                "POST", "/api/v1/system/login", body={"email": email, "password": password}
            )
        )
        if session.token:
            self.set_token(session.token)
        return session

    def install(self, token: str, email: str, password: str) -> AuthSession:
        res = self._http.request(
            "POST",
            "/api/v1/system/install",
            body={"token": token, "email": email, "password": password},
        )
        session = _session_from_response(res)
        if session.token:
            self.set_token(session.token)
        return session

    # ---- app users -------------------------------------------------------------

    def providers(self) -> dict[str, Any]:
        """Public: email/password flags and the enabled OAuth providers."""
        res = self._http.request("GET", self._path("providers"), auth_retry=False)
        return res if isinstance(res, dict) else {}

    def sign_up(self, email: str, password: str, name: str | None = None) -> AuthSession:
        body: dict[str, Any] = {"email": email, "password": password}
        if name is not None:
            body["name"] = name
        return self._sign_in_call("register", body)

    def sign_in(self, email: str, password: str) -> AuthSession:
        if self._is_system:
            return self._login_system(email, password)
        return self._sign_in_call("login", {"email": email, "password": password})

    # Backward compatible name.
    sign_in_with_password = sign_in

    def _sign_in_call(self, tail: str, body: dict[str, Any]) -> AuthSession:
        res = self._http.request("POST", self._path(tail), body=body, auth_retry=False)
        session = _session_from_response(res)
        self._apply(session)
        return session

    def sign_out(self) -> None:
        """Ends the session locally first, then revokes the refresh token on the server."""
        if self._is_system:
            self.set_token(None)
            return
        refresh = self._session.refresh_token if self._session else None
        self._clear()
        if refresh:
            self._http.request(
                "POST", self._path("logout"), body={"refreshToken": refresh}, auth_retry=False
            )

    def refresh(self) -> AuthSession:
        """Forces a refresh (rotation). Concurrent callers share one refresh."""
        with self._lock:
            return self._do_refresh()

    def me(self) -> dict[str, Any]:
        """Fetches the user from the server and updates the stored copy."""
        user = self._http.request("GET", self._path("me"))
        self._set_user(user)
        return user

    def update_profile(self, name: str) -> dict[str, Any]:
        user = self._http.request("PATCH", self._path("me"), body={"name": name})
        self._set_user(user)
        return user

    def change_password(self, old_password: str, new_password: str) -> None:
        """The server ends every session of the user, so this signs out locally; sign in again."""
        self._http.request(
            "POST",
            self._path("password"),
            body={"oldPassword": old_password, "newPassword": new_password},
        )
        self._clear()

    def _set_user(self, user: Any) -> None:
        if isinstance(user, dict) and self._session is not None:
            self._session.user = user
            self._apply(self._session)

    def access_token(self) -> str | None:
        """A token valid right now: refreshes first when it is about to expire."""
        self.ensure_fresh()
        return self._http.get_token()

    # ---- OAuth for servers, scripts and desktop apps ------------------------------

    def provider_sign_in_url(self, provider: str, redirect_uri: str) -> str:
        """URL to open in a browser. Do not forget to put `redirect_uri` in the app's allowed redirects."""
        return self._http.build_url(
            self._path(f"oauth/{urllib.parse.quote(provider, safe='')}/start"),
            {"redirectUri": redirect_uri},
        )

    def handle_redirect(self, url_or_fragment: str) -> AuthSession:
        """Finish a provider sign-in from the redirect URL, `#fragment` or bare `token=...` string."""
        if "#" in url_or_fragment:
            fragment = url_or_fragment.split("#", 1)[1]
        elif url_or_fragment.startswith(("token=", "error=")):
            fragment = url_or_fragment
        else:
            fragment = ""
        params = {k: v[0] for k, v in urllib.parse.parse_qs(fragment).items()}
        if "error" in params:
            code = params["error"]
            raise MonolithError(
                Problem(
                    type=code,
                    title="Provider sign-in failed",
                    status=0,
                    detail=params.get("error_description") or code,
                )
            )
        token = params.get("token")
        if not token:
            raise MonolithError(
                Problem(type="invalid_redirect", title="No session in redirect", status=0)
            )
        # The fragment carries no user: fetch it with the new token before announcing SignedIn.
        user = self._http.request(
            "GET",
            self._path("me"),
            headers={"Authorization": f"Bearer {token}"},
            auth_retry=False,
        )
        session = AuthSession(
            token=token,
            user=user if isinstance(user, dict) else None,
            refresh_token=params.get("refreshToken") or None,
            expires_at=_parse_time(params.get("expiresAt")),
        )
        self._apply(session)
        return session

    # ---- refresh machinery (called by HttpClient) -------------------------------------

    def ensure_fresh(self) -> None:
        s = self._session
        if s is None or not s.refresh_token or s.expires_at is None:
            return
        if s.expires_at - _now() > REFRESH_SKEW_SECONDS:
            return
        with self._lock:
            s = self._session  # another thread may have refreshed while we waited
            if s is None or not s.refresh_token or s.expires_at is None:
                return
            if s.expires_at - _now() > REFRESH_SKEW_SECONDS:
                return
            try:
                self._do_refresh()
            except MonolithError:
                if s.expires_at <= _now():  # token really expired: surface the failure
                    raise

    def refresh_after_unauthorized(self, used_token: str) -> bool:
        """True when the caller should retry once with a new token."""
        with self._lock:
            s = self._session
            if s is None or not s.refresh_token:
                return False
            if s.token != used_token:  # someone refreshed already
                return True
            try:
                self._do_refresh()
            except MonolithError:
                return False
            return True

    def _do_refresh(self) -> AuthSession:
        """Caller holds the lock."""
        s = self._session
        if s is None or not s.refresh_token:
            raise MonolithError(
                Problem(type="no_session", title="No refresh token", status=0)
            )
        try:
            res = self._http.request(
                "POST", self._path("refresh"), body={"refreshToken": s.refresh_token}, auth_retry=False
            )
        except MonolithError as err:
            if err.status in (400, 401):  # token rejected, revoked or already used
                self._clear()
            raise
        new = _session_from_response(res)
        if new.user is None:
            new.user = s.user
        self._apply(new)
        return new
