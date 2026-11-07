package dev.monolith.client

import dev.monolith.client.http.*
import io.ktor.client.HttpClient
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.plugins.contentnegotiation.ContentNegotiation
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import io.ktor.serialization.kotlinx.json.json
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlin.test.*

@Serializable
data class TestTask(
    val id: String = "",
    val title: String = "",
    val status: String = "open"
)

class MonolithClientTest {

    private val json = Json {
        ignoreUnknownKeys = true
        isLenient = true
        encodeDefaults = true
    }

    private fun createMockClient(engine: MockEngine): MonolithClient {
        val ktorClient = HttpClient(engine) {
            install(ContentNegotiation) {
                json(this@MonolithClientTest.json)
            }
        }
        return MonolithClient(
            ClientOptions(
                url = "https://api.example.com",
                app = "testapp",
                key = "mk_test_key",
                httpClient = ktorClient
            )
        )
    }

    @Test
    fun testUrlBuilder() {
        val client = MonolithClient(ClientOptions(url = "https://api.example.com/"))
        val url = client.http.buildUrl(
            "/api/v1/apps/demo/collections/tasks/records",
            mapOf("filter" to "status = 'open'", "limit" to 10)
        )
        assertTrue(url.contains("https://api.example.com/api/v1/apps/demo/collections/tasks/records"))
        assertTrue(url.contains("filter=status+%3D+%27open%27") || url.contains("filter=status%20%3D%20%27open%27"))
        assertTrue(url.contains("limit=10"))
    }

    @Test
    fun testRfc9457ProblemException() = runTest {
        val engine = MockEngine { request ->
            respond(
                content = """
                    {
                        "type": "https://monolith.dev/errors/validation",
                        "title": "Validation Failed",
                        "status": 422,
                        "detail": "Invalid title and status",
                        "requestId": "req_12345",
                        "errors": [
                            {"field": "title", "code": "required", "message": "Title is required"}
                        ]
                    }
                """.trimIndent(),
                status = HttpStatusCode.UnprocessableEntity,
                headers = headersOf(
                    HttpHeaders.ContentType to listOf("application/problem+json"),
                    "x-request-id" to listOf("req_12345")
                )
            )
        }

        val client = createMockClient(engine)
        val col = client.collection<TestTask>("tasks")

        val exception = assertFailsWith<MonolithException> {
            col.create(TestTask(title = ""))
        }

        assertEquals(422, exception.status)
        assertEquals("https://monolith.dev/errors/validation", exception.type)
        assertEquals("Invalid title and status", exception.detail)
        assertEquals("req_12345", exception.requestId)
        assertEquals("Title is required", exception.getFieldError("title"))
    }

    @Test
    fun testAuthServiceSignInAndTokenListener() = runTest {
        val engine = MockEngine { request ->
            when (request.url.encodedPath) {
                "/api/v1/apps/testapp/auth/login" -> {
                    respond(
                        content = """{"token": "jwt_token_abc123", "user": {"id": "usr_1"}}""",
                        status = HttpStatusCode.OK,
                        headers = headersOf(HttpHeaders.ContentType to listOf("application/json"))
                    )
                }
                else -> respond("Not found", HttpStatusCode.NotFound)
            }
        }

        val client = createMockClient(engine)
        var receivedToken: String? = null
        val unsubscribe = client.auth().onChange { token ->
            receivedToken = token
        }

        val session = client.auth().signInWithPassword("test@example.com", "secret123")
        assertEquals("jwt_token_abc123", session.token)
        assertEquals("jwt_token_abc123", client.auth().token)
        assertEquals("jwt_token_abc123", receivedToken)

        client.auth().signOut()
        assertNull(client.auth().token)
        assertNull(receivedToken)

        unsubscribe()
    }

