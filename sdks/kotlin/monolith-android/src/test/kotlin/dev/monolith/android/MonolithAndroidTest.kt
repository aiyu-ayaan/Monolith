package dev.monolith.android

import dev.monolith.android.storage.TokenStorage
import org.junit.Assert.*
import org.junit.Test

class FakeTokenStorage(private var token: String? = null) : TokenStorage {
    override fun getToken(): String? = token
    override fun setToken(token: String?) {
        this.token = token
    }
    override fun clearToken() {
        this.token = null
    }
}

class MonolithAndroidTest {

    @Test
    fun testBuilderRequiresValidUrl() {
        assertThrows(IllegalArgumentException::class.java) {
            // Fake context or builder validation
            val builder = MonolithAndroid.Builder(TestContext())
            builder.url("")
            builder.build()
        }
    }

    @Test
    fun testBuilderWithTokenStorage() {
        val fakeStorage = FakeTokenStorage("initial_saved_token")
        val client = MonolithAndroid.Builder(TestContext())
            .url("https://api.example.com")
            .app("demo")
            .tokenStorage(fakeStorage)
            .build()

        // Verify initial token loaded from storage
        assertEquals("initial_saved_token", client.http.getToken())

        // Verify token change updates storage
        client.auth().setToken("new_token_xyz")
        assertEquals("new_token_xyz", fakeStorage.getToken())

        // Verify token clear updates storage
        client.auth().signOut()
        assertNull(fakeStorage.getToken())
    }

    @Test
    fun testBuilderExplicitTokenOverridesStorage() {
        val fakeStorage = FakeTokenStorage("initial_saved_token")
        val client = MonolithAndroid.Builder(TestContext())
            .url("https://api.example.com")
            .token("explicit_token_123")
            .tokenStorage(fakeStorage)
            .build()

        assertEquals("explicit_token_123", client.http.getToken())
    }
}

// Minimal stub for Android Context in unit tests without robolectric
private class TestContext : android.content.ContextWrapper(null) {
    override fun getApplicationContext(): android.content.Context = this
}
