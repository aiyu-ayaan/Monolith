package dev.monolith.client.auth

import dev.monolith.client.MonolithClient
import dev.monolith.client.http.MonolithException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** Whether the email form follows the server (`Auto`), is never shown (`Hide`) or always shown (`Force`). */
enum class EmailMode { Auto, Hide, Force }

/** `Auto` follows the server, `Hide` never offers sign-up, `Only` starts on the sign-up form. */
enum class SignUpMode { Auto, Hide, Only }

enum class AuthMode { SignIn, SignUp }

/** What the controller shows and offers. UI kits add looks on top of this. */
data class AuthOptions(
    val email: EmailMode = EmailMode.Auto,
    val signUp: SignUpMode = SignUpMode.Auto,
    /** Null: every provider the server enables, in server order. Otherwise an allow list in this order. */
    val providers: List<String>? = null,
    /** Provider ids that are never shown. */
    val hideProviders: Set<String> = emptySet()
)

enum class AuthErrorCode(val id: String) {
    InvalidCredentials("invalid_credentials"),
    EmailTaken("email_taken"),
    WeakPassword("weak_password"),
    InvalidEmail("invalid_email"),
    SignupClosed("signup_closed"),
    Network("network"),
    RedirectNotAllowed("redirect_not_allowed"),
    ProviderFailed("provider_failed"),
    Unknown("unknown")
}

data class AuthUiError(val code: AuthErrorCode, val message: String)

/** Everything a sign-in screen needs to draw itself. */
data class AuthUiState(
    /** Fetching providers or restoring the session. */
    val loading: Boolean = true,
    /** Raw server answer; null while loading or when it failed. */
    val methods: PublicAuth? = null,
    val mode: AuthMode = AuthMode.SignIn,
    /** A request is running (email) or a provider page is open. */
    val submitting: Boolean = false,
    /** The provider being used while [submitting], else null. */
    val activeProvider: String? = null,
    val error: AuthUiError? = null,
    /** Set when signed in. */
    val user: AppUser? = null,
    /** The email form is shown (options applied to the server answer). */
    val showEmailForm: Boolean = false,
    /** The sign-in/sign-up switch is offered. */
    val canSignUp: Boolean = false,
    /** Providers to show, filtered and ordered by the options. */
    val providers: List<AuthProviderInfo> = emptyList(),
    val minPasswordLength: Int = 8
)

/** The actions of an [AuthController]; also what headless UIs receive. */
interface AuthActions {
    fun setMode(mode: AuthMode)
    fun signInWithEmail(email: String, password: String)
    fun signUpWithEmail(email: String, password: String, name: String = "")
    fun signInWithProvider(id: String)
    fun dismissError()
    fun signOut()
}

enum class AuthContext { SignIn, SignUp, Provider, Other }

/** Maps a server problem to the error the UI shows. */
fun MonolithException.toAuthUiError(context: AuthContext = AuthContext.Other): AuthUiError {
    val text = detail ?: problem.title
    val passwordField = errors.firstOrNull { it.field.equals("password", true) || it.field.equals("newPassword", true) }
    val emailField = errors.firstOrNull { it.field.equals("email", true) }
    return when {
        status == 0 -> AuthUiError(AuthErrorCode.Network, "Can't reach the server. Check your connection and try again.")
        status == 401 -> AuthUiError(AuthErrorCode.InvalidCredentials, "Wrong email or password.")
        status == 409 -> AuthUiError(AuthErrorCode.EmailTaken, "An account with this email already exists.")
        status == 403 && context == AuthContext.SignUp ->
            AuthUiError(AuthErrorCode.SignupClosed, "Sign-up is closed for this app.")
        (status == 400 || status == 422) && passwordField != null ->
            AuthUiError(AuthErrorCode.WeakPassword, passwordField.message)
        (status == 400 || status == 422) && emailField != null ->
            AuthUiError(AuthErrorCode.InvalidEmail, emailField.message)
        status == 400 && text.contains("redirect", ignoreCase = true) ->
            AuthUiError(
                AuthErrorCode.RedirectNotAllowed,
                "The redirect URL is not allowed. Add it under Allowed redirect URLs (Admin UI, Authentication). $text"
            )
        context == AuthContext.Provider -> AuthUiError(AuthErrorCode.ProviderFailed, text)
        status == 429 -> AuthUiError(AuthErrorCode.Unknown, "Too many attempts. Try again in a moment.")
        else -> AuthUiError(AuthErrorCode.Unknown, text)
    }
}