    @Test
    fun testCollectionTypedCrud() = runTest {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            when {
                path == "/api/v1/apps/testapp/collections/tasks/records" && request.method.value == "GET" -> {
                    respond(
                        content = """
                            {
                                "items": [
                                    {"id": "t1", "title": "Buy milk", "status": "open"},
                                    {"id": "t2", "title": "Clean desk", "status": "done"}
                                ],
                                "total": 2
                            }
                        """.trimIndent(),
                        status = HttpStatusCode.OK,
                        headers = headersOf(HttpHeaders.ContentType to listOf("application/json"))
                    )
                }
                path == "/api/v1/apps/testapp/collections/tasks/records/t1" && request.method.value == "GET" -> {
                    respond(
                        content = """{"id": "t1", "title": "Buy milk", "status": "open"}""",
                        status = HttpStatusCode.OK,
                        headers = headersOf(HttpHeaders.ContentType to listOf("application/json"))
                    )
                }
                path == "/api/v1/apps/testapp/collections/tasks/records" && request.method.value == "POST" -> {
                    respond(
                        content = """{"id": "t3", "title": "Walk dog", "status": "open"}""",
                        status = HttpStatusCode.Created,
                        headers = headersOf(HttpHeaders.ContentType to listOf("application/json"))
                    )
                }
                path == "/api/v1/apps/testapp/collections/tasks/records/t1" && request.method.value == "PATCH" -> {
                    respond(
                        content = """{"id": "t1", "title": "Buy oat milk", "status": "open"}""",
                        status = HttpStatusCode.OK,
                        headers = headersOf(HttpHeaders.ContentType to listOf("application/json"))
                    )
                }
                path == "/api/v1/apps/testapp/collections/tasks/records/t1" && request.method.value == "DELETE" -> {
                    respond("", HttpStatusCode.NoContent)
                }
                path == "/api/v1/apps/testapp/collections/tasks/records:count" -> {
                    respond(
                        content = """{"count": 42}""",
                        status = HttpStatusCode.OK,
                        headers = headersOf(HttpHeaders.ContentType to listOf("application/json"))
                    )
                }
                else -> respond("Unknown path: $path", HttpStatusCode.NotFound)
            }
        }

        val client = createMockClient(engine)
        val col = client.collection<TestTask>("tasks")

        // List
        val list = col.list(ListOptions(limit = 10))
        assertEquals(2, list.items.size)
        assertEquals("Buy milk", list.items[0].title)

        // GetOne
        val one = col.getOne("t1")
        assertEquals("t1", one.id)
        assertEquals("Buy milk", one.title)

        // Create
        val created = col.create(TestTask(title = "Walk dog"))
        assertEquals("t3", created.id)
        assertEquals("Walk dog", created.title)

        // Update
        val updated = col.update("t1", TestTask(title = "Buy oat milk"))
        assertEquals("Buy oat milk", updated.title)

        // Delete
        val deleted = col.delete("t1")
        assertTrue(deleted)

