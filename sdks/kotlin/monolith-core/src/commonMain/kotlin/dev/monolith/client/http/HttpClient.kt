package dev.monolith.client.http

import dev.monolith.client.ClientOptions
import io.ktor.client.HttpClient as KtorHttpClient
import io.ktor.client.engine.cio.CIO
import io.ktor.client.plugins.contentnegotiation.ContentNegotiation
import io.ktor.client.request.*
import io.ktor.client.statement.*
import io.ktor.http.*
import io.ktor.serialization.kotlinx.json.json
import kotlinx.coroutines.delay
import kotlinx.serialization.KSerializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.serializer
import kotlin.math.min
import kotlin.math.pow
import kotlin.random.Random

/**
 * Low-level HTTP transport client for Monolith.
 *
 * Implements exponential backoff retries for GET requests, token management,
 * and RFC 9457 Problem Details error decoding.
 */
class HttpClient(options: ClientOptions) {
    val baseUrl: String = options.url.trimEnd('/')
    val app: String = options.app

    val json: Json = Json {
        ignoreUnknownKeys = true
        isLenient = true
        encodeDefaults = true
        prettyPrint = false
    }

    private val ownsKtorClient: Boolean = options.httpClient == null
    val ktorClient: KtorHttpClient = options.httpClient ?: KtorHttpClient(CIO) {
        install(ContentNegotiation) {
            json(this@HttpClient.json)
        }
    }

    private var token: String? = options.token
    private var apiKey: String? = options.key
    private val defaultHeaders: Map<String, String> = options.headers

    fun setToken(token: String?) {
        this.token = token
    }

    fun getToken(): String? = this.token

    fun setApiKey(apiKey: String?) {
        this.apiKey = apiKey
    }

    fun getApiKey(): String? = this.apiKey

    /**
     * Builds a full URL with query parameters encoded.
     */
    fun buildUrl(path: String, query: Map<String, Any?>? = null): String {
        val cleanPath = if (path.startsWith("/")) path else "/$path"
        val urlBuilder = URLBuilder("$baseUrl$cleanPath")

        query?.forEach { (key, value) ->
            if (value != null) {
                when (value) {
                    is Iterable<*> -> {
                        value.filterNotNull().forEach { item ->
                            urlBuilder.parameters.append(key, item.toString())
                        }
                    }
                    else -> {
                        urlBuilder.parameters.append(key, value.toString())
                    }
                }
            }
        }

        return urlBuilder.buildString()
    }

