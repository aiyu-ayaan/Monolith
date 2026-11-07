package dev.monolith.client

import dev.monolith.client.auth.*
import dev.monolith.client.http.MonolithException
import io.ktor.http.HttpStatusCode
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.JsonElement
import kotlin.test.*

class AppAuthTest {
    private val records = "/api/v1/apps/testapp/collections/tasks/records"

    @Test
    fun signInSetsSessionTokenStateAndStorage() = runTest {
        val server = FakeAuthServer()
        val storage = MemorySessionStorage()
        val mono = server.client(storage, backgroundScope)
        assertEquals(AuthState.Unknown, mono.auth.state.value)

        val user = mono.auth.signIn("ann@x.io", "secret123")
        assertEquals("Ann", user.name)
        assertEquals(AuthState.SignedIn(user), mono.auth.state.value)
        assertEquals("t1", mono.http.getToken())
        assertEquals(user, mono.auth.user)
        assertTrue(storage.load()!!.contains("r1"))
    }

    @Test
    fun signUpSendsNameOnlyWhenGiven() = runTest {
        val server = FakeAuthServer()
        val mono = server.client()
        mono.auth.signUp("ann@x.io", "secret123")
        assertFalse(server.bodies["auth/register"]!!.contains("name"))
        mono.auth.signUp("ann@x.io", "secret123", "Ann")
        assertTrue(server.bodies["auth/register"]!!.contains("\"name\":\"Ann\""))
    }

    @Test
    fun signInErrorsSurfaceAsMonolithException() = runTest {
        val server = FakeAuthServer().apply { loginStatus = HttpStatusCode.Unauthorized }
        val mono = server.client()
        val e = assertFailsWith<MonolithException> { mono.auth.signIn("ann@x.io", "nope") }
        assertEquals(401, e.status)
        assertEquals(AuthState.SignedOut, mono.auth.state.value)
    }

    @Test
    fun restoresStoredSessionAndStartsUnknown() = runTest {
        val server = FakeAuthServer()
        val storage = MemorySessionStorage(server.sessionJson())
        val mono = server.client(storage, backgroundScope)
        assertEquals(AuthState.Unknown, mono.auth.state.value)
        val state = mono.auth.state.first { it !is AuthState.Unknown }
        assertIs<AuthState.SignedIn>(state)
        assertEquals("t1", mono.http.getToken())
        assertEquals(0, server.refreshRequests) // still fresh
    }

    @Test
    fun restoreWithoutStoredSessionIsSignedOut() = runTest {
        val mono = FakeAuthServer().client(MemorySessionStorage(), backgroundScope)
        assertEquals(AuthState.SignedOut, mono.auth.state.first { it !is AuthState.Unknown })
    }

    @Test
    fun restoreRefreshesAnExpiredSessionInTheBackground() = runTest {
        val server = FakeAuthServer()
        val storage = MemorySessionStorage(server.sessionJson(inMs = -1000))
        val mono = server.client(storage, backgroundScope)
        mono.auth.state.first { it !is AuthState.Unknown }
        mono.auth.accessToken() // waits for restore to finish
        assertEquals(1, server.refreshRequests)
        assertEquals("t2", mono.http.getToken())
        assertTrue(storage.load()!!.contains("r2"))
    }

    @Test
    fun refreshesBeforeExpiryAndSendsTheNewToken() = runTest {
        val server = FakeAuthServer()
        val mono = server.client()
        mono.auth.signIn("ann@x.io", "secret123")
        server.now += 880_000 // 20 s left, inside the 30 s leeway
        mono.http.get<JsonElement>(records)
        assertEquals(1, server.refreshRequests)
        assertEquals(listOf<String?>("Bearer t2"), server.seenAuth)
        // fresh again: no further refresh
        mono.http.get<JsonElement>(records)
        assertEquals(1, server.refreshRequests)
    }

    @Test
    fun refreshIsSingleFlightAcrossConcurrentRequests() = runTest {
        val server = FakeAuthServer().apply { refreshDelayMs = 50 }
        val mono = server.client()
        mono.auth.signIn("ann@x.io", "secret123")
        server.now += 899_000
        (1..8).map { async { mono.http.get<JsonElement>(records) } }.awaitAll()
        assertEquals(1, server.refreshRequests)
        assertTrue(server.seenAuth.all { it == "Bearer t2" })
    }

    @Test
    fun a401TriggersOneRefreshAndOneRetry() = runTest {
        val server = FakeAuthServer()
        val mono = server.client()
        mono.auth.signIn("ann@x.io", "secret123")
        server.revoked += "t1" // token not expired locally, but the server no longer accepts it
        mono.http.get<JsonElement>(records)
        assertEquals(1, server.refreshRequests)
        assertEquals(listOf<String?>("Bearer t1", "Bearer t2"), server.seenAuth)
    }

    @Test
    fun aSecond401DoesNotLoop() = runTest {
        val server = FakeAuthServer()
        val mono = server.client()
        mono.auth.signIn("ann@x.io", "secret123")
        server.rejectRecords = true
        val e = assertFailsWith<MonolithException> { mono.http.get<JsonElement>(records) }
        assertEquals(401, e.status)
        assertEquals(1, server.refreshRequests)
        assertEquals(2, server.recordRequests)
    }

