package dev.monolith.client.realtime

import dev.monolith.client.http.HttpClient
import dev.monolith.client.http.RecordModel
import io.ktor.client.request.*
import io.ktor.client.statement.*
import io.ktor.http.*
import io.ktor.utils.io.*
import kotlinx.coroutines.*
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.KSerializer
import kotlinx.serialization.json.*
import kotlin.math.min
import kotlin.math.pow
import kotlin.random.Random

/**
 * Event representing a realtime database mutation.
 *
 * [action] is the server `op`: `create`, `update`, `delete` or `leave`. `leave` means the
 * record stopped matching the subscription's filter or rule (it was not deleted).
 * [seq] is the change-log sequence number, the resume point after a reconnect.
 */
data class RealtimeEvent<T>(
    val action: String,
    val record: T,
    val collection: String,
    val seq: Long? = null
)

typealias RealtimeListener<T> = (RealtimeEvent<T>) -> Unit

/** Kind of change between two consecutive [QuerySnapshot]s. */
enum class DocumentChangeType { ADDED, MODIFIED, REMOVED }

data class DocumentChange<T>(val type: DocumentChangeType, val doc: T)

/**
 * Result set of an [RealtimeService.onSnapshot] query.
 * [docs] is the current result in order; [changes] is what changed since the previous
 * snapshot (everything is ADDED in the first one).
 */
data class QuerySnapshot<T>(
    val docs: List<T>,
    val changes: List<DocumentChange<T>>
)

/**
 * Options for [RealtimeService.onSnapshot]. [sort] uses the list syntax (`-created,title`).
 * [onError] receives server `error` events.
 */
data class SnapshotOptions(
    val filter: String? = null,
    val sort: String? = null,
    val limit: Int = 100,
    val onError: ((Throwable) -> Unit)? = null
)

private fun <T> decodeRecord(json: Json, serializer: KSerializer<T>?, element: JsonElement): T =
    if (serializer != null) {
        json.decodeFromJsonElement(serializer, element)
    } else {
        @Suppress("UNCHECKED_CAST")
        (json.decodeFromJsonElement(RecordModel.serializer(), element) as T)
    }

private class SubscriptionEntry<T>(
    val listener: RealtimeListener<T>,
    val serializer: KSerializer<T>?
) {
    fun dispatch(payload: JsonObject, json: Json) {
        try {
            val record = payload["record"] ?: return
            listener(
                RealtimeEvent(
                    action = payload["op"]?.jsonPrimitive?.contentOrNull ?: "update",
                    record = decodeRecord(json, serializer, record),
                    collection = payload["collection"]?.jsonPrimitive?.contentOrNull ?: "",
                    seq = payload["seq"]?.jsonPrimitive?.longOrNull
                )
            )
        } catch (_: Throwable) {
            // A bad payload or a throwing subscriber must not break the stream
        }
    }
}

/** Client-side comparator for `-created,title` style sort strings. */
internal fun sortComparator(sort: String?): Comparator<JsonObject>? {
    val keys = sort?.split(",")?.map { it.trim() }?.filter { it.isNotEmpty() }?.map {
        if (it.startsWith("-")) it.substring(1) to -1 else it.removePrefix("+") to 1
    } ?: return null
    if (keys.isEmpty()) return null
    return Comparator { a, b ->
        for ((field, dir) in keys) {
            val x = a[field]?.takeUnless { it is JsonNull } as? JsonPrimitive
            val y = b[field]?.takeUnless { it is JsonNull } as? JsonPrimitive
            val c = when {
                x == null && y == null -> 0
                x == null -> -1
                y == null -> 1
                else -> {
                    val xn = if (x.isString) null else x.doubleOrNull
                    val yn = if (y.isString) null else y.doubleOrNull
                    if (xn != null && yn != null) xn.compareTo(yn) else x.content.compareTo(y.content)
                }
            }
            if (c != 0) return@Comparator c * dir
        }
        0
    }
}

