package dev.monolith.client.auth

import dev.monolith.client.http.AuthHooks
import dev.monolith.client.http.HttpClient
import dev.monolith.client.http.MonolithException
import io.ktor.http.HttpMethod
import io.ktor.http.encodeURLPath
import io.ktor.http.parseQueryString
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.datetime.Instant
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/**
 * Auth for the end users of one app: email and password, provider sign-in, session persistence and
 * automatic token refresh. Reach it as `mono.auth`.
 *
 * The superuser/system login stays on [AuthService] (`mono.auth("_superusers")`).
 */
class AppAuth internal constructor(
    private val http: HttpClient,
    options: SessionOptions
) : AuthHooks {
    private val storage: SessionStorage? = options.storage
    private val launcher: BrowserLauncher? = options.browserLauncher
    private val clock: () -> Long = options.clock
    private val leewayMs: Long = options.refreshLeewayMs
    private val scope: CoroutineScope = options.scope ?: CoroutineScope(SupervisorJob() + Dispatchers.Default)

    /** Redirect URI sent to the provider start endpoint, e.g. `monolith-shop://auth`. */
    var redirectUri: String? = options.redirectUri

    @kotlin.concurrent.Volatile
    private var session: UserSession? = null

    @kotlin.concurrent.Volatile
    private var expiresAtMs: Long = 0

    private val refreshMutex = Mutex()
    private val _state = MutableStateFlow<AuthState>(if (storage != null) AuthState.Unknown else AuthState.SignedOut)
    private val _redirecting = MutableStateFlow(false)
    private val _redirectFailures = MutableSharedFlow<RedirectResult.Failure>(extraBufferCapacity = 8)
    private var restoreJob: Job? = null

    private var providersCache: PublicAuth? = null
    private var providersAt: Long = 0

    /** Unknown while the stored session is restored, then SignedOut or SignedIn. */
    val state: StateFlow<AuthState> = _state.asStateFlow()

    /** True while a provider redirect is being completed. */
    val redirecting: StateFlow<Boolean> = _redirecting.asStateFlow()

    /** Provider sign-ins that came back with `#error=`, or whose completion failed. */
    val redirectFailures: SharedFlow<RedirectResult.Failure> = _redirectFailures.asSharedFlow()

    /** The signed-in user, or null. */
    val user: AppUser? get() = (state.value as? AuthState.SignedIn)?.user

    /** True when a [BrowserLauncher] is installed, so [signInWithProvider] opens the page itself. */
    val canLaunchBrowser: Boolean get() = launcher != null

    init {
        http.authHooks = this
        if (storage != null) {
            restoreJob = scope.launch { restore() }
        }
    }

    // --- sign in / up / out ------------------------------------------------------------------

    /** What the server offers: email flags and enabled providers. Cached for 60 s unless [forceRefresh]. */
    suspend fun providers(forceRefresh: Boolean = false): PublicAuth {
        val cached = providersCache
        if (!forceRefresh && cached != null && clock() - providersAt < PROVIDERS_TTL_MS) return cached
        val fresh: PublicAuth = http.request(
            method = HttpMethod.Get,
            path = path("auth/providers"),
            serializer = PublicAuth.serializer(),
            anonymous = true
        )
        providersCache = fresh
        providersAt = clock()
        return fresh
    }

    suspend fun signUp(email: String, password: String, name: String? = null): AppUser {
        awaitRestore()
        val body = buildJsonObject {
            put("email", email)
            put("password", password)
            if (!name.isNullOrBlank()) put("name", name)
        }
        return applyNew(post("auth/register", body))
    }

    suspend fun signIn(email: String, password: String): AppUser {
        awaitRestore()
        val body = buildJsonObject {
            put("email", email)
            put("password", password)
        }
        return applyNew(post("auth/login", body))
    }

    /**
     * Builds the provider start URL and, when a [BrowserLauncher] is installed, opens it.
     * The sign-in finishes in [handleRedirect]. Returns the URL either way.
     */
    fun signInWithProvider(id: String, redirectUri: String? = null): String {
        val target = redirectUri ?: this.redirectUri
            ?: throw IllegalStateException("No redirectUri set. Pass one, or set it in SessionOptions.redirectUri.")
        val url = http.buildUrl("/api/v1/apps/${http.app}/auth/oauth/${id.encodeURLPath()}/start", mapOf("redirectUri" to target))
        launcher?.launch(url)
        return url
    }

    /**
     * Completes a provider sign-in from the redirect [uri]
     * (`scheme://auth#token=...&refreshToken=...&expiresAt=...` or `#error=...`).
     */
    suspend fun handleRedirect(uri: String): RedirectResult {
        val expected = redirectUri
        if (expected != null && !uri.substringBefore('#').trimEnd('/').equals(expected.trimEnd('/'), ignoreCase = true)) {
            return RedirectResult.NotHandled
        }
        val fragment = uri.substringAfter('#', "")
        if (fragment.isEmpty()) return RedirectResult.NotHandled
        val params = parseQueryString(fragment)
        params["error"]?.let { code ->
            val failure = RedirectResult.Failure(code, params["error_description"])
            _redirectFailures.tryEmit(failure)
            return failure
        }
        val token = params["token"] ?: return RedirectResult.NotHandled
        val refresh = params["refreshToken"] ?: return RedirectResult.NotHandled
        _redirecting.value = true
        try {
            awaitRestore()
            val user: AppUser = http.request(
                method = HttpMethod.Get,
                path = path("auth/me"),
                headers = mapOf("Authorization" to "Bearer $token"),
                serializer = AppUser.serializer(),
                anonymous = true
            )
            applySession(UserSession(token, refresh, params["expiresAt"] ?: "", user))
            return RedirectResult.Success(user)
        } catch (e: MonolithException) {
            val failure = RedirectResult.Failure(if (e.status == 0) "network" else "session_failed", e.message)
            _redirectFailures.tryEmit(failure)
            return failure
        } finally {
            _redirecting.value = false
        }
    }

    /** Fire-and-forget [handleRedirect] for platform callbacks (Android activities). */
    fun handleRedirectAsync(uri: String): Job {
        _redirecting.value = true
        return scope.launch {
            try {
                handleRedirect(uri)
            } finally {
                _redirecting.value = false
            }
        }
    }

    /** Clears the local session at once, then tells the server to end it (best effort). */
    suspend fun signOut() {
        awaitRestore()
        val s = session
        clearSession()
        if (s != null) {
            try {
                http.request<Unit>(
                    method = HttpMethod.Post,
                    path = path("auth/logout"),
                    body = buildJsonObject { put("refreshToken", s.refreshToken) },
                    anonymous = true
                )
            } catch (_: MonolithException) {
                // The local session is already gone; an unreachable server must not block sign-out.
            }
        }
    }

    suspend fun updateProfile(name: String): AppUser {
        val updated: AppUser = http.request(
            method = HttpMethod.Patch,
            path = path("auth/me"),
            body = buildJsonObject { put("name", name) },
            serializer = AppUser.serializer()
        )
        session?.let { applySession(it.copy(user = updated)) }
        return updated
    }

    /**
     * Changes the password. The server ends every session of the user, so this signs in again with the
     * new password (or signs out locally if that is impossible).
     */
    suspend fun changePassword(oldPassword: String, newPassword: String) {
        http.request<Unit>(
            method = HttpMethod.Post,
            path = path("auth/password"),
            body = buildJsonObject {
                put("oldPassword", oldPassword)
                put("newPassword", newPassword)
            }
        )
        val email = user?.email
        if (email == null) {
            clearSession()
            return
        }
        try {
            signIn(email, newPassword)
        } catch (_: MonolithException) {
            clearSession()
        }
    }

    /** A valid access token (refreshed if it is about to expire), or null when signed out. */
    suspend fun accessToken(): String? {
        beforeRequest()
        return session?.token ?: http.getToken()
    }

    /** Forces a refresh now. Single-flight; true when a valid session exists afterwards. */
    suspend fun refresh(): Boolean {
        awaitRestore()
        return refreshLocked { true }
    }

    // --- hooks wired into the HTTP client ----------------------------------------------------

    override suspend fun beforeRequest() {
        awaitRestore()
        val s = session ?: return
        if (http.getToken() != s.token || isFresh(s)) return
        refreshLocked { !isFresh(it) }
    }

    override suspend fun onUnauthorized(usedToken: String): Boolean {
        awaitRestore()
        val s = session ?: return false
        if (http.getToken() != s.token) return false
        if (s.token != usedToken) return true // somebody refreshed while this request was in flight
        return refreshLocked { it.token == usedToken }
    }

    // --- internals ---------------------------------------------------------------------------

    private fun isFresh(s: UserSession) = expiresAtMs - clock() > leewayMs

    /** Single-flight: concurrent callers queue on the mutex and re-check [needed] against the new session. */
    private suspend fun refreshLocked(needed: (UserSession) -> Boolean): Boolean = refreshMutex.withLock {
        val s = session ?: return@withLock false
        if (!needed(s)) return@withLock true
        try {
            val fresh: UserSession = post("auth/refresh", buildJsonObject { put("refreshToken", s.refreshToken) })
            applySession(fresh)
            true
        } catch (e: MonolithException) {
            // A dead refresh token ends the session; a flaky network keeps it for the next try.
            if (e.status == 400 || e.status == 401 || e.status == 403) clearSession()
            false
        }
    }

    private suspend fun awaitRestore() {
        restoreJob?.join()
    }

    private suspend fun restore() {
        val stored = try {
            storage?.load()?.let { http.json.decodeFromString(UserSession.serializer(), it) }
        } catch (_: Exception) {
            null
        }
        if (stored == null || session != null) {
            if (session == null) _state.value = AuthState.SignedOut
            return
        }
        applySession(stored, persist = false)
        if (!isFresh(stored)) refreshLocked { !isFresh(it) }
    }

    private suspend fun post(sub: String, body: JsonObject): UserSession = http.request(
        method = HttpMethod.Post,
        path = path(sub),
        body = body,
        serializer = UserSession.serializer(),
        anonymous = true
    )

    private fun applyNew(s: UserSession): AppUser {
        applySession(s)
        return s.user
    }

    private fun applySession(s: UserSession, persist: Boolean = true) {
        session = s
        expiresAtMs = parseExpiry(s.expiresAt)
        http.setToken(s.token)
        if (persist) {
            try {
                storage?.save(http.json.encodeToString(UserSession.serializer(), s))
            } catch (_: Exception) {
                // A broken keystore must not break sign-in; the session lives on in memory.
            }
        }
        _state.value = AuthState.SignedIn(s.user)
    }

    private fun clearSession() {
        val s = session
        session = null
        expiresAtMs = 0
        if (s != null && http.getToken() == s.token) http.setToken(null)
        try {
            storage?.save(null)
        } catch (_: Exception) {
        }
        _state.value = AuthState.SignedOut
    }

    private fun parseExpiry(iso: String): Long =
        try {
            Instant.parse(iso).toEpochMilliseconds()
        } catch (_: Exception) {
            clock() + leewayMs * 2 // unknown expiry: refresh soon
        }

    private fun path(sub: String) = "/api/v1/apps/${http.app}/$sub"

    private companion object {
        const val PROVIDERS_TTL_MS = 60_000L
    }
}