        // Count
        val count = col.count("status = 'open'")
        assertEquals(42L, count)
    }

    @Test
    fun testFilesService() = runTest {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            when {
                path == "/api/v1/apps/testapp/buckets/receipts/objects/file1.png" && request.method.value == "PUT" -> {
                    respond(
                        content = """
                            {
                                "bucket": "receipts",
                                "key": "file1.png",
                                "size": 1024,
                                "contentType": "image/png",
                                "blake3": "hash123"
                            }
                        """.trimIndent(),
                        status = HttpStatusCode.OK,
                        headers = headersOf(HttpHeaders.ContentType to listOf("application/json"))
                    )
                }
                path == "/api/v1/apps/testapp/buckets/receipts/objects/file1.png" && request.method.value == "DELETE" -> {
                    respond("", HttpStatusCode.NoContent)
                }
                path == "/api/v1/apps/testapp/buckets/receipts/objects" && request.method.value == "GET" -> {
                    respond(
                        content = """
                            {
                                "items": [
                                    {"bucket": "receipts", "key": "file1.png", "size": 1024}
                                ]
                            }
                        """.trimIndent(),
                        status = HttpStatusCode.OK,
                        headers = headersOf(HttpHeaders.ContentType to listOf("application/json"))
                    )
                }
                else -> respond("Unknown path: $path", HttpStatusCode.NotFound)
            }
        }

        val client = createMockClient(engine)
        val files = client.files

        val url = files.getURL("receipts", "file1.png", thumb = "100x100")
        assertTrue(url.contains("/api/v1/apps/testapp/buckets/receipts/objects/file1.png"))
        assertTrue(url.contains("thumb=100x100"))

        val uploaded = files.upload("receipts", "file1.png", "fake data", "image/png")
        assertEquals("receipts", uploaded.bucket)
        assertEquals("file1.png", uploaded.key)
        assertEquals(1024L, uploaded.size)

        val deleted = files.delete("receipts", "file1.png")
        assertTrue(deleted)

        val list = files.list("receipts")
        assertEquals(1, list.items.size)
        assertEquals("file1.png", list.items[0].key)
    }

    @Test
    fun testAnalyticsServiceFlush() = runTest {
        var eventCount = 0
        val engine = MockEngine { request ->
            if (request.url.encodedPath == "/api/v1/apps/testapp/analytics/events") {
                eventCount++
                respond("", HttpStatusCode.NoContent)
            } else {
                respond("Not found", HttpStatusCode.NotFound)
            }
        }

        val client = createMockClient(engine)
        client.analytics.track("app_opened", buildJsonObject { put("screen", "home") })
        client.analytics.flush()

        assertEquals(1, eventCount)
    }

    @Test
    fun testBatchOperations() = runTest {
        val engine = MockEngine { request ->
            if (request.url.encodedPath == "/api/v1/apps/testapp/batch") {
                respond(
                    content = """
                        [
                            {"status": 201, "body": {"id": "t1"}},
                            {"status": 200, "body": {"id": "t2"}}
                        ]
                    """.trimIndent(),
                    status = HttpStatusCode.OK,
                    headers = headersOf(HttpHeaders.ContentType to listOf("application/json"))
                )
            } else {
                respond("Not found", HttpStatusCode.NotFound)
            }
        }

        val client = createMockClient(engine)
        val responses = client.batch(
            listOf(
                BatchOperation(method = "POST", path = "/collections/tasks/records"),
                BatchOperation(method = "PATCH", path = "/collections/tasks/records/t2")
            )
        )

        assertEquals(2, responses.size)
        assertEquals(201, responses[0].status)
        assertEquals(200, responses[1].status)
    }

    @Test
    fun testRealtimeSubscription() = runTest {
        val ssePayload = "event: record\ndata: {\"op\":\"create\",\"collection\":\"tasks\",\"record\":{\"id\":\"t99\",\"title\":\"Live test\",\"status\":\"open\"}}\n\n"
        val engine = MockEngine { request ->
            if (request.url.encodedPath == "/api/v1/apps/testapp/realtime") {
                respond(
                    content = ssePayload,
                    status = HttpStatusCode.OK,
                    headers = headersOf(HttpHeaders.ContentType to listOf("text/event-stream"))
                )
            } else {
                respond("Not found", HttpStatusCode.NotFound)
            }
        }

        val client = createMockClient(engine)
        var receivedEvent: dev.monolith.client.realtime.RealtimeEvent<TestTask>? = null
        val unsubscribe = client.collection<TestTask>("tasks").subscribe({ ev ->
            receivedEvent = ev
        })

        // Give the background thread a moment to connect and process the stream
        var attempts = 0
        while (receivedEvent == null && attempts < 50) {
            Thread.sleep(20)
            attempts++
        }

        unsubscribe()
        client.close()

        assertNotNull(receivedEvent)
        assertEquals("create", receivedEvent?.action)
        assertEquals("tasks", receivedEvent?.collection)
        assertEquals("t99", receivedEvent?.record?.id)
        assertEquals("Live test", receivedEvent?.record?.title)
    }
}