    @Test
    fun rejectedRefreshSignsOutAndClearsStorage() = runTest {
        val server = FakeAuthServer()
        val storage = MemorySessionStorage()
        val mono = server.client(storage)
        mono.auth.signIn("ann@x.io", "secret123")
        server.rejectRefresh = true
        server.now += 899_000
        assertFailsWith<MonolithException> { mono.http.get<JsonElement>(records) }
        assertEquals(AuthState.SignedOut, mono.auth.state.value)
        assertNull(storage.load())
        assertNull(mono.http.getToken())
    }

    @Test
    fun signOutClearsLocallyThenCallsLogoutWithTheRefreshToken() = runTest {
        val server = FakeAuthServer()
        val storage = MemorySessionStorage()
        val mono = server.client(storage)
        mono.auth.signIn("ann@x.io", "secret123")
        mono.auth.signOut()
        assertEquals(AuthState.SignedOut, mono.auth.state.value)
        assertNull(storage.load())
        assertNull(mono.http.getToken())
        assertTrue(server.bodies["auth/logout"]!!.contains("\"refreshToken\":\"r1\""))
    }

    @Test
    fun updateProfileAndChangePassword() = runTest {
        val server = FakeAuthServer()
        val mono = server.client()
        mono.auth.signIn("ann@x.io", "secret123")
        assertEquals("Bob", mono.auth.updateProfile("Bob").name)
        assertEquals("Bob", mono.auth.user?.name)

        mono.auth.changePassword("secret123", "newsecret99")
        assertTrue(server.bodies["auth/password"]!!.contains("newsecret99"))
        assertTrue(server.bodies["auth/login"]!!.contains("newsecret99")) // signed in again
        assertIs<AuthState.SignedIn>(mono.auth.state.value)
    }

    @Test
    fun providersAreCachedFor60Seconds() = runTest {
        val server = FakeAuthServer()
        val mono = server.client()
        assertEquals(listOf("google", "github"), mono.auth.providers().providers.map { it.id })
        mono.auth.providers()
        assertEquals(1, server.providerRequests)
        server.now += 61_000
        mono.auth.providers()
        assertEquals(2, server.providerRequests)
        mono.auth.providers(forceRefresh = true)
        assertEquals(3, server.providerRequests)
    }

    @Test
    fun signInWithProviderBuildsTheStartUrlAndLaunches() = runTest {
        var launched: String? = null
        val mono = FakeAuthServer().client(launcher = BrowserLauncher { launched = it })
        val url = mono.auth.signInWithProvider("google")
        assertEquals(url, launched)
        assertTrue(url.startsWith("https://api.example.com/api/v1/apps/testapp/auth/oauth/google/start?redirectUri="))
        assertTrue(url.contains("monolith-testapp%3A%2F%2Fauth"))
    }

    @Test
    fun handleRedirectParsesTheFragmentAndFetchesTheUser() = runTest {
        val server = FakeAuthServer()
        val storage = MemorySessionStorage()
        val mono = server.client(storage)
        val uri = "monolith-testapp://auth#token=t1&refreshToken=r1&expiresAt=${server.expiry().replace(":", "%3A")}"
        val result = mono.auth.handleRedirect(uri)
        assertIs<RedirectResult.Success>(result)
        assertEquals("u1", result.user.id)
        assertEquals("t1", mono.http.getToken())
        assertIs<AuthState.SignedIn>(mono.auth.state.value)
        assertTrue(storage.load()!!.contains("r1"))
    }

    @Test
    fun handleRedirectReportsProviderErrors() = runTest {
        val mono = FakeAuthServer().client()
        val result = mono.auth.handleRedirect("monolith-testapp://auth#error=oauth_failed&error_description=access%20denied")
        assertEquals(RedirectResult.Failure("oauth_failed", "access denied"), result)
        assertEquals(AuthState.SignedOut, mono.auth.state.value)
    }

    @Test
    fun handleRedirectIgnoresOtherUris() = runTest {
        val mono = FakeAuthServer().client()
        assertEquals(RedirectResult.NotHandled, mono.auth.handleRedirect("https://elsewhere.example/cb#token=a&refreshToken=b"))
        assertEquals(RedirectResult.NotHandled, mono.auth.handleRedirect("monolith-testapp://auth"))
        assertEquals(RedirectResult.NotHandled, mono.auth.handleRedirect("monolith-testapp://auth#foo=bar"))
    }

    @Test
    fun legacySuperuserAuthStillWorks() = runTest {
        val mono = FakeAuthServer().client()
        mono.auth().setToken("manual")
        assertEquals("manual", mono.http.getToken())
        mono.auth().signOut()
        assertNull(mono.http.getToken())
    }

    @Test
    fun initReturnsTheSameInstancePerUrlAndApp() = runTest {
        Monolith.reset()
        val a = Monolith.init("https://api.example.com/", "shop")
        val b = Monolith.init("https://api.example.com", "shop")
        val c = Monolith.init("https://api.example.com", "other")
        assertSame(a, b)
        assertNotSame(a, c)
        assertSame(a, Monolith.get("https://api.example.com", "shop"))
        Monolith.reset()
        assertNull(Monolith.get("https://api.example.com", "shop"))
    }
}