    /**
     * Executes an HTTP request with automatic retries on GET, Bearer auth attachment,
     * and RFC 9457 error decoding.
     */
    suspend fun <T> request(
        method: HttpMethod,
        path: String,
        query: Map<String, Any?>? = null,
        body: Any? = null,
        headers: Map<String, String>? = null,
        serializer: KSerializer<T>? = null,
        bodySerializer: KSerializer<*>? = null
    ): T {
        val url = buildUrl(path, query)
        val isGet = method == HttpMethod.Get
        val maxAttempts = if (isGet) 4 else 1

        val mergedHeaders = mutableMapOf<String, String>()
        mergedHeaders.putAll(defaultHeaders)
        if (headers != null) {
            mergedHeaders.putAll(headers)
        }

        val authToken = token ?: apiKey
        if (authToken != null && !mergedHeaders.containsKey("Authorization") && !mergedHeaders.containsKey("authorization")) {
            mergedHeaders["Authorization"] = "Bearer $authToken"
        }

        for (attempt in 0 until maxAttempts) {
            val response: HttpResponse = try {
                ktorClient.request(url) {
                    this.method = method
                    mergedHeaders.forEach { (k, v) ->
                        this.headers.append(k, v)
                    }

                    if (body != null) {
                        when {
                            body is ByteArray -> {
                                setBody(body)
                            }
                            body is String -> {
                                if (!mergedHeaders.containsKey("Content-Type") && !mergedHeaders.containsKey("content-type")) {
                                    contentType(ContentType.Application.Json)
                                }
                                setBody(body)
                            }
                            body is JsonElement -> {
                                contentType(ContentType.Application.Json)
                                setBody(body.toString())
                            }
                            bodySerializer != null -> {
                                contentType(ContentType.Application.Json)
                                @Suppress("UNCHECKED_CAST")
                                val encoded = json.encodeToString(bodySerializer as KSerializer<Any>, body)
                                setBody(encoded)
                            }
                            else -> {
                                contentType(ContentType.Application.Json)
                                setBody(body)
                            }
                        }
                    }
                }
            } catch (e: Throwable) {
                if (attempt < maxAttempts - 1) {
                    val backoff = min(100L * 2.0.pow(attempt).toLong() + Random.nextLong(50L), 2000L)
                    delay(backoff)
                    continue
                }
                throw MonolithException(
                    Problem(
                        type = "https://monolith.dev/errors/network",
                        title = "Network Error",
                        status = 0,
                        detail = e.message ?: e.toString()
                    ),
                    cause = e
                )
            }

            // Retry on 503 for idempotent GET requests
            if (isGet && response.status == HttpStatusCode.ServiceUnavailable && attempt < maxAttempts - 1) {
                val retryAfter = response.headers["Retry-After"]?.toLongOrNull()
                val delayMs = if (retryAfter != null) retryAfter * 1000L else (100L * 2.0.pow(attempt).toLong())
                delay(delayMs)
                continue
            }

            // Handle 204 No Content
            if (response.status == HttpStatusCode.NoContent) {
                @Suppress("UNCHECKED_CAST")
                return Unit as T
            }

            val bodyText = response.bodyAsText()

            // Non-2xx response handling (RFC 9457 Problem Details)
            if (!response.status.isSuccess()) {
                val requestId = response.headers["x-request-id"] ?: response.headers["X-Request-Id"]
                val contentType = response.contentType()?.toString() ?: ""
                val isJson = contentType.contains("application/json") || contentType.contains("problem+json")

                val problem = if (isJson && bodyText.isNotBlank()) {
                    try {
                        val parsed = json.decodeFromString(Problem.serializer(), bodyText)
                        if (parsed.requestId == null && requestId != null) {
                            parsed.copy(requestId = requestId)
                        } else {
                            parsed
                        }
                    } catch (_: Throwable) {
                        Problem(
                            type = "https://monolith.dev/errors/http",
                            title = response.status.description,
                            status = response.status.value,
                            detail = bodyText.ifBlank { null },
                            requestId = requestId
                        )
                    }
                } else {
                    Problem(
                        type = "https://monolith.dev/errors/http",
                        title = response.status.description,
                        status = response.status.value,
                        detail = bodyText.ifBlank { null },
                        requestId = requestId
                    )
                }

                throw MonolithException(problem)
            }

            // Successful response deserialization
            if (serializer == null || serializer.descriptor.serialName == "kotlin.Unit") {
                @Suppress("UNCHECKED_CAST")
                return Unit as T
            }

            return json.decodeFromString(serializer, bodyText)
        }

        throw MonolithException(
            Problem(
                type = "https://monolith.dev/errors/request-failed",
                title = "Request failed after maximum retry attempts",
                status = 0
            )
        )
    }

    suspend inline fun <reified T> get(
        path: String,
        query: Map<String, Any?>? = null,
        headers: Map<String, String>? = null
    ): T {
        return request(HttpMethod.Get, path, query, null, headers, serializer<T>())
    }

    suspend inline fun <reified T> post(
        path: String,
        body: Any? = null,
        query: Map<String, Any?>? = null,
        headers: Map<String, String>? = null
    ): T {
        return request(HttpMethod.Post, path, query, body, headers, serializer<T>())
    }

    suspend inline fun <reified T> put(
        path: String,
        body: Any? = null,
        query: Map<String, Any?>? = null,
        headers: Map<String, String>? = null
    ): T {
        return request(HttpMethod.Put, path, query, body, headers, serializer<T>())
    }

    suspend inline fun <reified T> patch(
        path: String,
        body: Any? = null,
        query: Map<String, Any?>? = null,
        headers: Map<String, String>? = null
    ): T {
        return request(HttpMethod.Patch, path, query, body, headers, serializer<T>())
    }

    suspend inline fun <reified T> delete(
        path: String,
        query: Map<String, Any?>? = null,
        headers: Map<String, String>? = null
    ): T {
        return request(HttpMethod.Delete, path, query, null, headers, serializer<T>())
    }

    suspend inline fun <reified T> putBytes(
        path: String,
        bytes: ByteArray,
        headers: Map<String, String>? = null
    ): T {
        return request(HttpMethod.Put, path, null, bytes, headers, serializer<T>())
    }

    fun close() {
        if (ownsKtorClient) {
            ktorClient.close()
        }
    }
}
