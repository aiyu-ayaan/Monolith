package dev.monolith.client.auth

import dev.monolith.client.http.HttpClient
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

/**
 * Authentication session payload returned by login and install endpoints.
 */
@Serializable
data class AuthSession(
    val token: String,
    val user: JsonObject? = null
)

@Serializable
private data class LoginPayload(
    val email: String,
    val password: String
)

@Serializable
private data class InstallPayload(
    val token: String,
    val email: String,
    val password: String
)

typealias AuthChangeListener = (String?) -> Unit

/**
 * Manages authentication credentials, sign in, installation, and token observers.
 */
class AuthService(
    private val http: HttpClient,
    val collection: String = "users"
) {
    private val listeners = mutableSetOf<AuthChangeListener>()
    private val _tokenFlow = MutableStateFlow<String?>(http.getToken())
    val tokenFlow: StateFlow<String?> = _tokenFlow.asStateFlow()

    /**
     * Current bearer token, or null if unauthenticated.
     */
    val token: String?
        get() = http.getToken()

    /**
     * Manually sets or clears the current authentication token and notifies observers.
     */
    fun setToken(token: String?) {
        http.setToken(token)
        _tokenFlow.value = token
        notifyListeners(token)
    }

    /**
     * Registers a listener callback invoked whenever the auth token changes.
     * Returns an unsubscribe function.
     */
    fun onChange(listener: AuthChangeListener): () -> Unit {
        listeners.add(listener)
        return {
            listeners.remove(listener)
        }
    }

    private fun notifyListeners(token: String?) {
        val copy = listeners.toList()
        for (listener in copy) {
            try {
                listener(token)
            } catch (_: Throwable) {
                // Ignore listener exceptions
            }
        }
    }

    /**
     * Authenticates with email and password.
     */
    suspend fun signInWithPassword(email: String, password: String): AuthSession {
        val isSystem = collection == "_superusers" || collection == "system"
        val path = if (isSystem) {
            "/api/v1/system/auth/login"
        } else {
            "/api/v1/apps/${http.app}/auth/login"
        }

        val session: AuthSession = http.request(
            method = io.ktor.http.HttpMethod.Post,
            path = path,
            body = LoginPayload(email, password),
            serializer = AuthSession.serializer(),
            bodySerializer = LoginPayload.serializer()
        )

        if (session.token.isNotBlank()) {
            setToken(session.token)
        }
        return session
    }

    /**
     * Performs initial system installation with the setup token and root superuser credentials.
     */
    suspend fun install(token: String, email: String, password: String): AuthSession {
        val session: AuthSession = http.request(
            method = io.ktor.http.HttpMethod.Post,
            path = "/api/v1/system/install",
            body = InstallPayload(token, email, password),
            serializer = AuthSession.serializer(),
            bodySerializer = InstallPayload.serializer()
        )

        if (session.token.isNotBlank()) {
            setToken(session.token)
        }
        return session
    }

    /**
     * Clears authentication credentials.
     */
    fun signOut() {
        setToken(null)
    }
}
