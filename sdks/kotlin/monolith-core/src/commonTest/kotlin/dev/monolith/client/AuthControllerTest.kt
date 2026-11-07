package dev.monolith.client

import dev.monolith.client.auth.*
import io.ktor.http.HttpStatusCode
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.*

class AuthControllerTest {
    private suspend fun AuthController.loaded(): AuthUiState = state.first { !it.loading }

    @Test
    fun loadsMethodsAndAppliesDefaults() = runTest {
        val server = FakeAuthServer()
        val c = AuthController(server.client(scope = backgroundScope), scope = backgroundScope)
        val s = c.loaded()
        assertTrue(s.showEmailForm)
        assertTrue(s.canSignUp)
        assertEquals(listOf("google", "github"), s.providers.map { it.id })
        assertEquals(8, s.minPasswordLength)
        assertEquals(AuthMode.SignIn, s.mode)
    }

    @Test
    fun optionsFilterAndOrderProviders() = runTest {
        val server = FakeAuthServer()
        val mono = server.client(scope = backgroundScope)
        val c = AuthController(mono, AuthOptions(providers = listOf("github", "apple", "google")), backgroundScope)
        assertEquals(listOf("github", "google"), c.loaded().providers.map { it.id })
        c.setOptions(AuthOptions(hideProviders = setOf("google")))
        assertEquals(listOf("github"), c.state.first { it.providers.size == 1 }.providers.map { it.id })
        c.setOptions(AuthOptions(email = EmailMode.Hide, signUp = SignUpMode.Hide))
        val s = c.state.first { !it.showEmailForm }
        assertFalse(s.canSignUp)
    }

    @Test
    fun signUpOnlyStartsInSignUpAndFallsBackWhenSignupIsClosed() = runTest {
        val open = AuthController(FakeAuthServer().client(scope = backgroundScope), AuthOptions(signUp = SignUpMode.Only), backgroundScope)
        assertEquals(AuthMode.SignUp, open.loaded().mode)

        val closed = FakeAuthServer().apply {
            providersJson = """{"app":"testapp","emailPassword":{"enabled":true,"allowSignup":false,"minPasswordLength":8},"providers":[]}"""
        }
        val c = AuthController(closed.client(scope = backgroundScope), AuthOptions(signUp = SignUpMode.Only), backgroundScope)
        val s = c.loaded()
        assertFalse(s.canSignUp)
        assertEquals(AuthMode.SignIn, s.mode)
    }

    @Test
    fun validatesBeforeAnyRequest() = runTest {
        val server = FakeAuthServer()
        val c = AuthController(server.client(scope = backgroundScope), scope = backgroundScope)
        c.loaded()
        c.signInWithEmail("not-an-email", "secret123")
        runCurrent()
        assertEquals(AuthErrorCode.InvalidEmail, c.state.value.error?.code)
        c.dismissError()
        runCurrent()
        assertNull(c.state.value.error)
        c.signUpWithEmail("ann@x.io", "short")
        runCurrent()
        assertEquals(AuthErrorCode.WeakPassword, c.state.value.error?.code)
        c.signInWithEmail("ann@x.io", "")
        runCurrent()
        assertEquals(AuthErrorCode.InvalidCredentials, c.state.value.error?.code)
        assertFalse(server.bodies.containsKey("auth/login"))
        assertFalse(server.bodies.containsKey("auth/register"))
    }

    @Test
    fun signInSuccessSetsUserAndClearsBusyState() = runTest {
        val server = FakeAuthServer()
        val c = AuthController(server.client(scope = backgroundScope), scope = backgroundScope)
        c.loaded()
        c.signInWithEmail(" ann@x.io ", "secret123")
        val s = c.state.first { it.user != null }
        assertEquals("Ann", s.user?.name)
        assertFalse(s.submitting)
        assertTrue(server.bodies["auth/login"]!!.contains("\"email\":\"ann@x.io\""))
    }

    @Test
    fun serverErrorsMapToCodes() = runTest {
        val server = FakeAuthServer().apply { loginStatus = HttpStatusCode.Unauthorized; registerStatus = HttpStatusCode.Conflict }
        val c = AuthController(server.client(scope = backgroundScope), scope = backgroundScope)
        c.loaded()
        c.signInWithEmail("ann@x.io", "secret123")
        assertEquals(AuthErrorCode.InvalidCredentials, c.state.first { it.error != null }.error?.code)
        assertFalse(c.state.value.submitting)
        c.signUpWithEmail("ann@x.io", "secret123")
        assertEquals(AuthErrorCode.EmailTaken, c.state.first { it.error?.code == AuthErrorCode.EmailTaken }.error?.code)
        server.registerStatus = HttpStatusCode.Forbidden
        c.signUpWithEmail("ann@x.io", "secret123")
        assertEquals(AuthErrorCode.SignupClosed, c.state.first { it.error?.code == AuthErrorCode.SignupClosed }.error?.code)
    }

    @Test
    fun providerSignInNeedsALauncherAndKeepsBusyStateWhileOpen() = runTest {
        val server = FakeAuthServer()
        val none = AuthController(server.client(scope = backgroundScope), scope = backgroundScope)
        none.loaded()
        none.signInWithProvider("google")
        runCurrent()
        assertEquals(AuthErrorCode.ProviderFailed, none.state.value.error?.code)

        val urls = mutableListOf<String>()
        val c = AuthController(server.client(scope = backgroundScope, launcher = { urls += it }), scope = backgroundScope)
        c.loaded()
        c.signInWithProvider("google")
        val s = c.state.first { it.activeProvider == "google" }
        assertTrue(s.submitting)
        assertTrue(urls.single().contains("/auth/oauth/google/start"))
    }

    @Test
    fun failedProviderRedirectShowsAnError() = runTest {
        val server = FakeAuthServer()
        val mono = server.client(scope = backgroundScope)
        val c = AuthController(mono, scope = backgroundScope)
        c.loaded()
        mono.auth.handleRedirect("monolith-testapp://auth#error=oauth_failed&error_description=email%20taken")
        val s = c.state.first { it.error != null }
        assertEquals(AuthErrorCode.ProviderFailed, s.error?.code)
        assertFalse(s.submitting)
    }

    @Test
    fun providerListFailureShowsNetworkOrUnknownError() = runTest {
        val server = FakeAuthServer().apply { providersFail = true }
        val c = AuthController(server.client(scope = backgroundScope), scope = backgroundScope)
        val s = c.loaded()
        assertNull(s.methods)
        assertNotNull(s.error)
        server.providersFail = false
        c.reloadMethods()
        assertNotNull(c.state.first { it.methods != null }.methods)
    }
}
