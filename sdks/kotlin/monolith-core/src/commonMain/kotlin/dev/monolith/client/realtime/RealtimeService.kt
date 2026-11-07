package dev.monolith.client.realtime

import dev.monolith.client.http.HttpClient
import dev.monolith.client.http.RecordModel
import io.ktor.client.plugins.HttpTimeout
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
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.*
import kotlinx.serialization.serializer
import kotlin.math.min
import kotlin.math.pow
import kotlin.random.Random

/**
 * Event representing a realtime database mutation.
 */
data class RealtimeEvent<T>(
    val action: String,
    val record: T,
    val collection: String
)

typealias RealtimeListener<T> = (RealtimeEvent<T>) -> Unit

private interface SubscriptionEntry {
    val filter: String?
    fun dispatch(event: RawRealtimePayload, json: Json)
}

private class TypedSubscriptionEntry<T>(
    override val filter: String?,
    val listener: RealtimeListener<T>,
    val serializer: KSerializer<T>?
) : SubscriptionEntry {
    override fun dispatch(event: RawRealtimePayload, json: Json) {
        val parsedRecord: T = if (serializer != null) {
            json.decodeFromJsonElement(serializer, event.record)
        } else {
            @Suppress("UNCHECKED_CAST")
            (json.decodeFromJsonElement(RecordModel.serializer(), event.record) as T)
        }

        val typedEvent = RealtimeEvent(
            action = event.op,
            record = parsedRecord,
            collection = event.collection
        )

        try {
            listener(typedEvent)
        } catch (_: Throwable) {
            // Ignore subscriber callback exceptions
        }
    }
}

@Serializable
internal data class RawRealtimePayload(
    val op: String = "update",
    val collection: String = "",
    val seq: Long = 0,
    val record: JsonElement = JsonObject(emptyMap())
)

/**
 * Multiplexed Server-Sent Events (SSE) realtime service for subscribing to
 * live record updates across collections.
 */
class RealtimeService(
    private val http: HttpClient,
    private val scope: CoroutineScope = CoroutineScope(Dispatchers.Default + SupervisorJob())
) {
    private val subscriptions = mutableMapOf<String, MutableSet<SubscriptionEntry>>()
    private val mutex = Mutex()
    private var streamJob: Job? = null
    private var reconnectAttempts = 0
    private var isConnected = false

    fun isConnected(): Boolean = isConnected

    /**
     * Subscribes a listener to change events in the given [collection].
     * Returns an unsubscribe function.
     */
    fun <T> subscribe(
        collection: String,
        listener: RealtimeListener<T>,
        filter: String? = null,
        serializer: KSerializer<T>? = null
    ): () -> Unit {
        val entry = TypedSubscriptionEntry(filter, listener, serializer)
        scope.launch {
            mutex.withLock {
                val set = subscriptions.getOrPut(collection) { mutableSetOf() }
                set.add(entry)
                reconnectLocked()
            }
        }

        return {
            scope.launch {
                mutex.withLock {
                    val set = subscriptions[collection]
                    if (set != null) {
                        set.remove(entry)
                        if (set.isEmpty()) {
                            subscriptions.remove(collection)
                        }
                    }
                    if (subscriptions.isEmpty()) {
                        disconnectLocked()
                    } else {
                        reconnectLocked()
                    }
                }
            }
        }
    }

    /**
     * Subscribes to change events in [collection] as a Kotlin [Flow].
     */
    fun <T> subscribeFlow(
        collection: String,
        filter: String? = null,
        serializer: KSerializer<T>? = null
    ): Flow<RealtimeEvent<T>> = callbackFlow {
        val unsubscribe = subscribe(collection, { trySend(it) }, filter, serializer)
        awaitClose { unsubscribe() }
    }

    suspend fun disconnect() {
        mutex.withLock {
            disconnectLocked()
        }
    }

    private fun disconnectLocked() {
        streamJob?.cancel()
        streamJob = null
        isConnected = false
        reconnectAttempts = 0
    }

    private fun reconnectLocked() {
        if (subscriptions.isEmpty()) {
            disconnectLocked()
            return
        }

        streamJob?.cancel()
        streamJob = scope.launch {
            connectLoop()
        }
    }

    private suspend fun connectLoop() {
        while (currentCoroutineContext().isActive) {
            val collectionsParam = mutex.withLock {
                if (subscriptions.isEmpty()) return
                subscriptions.keys.joinToString(",")
            }

            try {
                val path = "/api/v1/apps/${http.app}/realtime"
                val url = http.buildUrl(path, mapOf("collections" to collectionsParam))

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

                    isConnected = true
                    reconnectAttempts = 0

                    val channel = response.bodyAsChannel()
                    var currentEvent = "message"
                    val dataLines = mutableListOf<String>()

                    while (!channel.isClosedForRead && currentCoroutineContext().isActive) {
                        val line = channel.readUTF8Line() ?: break
                        val trimmed = line.trim()

                        if (trimmed.isEmpty()) {
                            // Empty line terminates an event block
                            if (dataLines.isNotEmpty()) {
                                val fullData = dataLines.joinToString("\n")
                                dataLines.clear()
                                dispatchEvent(currentEvent, fullData)
                                currentEvent = "message"
                            }
                            continue
                        }

                        if (trimmed.startsWith(":")) {
                            // Ping / comment line, ignore
                            continue
                        }

                        if (trimmed.startsWith("event:")) {
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
                isConnected = false
            }

            if (!currentCoroutineContext().isActive) break

            // Schedule reconnect with exponential backoff and jitter
            val delayMs = min(500L * 2.0.pow(reconnectAttempts).toLong() + Random.nextLong(200L), 15000L)
            reconnectAttempts++
            delay(delayMs)
        }
    }

    private suspend fun dispatchEvent(eventName: String, data: String) {
        if (data.isBlank()) return

        try {
            val jsonElement = http.json.parseToJsonElement(data)
            if (jsonElement !is JsonObject) return

            val op = jsonElement["op"]?.jsonPrimitive?.contentOrNull
                ?: jsonElement["action"]?.jsonPrimitive?.contentOrNull
                ?: "update"

            val collection = jsonElement["collection"]?.jsonPrimitive?.contentOrNull
                ?: (if (eventName != "record" && eventName != "message") eventName else "")

            val record = jsonElement["record"] ?: jsonElement
            val seq = jsonElement["seq"]?.jsonPrimitive?.longOrNull ?: 0L

            val raw = RawRealtimePayload(
                op = op,
                collection = collection,
                seq = seq,
                record = record
            )

            val targets = mutex.withLock {
                subscriptions[collection]?.toList() ?: emptyList()
            }

            for (target in targets) {
                target.dispatch(raw, http.json)
            }
        } catch (_: Throwable) {
            // Ignore unparseable realtime event
        }
    }
}
