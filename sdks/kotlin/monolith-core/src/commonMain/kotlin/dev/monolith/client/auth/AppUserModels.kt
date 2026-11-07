package dev.monolith.client.auth

import kotlinx.serialization.Serializable

/** An app user (end user of a Monolith app), as returned by `auth/me` and inside sessions. */
@Serializable
data class AppUser(
    val id: String,
    val email: String? = null,
    val name: String = "",
    val created: String = "",
    val disabled: Boolean = false,
    val lastLogin: String? = null,
    val providers: List<String> = emptyList()
) {
    /** Name, else email, else id: something to show in a chip. */
    val displayName: String get() = name.ifBlank { email ?: id }
}

/** Session returned by register, login and refresh. [expiresAt] is the access token expiry (ISO 8601). */
@Serializable
data class UserSession(
    val token: String,
    val refreshToken: String,
    val expiresAt: String,
    val user: AppUser
)

/** One sign-in provider the server offers. [startPath] is relative to the server. */
@Serializable
data class AuthProviderInfo(
    val id: String,
    val name: String,
    val startPath: String = ""
)

@Serializable
data class EmailPasswordInfo(
    val enabled: Boolean = true,
    val allowSignup: Boolean = true,
    val minPasswordLength: Int = 8
)

/** Result of `GET auth/providers`: what a sign-in screen can offer. */
@Serializable
data class PublicAuth(
    val app: String = "",
    val emailPassword: EmailPasswordInfo = EmailPasswordInfo(),
    val providers: List<AuthProviderInfo> = emptyList()
)

/** Auth state of the current app user. */
sealed interface AuthState {
    /** The stored session is still being restored. */
    data object Unknown : AuthState

    data object SignedOut : AuthState

    data class SignedIn(val user: AppUser) : AuthState
}

/** Opens the provider sign-in page (Custom Tab, system browser, ...). Keeps core platform neutral. */
fun interface BrowserLauncher {
    fun launch(url: String)
}

/**
 * Persists the session as an opaque string. Implementations must be fast and non-suspending;
 * encrypt at rest where the platform allows it.
 */
interface SessionStorage {
    fun load(): String?
    fun save(value: String?)
}

/** Keeps the session for the life of the process. */
class MemorySessionStorage(private var value: String? = null) : SessionStorage {
    override fun load(): String? = value
    override fun save(value: String?) {
        this.value = value
    }
}

/** Outcome of [AppAuth.handleRedirect]. */
sealed interface RedirectResult {
    /** The URI is not this app's sign-in redirect. */
    data object NotHandled : RedirectResult

    data class Success(val user: AppUser) : RedirectResult

    data class Failure(val code: String, val description: String?) : RedirectResult
}

/** Session behaviour knobs for [dev.monolith.client.MonolithClient]. All optional. */
data class SessionOptions(
    /** Where the session is persisted; null keeps it in memory. */
    val storage: SessionStorage? = null,
    val browserLauncher: BrowserLauncher? = null,
    /** Redirect URI sent to `oauth/{provider}/start`; must be in the app's allowed redirect URLs. */
    val redirectUri: String? = null,
    /** Scope for background work (restore, redirect handling). Default: a private supervisor scope. */
    val scope: kotlinx.coroutines.CoroutineScope? = null,
    /** Epoch milliseconds; replace in tests. */
    val clock: () -> Long = { kotlinx.datetime.Clock.System.now().toEpochMilliseconds() },
    /** Refresh the access token when it expires within this many milliseconds. */
    val refreshLeewayMs: Long = 30_000
)