/**
 * One SSE connection with reconnect, backoff and resume (`since`, or a fresh snapshot).
 * Commands are applied in call order (the lock is fair and taken before the launch suspends).
 */
internal class SseStream(
    private val http: HttpClient,
    private val scope: CoroutineScope,
    /** A snapshot stream re-snapshots on reconnect instead of replaying. */
    private val snapshot: Boolean,
    private val params: suspend () -> Map<String, String>,
    private val dispatch: suspend (event: String, payload: JsonObject) -> Unit
) {
    private val lock = Mutex()
    private var job: Job? = null
    private var stopped = true
    private var lastSeq = 0L
    private var attempts = 0

    var connected = false
        private set

    fun start() = command {
        stopped = false
        spawn()
    }

    /** Reopens now (the collection list changed). Keeps `lastSeq` so nothing is missed. */
    fun restart() = command { if (!stopped) spawn() }

    fun stop() = command {
        stopped = true
        job?.cancel()
        job = null
        attempts = 0
        connected = false
    }

    private fun command(block: () -> Unit) {
        scope.launch(start = CoroutineStart.UNDISPATCHED) { lock.withLock { block() } }
    }

    private fun spawn() {
        val old = job
        old?.cancel()
        job = scope.launch {
            old?.join()
            connectLoop()
        }
    }

    private suspend fun connectLoop() {
        while (currentCoroutineContext().isActive) {
            var resync = false
            try {
                http.authHooks?.beforeRequest()
                val query = mutableMapOf<String, Any?>().apply { putAll(params()) }
                if (!snapshot && lastSeq > 0) query["since"] = lastSeq
                val url = http.buildUrl("/api/v1/apps/${http.app}/realtime", query)

                http.ktorClient.prepareGet(url) {
                    header(HttpHeaders.Accept, "text/event-stream")
                    header(HttpHeaders.CacheControl, "no-cache")
                    val authToken = http.getToken() ?: http.getApiKey()
                    if (authToken != null) {
                        header(HttpHeaders.Authorization, "Bearer $authToken")
                    }
                }.execute { response ->
                    if (!response.status.isSuccess()) {
                        throw Exception("Realtime connection failed with HTTP ${response.status}")
                    }
                    connected = true
                    attempts = 0

                    val channel = response.bodyAsChannel()
                    var currentEvent = "message"
                    val dataLines = mutableListOf<String>()

                    while (!channel.isClosedForRead && currentCoroutineContext().isActive) {
                        val trimmed = (channel.readUTF8Line() ?: break).trim()
                        if (trimmed.isEmpty()) {
                            // Empty line terminates an event block
                            if (dataLines.isNotEmpty()) {
                                val data = dataLines.joinToString("\n")
                                dataLines.clear()
                                if (handle(currentEvent, data)) {
                                    resync = true
                                    break
                                }
                            }
                            currentEvent = "message"
                        } else if (trimmed.startsWith(":")) {
                            // Ping / comment line, ignore
                        } else if (trimmed.startsWith("event:")) {
                            currentEvent = trimmed.removePrefix("event:").trim()
                        } else if (trimmed.startsWith("data:")) {
                            dataLines.add(trimmed.removePrefix("data:").trim())
                        }
                    }
                }
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (_: Throwable) {
                // Connection lost or failed
            } finally {
                connected = false
            }

            if (!currentCoroutineContext().isActive) break
            if (resync) continue // reconnect from scratch, no backoff

            val delayMs = min(500L * 2.0.pow(attempts).toLong() + Random.nextLong(200L), 15000L)
            attempts++
            delay(delayMs)
        }
    }

    /** Returns true when the server asked for a resync (the caller must reconnect). */
    private suspend fun handle(event: String, data: String): Boolean {
        val payload = try {
            http.json.parseToJsonElement(data) as? JsonObject
        } catch (_: Throwable) {
            null
        } ?: return false

        payload["seq"]?.jsonPrimitive?.longOrNull?.let { if (it > lastSeq) lastSeq = it }
        if (event == "resync") {
            // Missed more than the server keeps: start over.
            lastSeq = 0
            try {
                dispatch(event, payload)
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (_: Throwable) {
            }
            return true
        }
        try {
            dispatch(event, payload)
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (_: Throwable) {
            // A listener must not break the stream
        }
        return false
    }
}

/**
 * Server-Sent Events (SSE) realtime service for subscribing to live record updates.
 *
 * Unfiltered subscriptions share one connection. A `filter`, `ids` or snapshot query opens a
 * dedicated connection. After a reconnect the stream resumes from the last seen change.
 */
class RealtimeService(
    private val http: HttpClient,
    private val scope: CoroutineScope = CoroutineScope(Dispatchers.Default + SupervisorJob())
) {
    private val mutex = Mutex()
    private val listeners = mutableMapOf<String, MutableSet<SubscriptionEntry<*>>>()
    private var shared: SseStream? = null
    private var dedicated: List<SseStream> = emptyList()

    fun isConnected(): Boolean = shared?.connected == true || dedicated.any { it.connected }

    /**
     * Subscribes a listener to change events in the given [collection].
     * A non-empty [filter] or [ids] opens a dedicated connection with server-side filtering.
     * Returns an unsubscribe function.
     */
    fun <T> subscribe(
        collection: String,
        listener: RealtimeListener<T>,
        filter: String? = null,
        ids: List<String>? = null,
        serializer: KSerializer<T>? = null
    ): () -> Unit {
        val entry = SubscriptionEntry(listener, serializer)

        if (!filter.isNullOrEmpty() || !ids.isNullOrEmpty()) {
            val stream = SseStream(
                http, scope, snapshot = false,
                params = {
                    buildMap {
                        put("collections", collection)
                        if (!filter.isNullOrEmpty()) put("filter.$collection", filter)
                        if (!ids.isNullOrEmpty()) put("ids.$collection", ids.joinToString(","))
                    }
                },
                dispatch = { ev, p -> if (ev == "record") entry.dispatch(p, http.json) }
            )
            return startDedicated(stream)
        }

        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            mutex.withLock {
                val fresh = !listeners.containsKey(collection)
                listeners.getOrPut(collection) { mutableSetOf() }.add(entry)
                val current = shared
                if (current == null) {
                    shared = newSharedStream().also { it.start() }
                } else if (fresh) {
                    current.restart()
                }
            }
        }

        return {
            scope.launch(start = CoroutineStart.UNDISPATCHED) {
                mutex.withLock {
                    val set = listeners[collection]
                    set?.remove(entry)
                    if (set != null && set.isEmpty()) listeners.remove(collection)
                    if (listeners.isEmpty()) {
                        shared?.stop()
                        shared = null
                    } else if (set != null && set.isEmpty()) {
                        shared?.restart()
                    }
                }
            }
        }
    }

    private fun newSharedStream() = SseStream(
        http, scope, snapshot = false,
        params = { mutex.withLock { mapOf("collections" to listeners.keys.joinToString(",")) } },
        dispatch = { ev, p ->
            if (ev == "record") {
                val c = p["collection"]?.jsonPrimitive?.contentOrNull
                val targets = mutex.withLock { listeners[c]?.toList() }.orEmpty()
                for (t in targets) t.dispatch(p, http.json)
            }
        }
    )

    private fun startDedicated(stream: SseStream): () -> Unit {
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            mutex.withLock { dedicated = dedicated + stream }
            stream.start()
        }
        return {
            scope.launch(start = CoroutineStart.UNDISPATCHED) {
                mutex.withLock { dedicated = dedicated - stream }
                stream.stop()
            }
        }
    }

    /**
     * Subscribes to change events in [collection] as a Kotlin [Flow].
     */
    fun <T> subscribeFlow(
        collection: String,
        filter: String? = null,
        ids: List<String>? = null,
        serializer: KSerializer<T>? = null
    ): Flow<RealtimeEvent<T>> = callbackFlow {
        val unsubscribe = subscribe(collection, { trySend(it) }, filter, ids, serializer)
        awaitClose { unsubscribe() }
    }

    /**
     * Firestore-style query listener: calls back with the current result set, then again after
     * every change that affects it. Reconnects re-snapshot, so the view always converges.
     * Returns an unsubscribe function.
     */
    fun <T> onSnapshot(
        collection: String,
        callback: (QuerySnapshot<T>) -> Unit,
        options: SnapshotOptions = SnapshotOptions(),
        serializer: KSerializer<T>? = null
    ): () -> Unit {
        val docs = LinkedHashMap<String, JsonObject>()
        val cmp = sortComparator(options.sort)
        val json = http.json

        fun ordered(): List<JsonObject> = docs.values.toList().let { if (cmp != null) it.sortedWith(cmp) else it }
        fun idOf(o: JsonObject) = o["id"]?.jsonPrimitive?.contentOrNull

        val stream = SseStream(
            http, scope, snapshot = true,
            params = {
                buildMap {
                    put("collections", collection)
                    put("snapshot", options.limit.toString())
                    if (!options.filter.isNullOrEmpty()) put("filter.$collection", options.filter)
                    if (!options.sort.isNullOrEmpty()) put("sort.$collection", options.sort)
                }
            },
            dispatch = dispatch@{ ev, p ->
                val changes = mutableListOf<Pair<DocumentChangeType, JsonObject>>()
                when (ev) {
                    "snapshot" -> {
                        val next = LinkedHashMap<String, JsonObject>()
                        (p["items"] as? JsonArray)?.forEach { el ->
                            val o = el as? JsonObject ?: return@forEach
                            idOf(o)?.let { next[it] = o }
                        }
                        for ((id, d) in next) {
                            changes += (if (docs.containsKey(id)) DocumentChangeType.MODIFIED else DocumentChangeType.ADDED) to d
                        }
                        for ((id, d) in docs) if (!next.containsKey(id)) changes += DocumentChangeType.REMOVED to d
                        docs.clear()
                        docs.putAll(next)
                    }
                    "record" -> {
                        val rec = p["record"] as? JsonObject ?: return@dispatch
                        val id = idOf(rec) ?: return@dispatch
                        val op = p["op"]?.jsonPrimitive?.contentOrNull
                        if (op == "delete" || op == "leave") {
                            val old = docs.remove(id) ?: return@dispatch
                            changes += DocumentChangeType.REMOVED to old
                        } else {
                            changes += (if (docs.containsKey(id)) DocumentChangeType.MODIFIED else DocumentChangeType.ADDED) to rec
                            docs[id] = rec
                        }
                    }
                    "error" -> {
                        val msg = p["message"]?.jsonPrimitive?.contentOrNull ?: "realtime error"
                        try {
                            options.onError?.invoke(Exception(msg))
                        } catch (_: Throwable) {
                        }
                        return@dispatch
                    }
                    else -> return@dispatch
                }
                try {
                    callback(
                        QuerySnapshot(
                            docs = ordered().map { decodeRecord(json, serializer, it) },
                            changes = changes.map { (t, d) -> DocumentChange(t, decodeRecord(json, serializer, d)) }
                        )
                    )
                } catch (_: Throwable) {
                    // A listener must not break the stream
                }
            }
        )
        return startDedicated(stream)
    }

    /**
     * [onSnapshot] as a Kotlin [Flow]. Server `error` events go to [SnapshotOptions.onError].
     */
    fun <T> onSnapshotFlow(
        collection: String,
        options: SnapshotOptions = SnapshotOptions(),
        serializer: KSerializer<T>? = null
    ): Flow<QuerySnapshot<T>> = callbackFlow {
        val off = onSnapshot<T>(collection, { trySend(it) }, options, serializer)
        awaitClose { off() }
    }

    suspend fun disconnect() {
        mutex.withLock {
            shared?.stop()
            shared = null
            listeners.clear()
            dedicated.forEach { it.stop() }
            dedicated = emptyList()
        }
    }
}
