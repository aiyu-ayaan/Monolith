package dev.monolith.client

import dev.monolith.client.realtime.DocumentChangeType
import dev.monolith.client.realtime.QuerySnapshot
import dev.monolith.client.realtime.RealtimeEvent
import dev.monolith.client.realtime.SnapshotOptions
import dev.monolith.client.realtime.sortComparator
import io.ktor.client.HttpClient
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.plugins.contentnegotiation.ContentNegotiation
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.Url
import io.ktor.http.headersOf
import io.ktor.serialization.kotlinx.json.json
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlin.test.*

class RealtimeTest {

    /** Serves each realtime call one SSE body (then empty bodies) and records the request URLs. */
    private class Sse(bodies: List<String>) {
        private val queue = bodies.toMutableList()
        val urls = mutableListOf<Url>()
        val engine = MockEngine { request ->
            urls += request.url
            val body = if (queue.isEmpty()) "" else queue.removeAt(0)
            respond(
                content = body,
                status = HttpStatusCode.OK,
                headers = headersOf(HttpHeaders.ContentType to listOf("text/event-stream"))
            )
        }
    }

    private fun client(engine: MockEngine) = MonolithClient(
        ClientOptions(
            url = "https://api.example.com",
            app = "testapp",
            key = "mk_test_key",
            httpClient = HttpClient(engine) {
                install(ContentNegotiation) { json(Json { ignoreUnknownKeys = true; isLenient = true }) }
            }
        )
    )

    private fun rec(seq: Int, op: String, record: String) =
        "id: $seq\nevent: record\ndata: {\"op\":\"$op\",\"collection\":\"tasks\",\"seq\":$seq,\"record\":$record}\n\n"

    private fun task(id: String, title: String) = "{\"id\":\"$id\",\"title\":\"$title\",\"status\":\"open\"}"

    private fun waitFor(timeoutMs: Long = 5000, cond: () -> Boolean) {
        var waited = 0L
        while (!cond() && waited < timeoutMs) {
            Thread.sleep(20)
            waited += 20
        }
    }

    @Test
    fun testOpMapsToActionAndFilterIdsUseDedicatedTopics() = runTest {
        val sse = Sse(listOf(rec(5, "leave", task("a", "x"))))
        val client = client(sse.engine)
        val got = mutableListOf<RealtimeEvent<TestTask>>()
        val off = client.collection<TestTask>("tasks").subscribe({ got += it }, "done = false", listOf("a", "b"))
        waitFor { got.isNotEmpty() }
        off()
        client.close()

        assertEquals(1, got.size)
        assertEquals("leave", got[0].action)
        assertEquals(5L, got[0].seq)
        assertEquals("tasks", got[0].collection)
        assertEquals("a", got[0].record.id)
        val q = sse.urls[0].parameters
        assertEquals("tasks", q["collections"])
        assertEquals("done = false", q["filter.tasks"])
        assertEquals("a,b", q["ids.tasks"])
        assertNull(q["since"])
    }

    @Test
    fun testPlainSubscriptionResumesWithSince() = runTest {
        val sse = Sse(listOf(rec(9, "create", task("z", "z"))))
        val client = client(sse.engine)
        val got = mutableListOf<RealtimeEvent<TestTask>>()
        val off = client.collection<TestTask>("tasks").subscribe({ got += it })
        // The first body ends right away: the SDK must reconnect with since=9.
        waitFor { sse.urls.size > 1 }
        off()
        client.close()

        assertEquals("create", got[0].action)
        assertTrue(sse.urls.size > 1)
        assertNull(sse.urls[0].parameters["since"])
        assertEquals("tasks", sse.urls[0].parameters["collections"])
        assertEquals("9", sse.urls[1].parameters["since"])
    }

    @Test
    fun testResyncResetsSince() = runTest {
        val sse = Sse(
            listOf(
                rec(9, "create", task("z", "z")) + "event: resync\ndata: {\"seq\":50}\n\n",
                rec(2, "create", task("y", "y"))
            )
        )
        val client = client(sse.engine)
        val got = mutableListOf<RealtimeEvent<TestTask>>()
        val off = client.collection<TestTask>("tasks").subscribe({ got += it })
        waitFor { got.size >= 2 }
        off()
        client.close()

        assertEquals(2, got.size)
        // resync reconnects at once, without since
        assertNull(sse.urls[1].parameters["since"])
    }