/**
 * Headless sign-in state machine. UI kits (Compose, custom) observe [state] and call the actions.
 *
 * Methods come from [AppAuth.providers] when the controller starts (cached 60 s there), so enabling a
 * provider in the Admin UI shows its button without an app update.
 */
class AuthController(
    private val auth: AppAuth,
    options: AuthOptions = AuthOptions(),
    scope: CoroutineScope? = null
) : AuthActions {
    constructor(client: MonolithClient, options: AuthOptions = AuthOptions(), scope: CoroutineScope? = null) :
        this(client.auth, options, scope)

    private data class Raw(
        val methods: PublicAuth? = null,
        val loadingMethods: Boolean = true,
        val mode: AuthMode = AuthMode.SignIn,
        val submitting: Boolean = false,
        val activeProvider: String? = null,
        val error: AuthUiError? = null
    )

    private val ownsScope = scope == null
    private val scope: CoroutineScope = scope ?: CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val options = MutableStateFlow(options)
    private val raw = MutableStateFlow(Raw(mode = if (options.signUp == SignUpMode.Only) AuthMode.SignUp else AuthMode.SignIn))

    val state: StateFlow<AuthUiState> = combine(raw, this.options, auth.state) { r, o, s -> build(r, o, s) }
        .stateIn(this.scope, SharingStarted.Eagerly, build(raw.value, options, auth.state.value))

    init {
        this.scope.launch {
            auth.state.collect { s ->
                if (s is AuthState.SignedIn) raw.update { it.copy(submitting = false, activeProvider = null, error = null) }
            }
        }
        this.scope.launch {
            auth.redirectFailures.collect { f ->
                raw.update {
                    it.copy(
                        submitting = false,
                        activeProvider = null,
                        error = AuthUiError(AuthErrorCode.ProviderFailed, f.description ?: "Sign-in with the provider failed (${f.code}).")
                    )
                }
            }
        }
        reloadMethods(force = false)
    }

    /** Replaces the options (UI kits call this when their options change). */
    fun setOptions(options: AuthOptions) {
        val old = this.options.value
        if (old == options) return
        this.options.value = options
        if (options.signUp == SignUpMode.Only && old.signUp != SignUpMode.Only) raw.update { it.copy(mode = AuthMode.SignUp) }
    }

    /** Fetches the methods again (after a `network` error, or on pull to refresh). */
    fun reloadMethods(force: Boolean = true) {
        raw.update { it.copy(loadingMethods = true) }
        scope.launch {
            try {
                val methods = auth.providers(force)
                raw.update { it.copy(methods = methods, loadingMethods = false) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                val err = (e as? MonolithException)?.toAuthUiError() ?: AuthUiError(AuthErrorCode.Unknown, e.message ?: "Unknown error")
                raw.update { it.copy(loadingMethods = false, error = err) }
            }
        }
    }

    override fun setMode(mode: AuthMode) = raw.update { it.copy(mode = mode, error = null) }

    override fun dismissError() = raw.update { it.copy(error = null) }

    override fun signInWithEmail(email: String, password: String) {
        validateEmail(email)?.let { return fail(it) }
        if (password.isEmpty()) return fail(AuthUiError(AuthErrorCode.InvalidCredentials, "Enter your password."))
        submit(null, AuthContext.SignIn) { auth.signIn(email.trim(), password) }
    }

    override fun signUpWithEmail(email: String, password: String, name: String) {
        validateEmail(email)?.let { return fail(it) }
        val min = raw.value.methods?.emailPassword?.minPasswordLength ?: 8
        if (password.length < min) {
            return fail(AuthUiError(AuthErrorCode.WeakPassword, "Use at least $min characters for the password."))
        }
        submit(null, AuthContext.SignUp) { auth.signUp(email.trim(), password, name.trim().ifEmpty { null }) }
    }

    override fun signInWithProvider(id: String) {
        if (!auth.canLaunchBrowser) {
            return fail(AuthUiError(AuthErrorCode.ProviderFailed, "No browser launcher is configured, so the provider page cannot open."))
        }
        submit(id, AuthContext.Provider, keepSubmitting = true) { auth.signInWithProvider(id) }
    }

    override fun signOut() {
        scope.launch { auth.signOut() }
    }

    /**
     * Call when the app returns to the foreground. If a provider page was open and no redirect is being
     * completed, the user closed it: clear the busy state.
     */
    fun onAppResumed() {
        if (raw.value.activeProvider == null) return
        scope.launch {
            delay(RESUME_GRACE_MS)
            if (!auth.redirecting.value && auth.state.value !is AuthState.SignedIn) {
                raw.update { it.copy(submitting = false, activeProvider = null) }
            }
        }
    }

    /** Stops background work if the controller created its own scope. */
    fun close() {
        if (ownsScope) scope.cancel()
    }

    // --- internals ---------------------------------------------------------------------------

    private fun fail(error: AuthUiError) = raw.update { it.copy(error = error) }

    private fun validateEmail(email: String): AuthUiError? =
        if (EMAIL.matches(email.trim())) null else AuthUiError(AuthErrorCode.InvalidEmail, "Enter a valid email address.")

    private fun submit(provider: String?, context: AuthContext, keepSubmitting: Boolean = false, block: suspend () -> Unit) {
        while (true) {
            val old = raw.value
            if (old.submitting) return
            if (raw.compareAndSet(old, old.copy(submitting = true, activeProvider = provider, error = null))) break
        }
        scope.launch {
            try {
                block()
                if (!keepSubmitting) raw.update { it.copy(submitting = false, activeProvider = null) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                val err = (e as? MonolithException)?.toAuthUiError(context)
                    ?: AuthUiError(AuthErrorCode.Unknown, e.message ?: "Unknown error")
                raw.update { it.copy(submitting = false, activeProvider = null, error = err) }
            }
        }
    }

    private fun build(r: Raw, o: AuthOptions, s: AuthState): AuthUiState {
        val m = r.methods
        val showEmail = when (o.email) {
            EmailMode.Hide -> false
            EmailMode.Force -> true
            EmailMode.Auto -> m?.emailPassword?.enabled == true
        }
        val canSignUp = showEmail && o.signUp != SignUpMode.Hide &&
            (m?.emailPassword?.allowSignup ?: (o.email == EmailMode.Force))
        val server = m?.providers.orEmpty().filter { it.id !in o.hideProviders }
        val providers = o.providers?.let { order -> order.mapNotNull { id -> server.firstOrNull { it.id == id } } } ?: server
        val mode = if (r.mode == AuthMode.SignUp && !canSignUp && m != null) AuthMode.SignIn else r.mode
        return AuthUiState(
            loading = r.loadingMethods || s is AuthState.Unknown,
            methods = m,
            mode = mode,
            submitting = r.submitting,
            activeProvider = r.activeProvider,
            error = r.error,
            user = (s as? AuthState.SignedIn)?.user,
            showEmailForm = showEmail,
            canSignUp = canSignUp,
            providers = providers,
            minPasswordLength = m?.emailPassword?.minPasswordLength ?: 8
        )
    }

    private companion object {
        val EMAIL = Regex("^[^@\\s]+@[^@\\s]+\\.[^@\\s]+$")
        const val RESUME_GRACE_MS = 400L
    }
}
