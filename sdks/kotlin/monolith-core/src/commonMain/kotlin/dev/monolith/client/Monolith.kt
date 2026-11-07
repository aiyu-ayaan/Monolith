package dev.monolith.client

import dev.monolith.client.auth.BrowserLauncher
import dev.monolith.client.auth.MemorySessionStorage
import dev.monolith.client.auth.SessionOptions
import dev.monolith.client.auth.SessionStorage
import io.ktor.client.HttpClient
import kotlinx.coroutines.sync.Mutex

/**
 * Two-line start:
 * ```
 * val mono = Monolith.init(url = "https://api.example.com", app = "shop")
 * mono.collection<Task>("tasks").onSnapshot().collect { render(it.docs) }
 * ```
 * The client restores a stored session, refreshes tokens before they expire and once after a 401.
 * Calling [init] again with the same url and app returns the same instance.
 */
object Monolith {
    private val lock = Mutex()
    private val instances = HashMap<String, MonolithClient>()

    /**
     * @param storage where the session is kept; memory by default (use the Android module for encrypted storage)
     * @param redirectUri where provider sign-ins return to, e.g. `monolith-shop://auth`
     * @param browserLauncher opens the provider sign-in page
     */
    fun init(
        url: String,
        app: String = "default",
        storage: SessionStorage = MemorySessionStorage(),
        key: String? = null,
        redirectUri: String? = null,
        browserLauncher: BrowserLauncher? = null,
        headers: Map<String, String> = emptyMap(),
        httpClient: HttpClient? = null,
        session: SessionOptions = SessionOptions()
    ): MonolithClient = getOrCreate(registryKey(url, app)) {
        MonolithClient(
            ClientOptions(
                url = url,
                app = app,
                key = key,
                headers = headers,
                httpClient = httpClient,
                session = session.copy(
                    storage = session.storage ?: storage,
                    browserLauncher = session.browserLauncher ?: browserLauncher,
                    redirectUri = session.redirectUri ?: redirectUri
                )
            )
        )
    }

    /** The client created by [init] for this url and app, or null. */
    fun get(url: String, app: String = "default"): MonolithClient? {
        spin()
        try {
            return instances[registryKey(url, app)]
        } finally {
            lock.unlock()
        }
    }

    /** All clients created by [init] (used by platform redirect handlers). */
    fun all(): List<MonolithClient> {
        spin()
        try {
            return instances.values.toList()
        } finally {
            lock.unlock()
        }
    }

    /** Closes and forgets every client. Mostly for tests. */
    suspend fun reset() {
        val all = all()
        spin()
        try {
            instances.clear()
        } finally {
            lock.unlock()
        }
        all.forEach { it.close() }
    }

    private fun registryKey(url: String, app: String) = url.trimEnd('/') + "|" + app

    // Registry access is rare and short; a spin on tryLock keeps this code platform neutral.
    private fun spin() {
        while (!lock.tryLock()) {
            // contended for a few nanoseconds at most
        }
    }

    private fun getOrCreate(key: String, create: () -> MonolithClient): MonolithClient {
        spin()
        try {
            return instances.getOrPut(key, create)
        } finally {
            lock.unlock()
        }
    }
}
