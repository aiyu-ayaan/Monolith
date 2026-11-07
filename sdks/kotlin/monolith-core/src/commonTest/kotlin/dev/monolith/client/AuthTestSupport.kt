package dev.monolith.client

import dev.monolith.client.auth.BrowserLauncher
import dev.monolith.client.auth.MemorySessionStorage
import dev.monolith.client.auth.SessionOptions
import io.ktor.client.HttpClient
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.plugins.contentnegotiation.ContentNegotiation
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import io.ktor.serialization.kotlinx.json.json
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import kotlinx.datetime.Instant
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/** In-memory stand-in for the app-user auth endpoints with rotating tokens and a controllable clock. */
class FakeAuthServer {
    var now = 1_700_000_000_000L
    var generation = 1
    var rejectRefresh = false
    var refreshDelayMs = 0L
    var providersJson = """{"app":"testapp","emailPassword":{"enabled":true,"allowSignup":true,"minPasswordLength":8},
        "providers":[{"id":"google","name":"Google","startPath":"/x"},{"id":"github","name":"GitHub","startPath":"/y"}]}"""
    var loginStatus = HttpStatusCode.OK
    var registerStatus = HttpStatusCode.Created
    var providersFail = false
    var rejectRecords = false
    val revoked = mutableSetOf<String>()
    var providerRequests = 0
    var refreshRequests = 0
    var recordRequests = 0
    val bodies = mutableMapOf<String, String>()
    val seenAuth = mutableListOf<String?>()

    val token get() = "t$generation"
    val refreshToken get() = "r$generation"

    fun expiry(inMs: Long = 900_000) = Instant.fromEpochMilliseconds(now + inMs).toString()

    fun sessionJson(name: String = "Ann", inMs: Long = 900_000) =
        """{"token":"$token","refreshToken":"$refreshToken","expiresAt":"${expiry(inMs)}",
           "user":{"id":"u1","email":"ann@x.io","name":"$name","created":"2026-01-01T00:00:00Z","disabled":false,"providers":["password"]}}"""

    private val jsonHeaders = headersOf(HttpHeaders.ContentType to listOf("application/json"))
    private val problemHeaders = headersOf(HttpHeaders.ContentType to listOf("application/problem+json"))

    val engine = MockEngine { req ->
        val path = req.url.encodedPath.removePrefix("/api/v1/apps/testapp/")
        val bodyText = (req.body as? io.ktor.http.content.TextContent)?.text
        if (bodyText != null) bodies[path] = bodyText
        val auth = req.headers[HttpHeaders.Authorization]
        when {
            path == "auth/providers" -> {
                providerRequests++
                if (providersFail) respond("""{"title":"x","status":503}""", HttpStatusCode.ServiceUnavailable, problemHeaders)
                else respond(providersJson, HttpStatusCode.OK, jsonHeaders)
            }
            path == "auth/login" -> {
                if (loginStatus != HttpStatusCode.OK) problem(loginStatus, "bad credentials")
                else respond(sessionJson(), HttpStatusCode.OK, jsonHeaders)
            }
            path == "auth/register" -> {
                if (registerStatus != HttpStatusCode.Created) problem(registerStatus, "register failed")
                else respond(sessionJson(), HttpStatusCode.Created, jsonHeaders)
            }
            path == "auth/refresh" -> {
                refreshRequests++
                if (refreshDelayMs > 0) delay(refreshDelayMs)
                val sent = Json.parseToJsonElement(bodyText!!).jsonObject["refreshToken"]!!.jsonPrimitive.content
                if (rejectRefresh || sent != refreshToken) problem(HttpStatusCode.Unauthorized, "invalid refresh token")
                else {
                    generation++
                    respond(sessionJson(), HttpStatusCode.OK, jsonHeaders)
                }
            }
            path == "auth/logout" -> respond("", HttpStatusCode.NoContent)
            path == "auth/password" -> respond("", HttpStatusCode.NoContent)
            path == "auth/me" && req.method == HttpMethod.Get ->
                if (auth == "Bearer $token") respond(
                    """{"id":"u1","email":"ann@x.io","name":"Ann","providers":["google"]}""", HttpStatusCode.OK, jsonHeaders
                ) else problem(HttpStatusCode.Unauthorized, "bad token")
            path == "auth/me" && req.method == HttpMethod.Patch ->
                respond("""{"id":"u1","email":"ann@x.io","name":"Bob","providers":["password"]}""", HttpStatusCode.OK, jsonHeaders)
            path == "collections/tasks/records" -> {
                recordRequests++
                seenAuth += auth
                if (auth == "Bearer $token" && token !in revoked && !rejectRecords) respond("""{"items":[]}""", HttpStatusCode.OK, jsonHeaders)
                else problem(HttpStatusCode.Unauthorized, "expired")
            }
            else -> respond("not found", HttpStatusCode.NotFound)
        }
    }

    private fun io.ktor.client.engine.mock.MockRequestHandleScope.problem(status: HttpStatusCode, detail: String) =
        respond(
            """{"type":"https://monolith.dev/errors/x","title":"${status.description}","status":${status.value},"detail":"$detail"}""",
            status,
            problemHeaders
        )

    fun client(
        storage: MemorySessionStorage? = null,
        scope: CoroutineScope? = null,
        launcher: BrowserLauncher? = null,
        redirectUri: String? = "monolith-testapp://auth"
    ): MonolithClient {
        val ktor = HttpClient(engine) {
            install(ContentNegotiation) { json(Json { ignoreUnknownKeys = true }) }
        }
        return MonolithClient(
            ClientOptions(
                url = "https://api.example.com",
                app = "testapp",
                httpClient = ktor,
                session = SessionOptions(
                    storage = storage,
                    browserLauncher = launcher,
                    redirectUri = redirectUri,
                    scope = scope,
                    clock = { now }
                )
            )
        )
    }
}
