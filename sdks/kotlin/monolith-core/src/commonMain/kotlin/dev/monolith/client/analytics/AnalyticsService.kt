package dev.monolith.client.analytics

import dev.monolith.client.http.HttpClient
import io.ktor.http.*
import kotlinx.coroutines.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.datetime.Clock
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import kotlin.time.Duration.Companion.milliseconds

/**
 * Single analytics event recorded by the client.
 */
@Serializable
data class AnalyticsEvent(
    val event: String,
    val properties: JsonElement? = null,
    val timestamp: String
)

@Serializable
private data class AnalyticsBatchRequest(
    val events: List<AnalyticsEvent>
)

/**
 * Batched analytics ingest service for recording client and app usage metrics.
 */
class AnalyticsService(
    private val http: HttpClient,
    private val flushIntervalMs: Long = 5000L,
    private val maxBatchSize: Int = 20,
    private val scope: CoroutineScope = CoroutineScope(Dispatchers.Default + SupervisorJob())
) {
    private val queue = mutableListOf<AnalyticsEvent>()
    private val mutex = Mutex()
    private var flushJob: Job? = null
    private var isFlushing = false

    /**
     * Enqueues an event for batched reporting.
     */
    fun track(event: String, properties: JsonElement? = null) {
        val timestamp = Clock.System.now().toString()
        val item = AnalyticsEvent(event, properties, timestamp)

        var shouldFlushImmediately = false
        var shouldScheduleTimer = false

        // Synchronously add to queue to ensure deterministic flush behavior
        synchronized(queue) {
            queue.add(item)
            if (queue.size >= maxBatchSize) {
                shouldFlushImmediately = true
            } else if (flushJob == null || !flushJob!!.isActive) {
                shouldScheduleTimer = true
            }
        }

        if (shouldFlushImmediately) {
            scope.launch {
                flush()
            }
        } else if (shouldScheduleTimer) {
            scheduleFlushTimer()
        }
    }

    /**
     * Flushes all currently queued analytics events immediately.
     */
    suspend fun flush() {
        mutex.withLock {
            flushJob?.cancel()
            flushJob = null
            flushInternal()
        }
    }

    private fun scheduleFlushTimer() {
        flushJob = scope.launch {
            delay(flushIntervalMs.milliseconds)
            mutex.withLock {
                flushJob = null
                flushInternal()
            }
        }
    }

    private suspend fun flushInternal() {
        if (isFlushing) return

        val batch = synchronized(queue) {
            if (queue.isEmpty()) return
            val count = minOf(queue.size, maxBatchSize)
            val taken = queue.take(count).toList()
            for (i in 0 until count) {
                queue.removeAt(0)
            }
            taken
        }

        isFlushing = true
        val path = "/api/v1/apps/${http.app}/analytics/events"
        try {
            http.request<Unit>(
                method = HttpMethod.Post,
                path = path,
                body = AnalyticsBatchRequest(batch),
                bodySerializer = AnalyticsBatchRequest.serializer()
            )
        } catch (_: Throwable) {
            // Re-queue on failure if within bounds
            synchronized(queue) {
                if (queue.size < 500) {
                    queue.addAll(0, batch)
                }
            }
        } finally {
            isFlushing = false
            val remaining = synchronized(queue) { queue.size }
            if (remaining > 0 && (flushJob == null || !flushJob!!.isActive)) {
                scheduleFlushTimer()
            }
        }
    }
}
