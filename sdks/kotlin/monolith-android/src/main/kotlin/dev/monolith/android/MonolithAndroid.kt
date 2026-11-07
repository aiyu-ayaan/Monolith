package dev.monolith.android

import android.content.Context
import dev.monolith.android.storage.EncryptedTokenStorage
import dev.monolith.android.storage.TokenStorage
import dev.monolith.client.ClientOptions
import dev.monolith.client.MonolithClient
import io.ktor.client.HttpClient

/**
 * Android integration and client builder for Monolith.
 *
 * Provides Android context integration, automatic encrypted token storage,
 * and lifecycle-friendly configuration.
 */
object MonolithAndroid {

    /**
     * Creates a configured [MonolithClient] for Android with automatic encrypted token storage.
     *
     * @param context Android context (Application context is retained).
     * @param url Base URL of the Monolith server.
     * @param app Target app name (defaults to "default").
     * @param key Optional API key for server authorization.
     * @param token Optional initial bearer token (if omitted, loads from [TokenStorage]).
     * @param tokenStorage Custom [TokenStorage] implementation (defaults to [EncryptedTokenStorage]).
     */
    @JvmStatic
    fun create(
        context: Context,
        url: String,
        app: String = "default",
        key: String? = null,
        token: String? = null,
        tokenStorage: TokenStorage? = null
    ): MonolithClient {
        return Builder(context)
            .url(url)
            .app(app)
            .key(key)
            .token(token)
            .tokenStorage(tokenStorage)
            .build()
    }

    /**
     * Fluent builder for configuring a [MonolithClient] on Android.
     */
    class Builder(context: Context) {
        private val appContext: Context = context.applicationContext
        private var url: String = ""
        private var app: String = "default"
        private var key: String? = null
        private var token: String? = null
        private val headers = mutableMapOf<String, String>()
        private var customHttpClient: HttpClient? = null
        private var tokenStorage: TokenStorage? = null
        private var persistAuthTokens: Boolean = true

        fun url(url: String) = apply { this.url = url }
        fun app(app: String) = apply { this.app = app }
        fun key(key: String?) = apply { this.key = key }
        fun token(token: String?) = apply { this.token = token }
        fun header(name: String, value: String) = apply { this.headers[name] = value }
        fun headers(headers: Map<String, String>) = apply { this.headers.putAll(headers) }
        fun httpClient(client: HttpClient?) = apply { this.customHttpClient = client }
        fun tokenStorage(storage: TokenStorage?) = apply { this.tokenStorage = storage }
        fun persistAuthTokens(persist: Boolean) = apply { this.persistAuthTokens = persist }

        fun build(): MonolithClient {
            require(url.isNotBlank()) { "url must not be blank" }

            val storage = tokenStorage ?: if (persistAuthTokens) {
                EncryptedTokenStorage(appContext)
            } else null

            // Load persisted token if initial token was not explicitly supplied
            val initialToken = token ?: storage?.getToken()

            val options = ClientOptions(
                url = url,
                app = app,
                key = key,
                token = initialToken,
                headers = headers,
                httpClient = customHttpClient
            )

            val client = MonolithClient(options)

            // Auto-sync token changes to persistent storage
            if (storage != null) {
                client.auth().onChange { newToken ->
                    if (newToken != null) {
                        storage.setToken(newToken)
                    } else {
                        storage.clearToken()
                    }
                }
            }

            return client
        }
    }
}
