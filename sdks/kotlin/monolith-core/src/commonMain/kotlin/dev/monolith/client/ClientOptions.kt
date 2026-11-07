package dev.monolith.client

import io.ktor.client.HttpClient

/**
 * Options used to initialize a [MonolithClient].
 */
data class ClientOptions(
    val url: String,
    val app: String = "default",
    val key: String? = null,
    val token: String? = null,
    val headers: Map<String, String> = emptyMap(),
    val httpClient: HttpClient? = null
)