    @Test
    fun testOnSnapshotMergesSnapshotUpdatesLeaveAndDelete() = runTest {
        val snap = "event: snapshot\ndata: {\"collection\":\"tasks\",\"seq\":3,\"items\":[" +
            "${task("a", "2")},${task("b", "1")}],\"next\":null}\n\n"
        val sse = Sse(
            listOf(
                snap + rec(4, "create", task("c", "0")) + rec(5, "leave", task("a", "2")) +
                    rec(6, "delete", task("b", "1")) + "event: error\ndata: {\"message\":\"boom\"}\n\n"
            )
        )
        val client = client(sse.engine)
        val snaps = mutableListOf<QuerySnapshot<TestTask>>()
        var error: Throwable? = null
        val off = client.collection<TestTask>("tasks").onSnapshot(
            { snaps += it },
            SnapshotOptions(filter = "status = 'open'", sort = "title", limit = 5, onError = { error = it })
        )
        waitFor { snaps.size >= 4 && error != null }
        off()
        client.close()

        assertEquals(
            listOf(listOf("b", "a"), listOf("c", "b", "a"), listOf("c", "b"), listOf("c")),
            snaps.take(4).map { s -> s.docs.map { it.id } }
        )
        assertEquals(listOf(DocumentChangeType.ADDED, DocumentChangeType.ADDED), snaps[0].changes.map { it.type })
        assertEquals(listOf(DocumentChangeType.ADDED), snaps[1].changes.map { it.type })
        assertEquals("c", snaps[1].changes[0].doc.id)
        assertEquals(DocumentChangeType.REMOVED, snaps[2].changes[0].type)
        assertEquals("boom", error?.message)
        val q = sse.urls[0].parameters
        assertEquals("5", q["snapshot"])
        assertEquals("title", q["sort.tasks"])
        assertEquals("status = 'open'", q["filter.tasks"])
        assertEquals("tasks", q["collections"])
    }

    @Test
    fun testSnapshotStreamResnapshotsOnReconnectInsteadOfSince() = runTest {
        val first = "event: snapshot\ndata: {\"collection\":\"tasks\",\"seq\":3,\"items\":[${task("a", "1")},${task("b", "2")}]}\n\n"
        val second = "event: snapshot\ndata: {\"collection\":\"tasks\",\"seq\":8,\"items\":[${task("b", "3")},${task("c", "4")}]}\n\n"
        val sse = Sse(listOf(first, second))
        val client = client(sse.engine)
        val snaps = mutableListOf<QuerySnapshot<TestTask>>()
        val off = client.collection<TestTask>("tasks").onSnapshot({ snaps += it })
        waitFor { snaps.size >= 2 }
        off()
        client.close()

        assertNull(sse.urls[1].parameters["since"])
        assertEquals("100", sse.urls[1].parameters["snapshot"])
        assertEquals(listOf("b", "c"), snaps[1].docs.map { it.id })
        val byType = snaps[1].changes.associate { it.doc.id to it.type }
        assertEquals(
            mapOf(
                "b" to DocumentChangeType.MODIFIED,
                "c" to DocumentChangeType.ADDED,
                "a" to DocumentChangeType.REMOVED
            ),
            byType
        )
    }

    @Test
    fun testOnDocSnapshotUsesIdFilterAndLimitOne() = runTest {
        val snap = "event: snapshot\ndata: {\"collection\":\"tasks\",\"seq\":1,\"items\":[${task("t1", "x")}]}\n\n"
        val sse = Sse(listOf(snap + rec(2, "delete", task("t1", "x"))))
        val client = client(sse.engine)
        val docs = mutableListOf<TestTask?>()
        val off = client.collection<TestTask>("tasks").onDocSnapshot("t1", { docs += it })
        waitFor { docs.size >= 2 }
        off()
        client.close()

        assertEquals(listOf("t1", null), docs.map { it?.id })
        val q = sse.urls[0].parameters
        assertEquals("id = 't1'", q["filter.tasks"])
        assertEquals("1", q["snapshot"])
    }

    @Test
    fun testSortComparator() {
        fun o(created: Long?, title: String) = JsonObject(
            mapOf(
                "created" to (created?.let { JsonPrimitive(it) } ?: JsonNull),
                "title" to JsonPrimitive(title)
            )
        )
        val items = listOf(o(1, "b"), o(2, "a"), o(2, "c"), o(null, "z"))
        val sorted = items.sortedWith(sortComparator("-created,title")!!)
        assertEquals(listOf("a", "c", "b", "z"), sorted.map { (it["title"] as JsonPrimitive).content })
        assertNull(sortComparator(null))
        assertEquals(10L, (listOf(o(10, "x"), o(9, "y")).sortedWith(sortComparator("created")!!).last()["created"] as JsonPrimitive).content.toLong())
    }
}
