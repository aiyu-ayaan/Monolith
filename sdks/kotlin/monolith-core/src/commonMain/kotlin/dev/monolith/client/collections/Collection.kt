package dev.monolith.client.collections

import dev.monolith.client.http.CountResult
import dev.monolith.client.http.HttpClient
import dev.monolith.client.http.ListOptions
import dev.monolith.client.http.ListResult
import dev.monolith.client.http.RecordModel
import dev.monolith.client.realtime.RealtimeEvent
import dev.monolith.client.realtime.RealtimeListener
import dev.monolith.client.realtime.RealtimeService
import io.ktor.http.*
import kotlinx.coroutines.flow.Flow
import kotlinx.serialization.KSerializer
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.serializer

/**
 * Handle for performing typed CRUD operations, queries, and realtime subscriptions
 * on a specific Monolith collection.
 */
class Collection<T>(
    val name: String,
    private val http: HttpClient,
    private val realtime: RealtimeService? = null,
    private val serializer: KSerializer<T>? = null
) {
    private val basePath: String
        get() = "/api/v1/apps/${http.app}/collections/$name/records"

    @Suppress("UNCHECKED_CAST")
    private val effectiveSerializer: KSerializer<T>
        get() = serializer ?: (RecordModel.serializer() as KSerializer<T>)

    /**
     * Lists records in this collection according to the provided pagination and filter [options].
     */
    suspend fun list(options: ListOptions = ListOptions()): ListResult<T> {
        val query = mutableMapOf<String, Any?>()
        options.page?.let { query["page"] = it }
        options.perPage?.let { query["perPage"] = it }
        options.cursor?.let { query["cursor"] = it }
        options.limit?.let { query["limit"] = it }
        options.filter?.let { query["filter"] = it }
        options.sort?.let { query["sort"] = it }
        options.expand?.let { query["expand"] = it }
        options.fields?.let { query["fields"] = it }
        options.total?.let { query["total"] = it }

        val listSerializer = ListResult.serializer(effectiveSerializer)
        return http.request(
            method = HttpMethod.Get,
            path = basePath,
            query = query,
            serializer = listSerializer
        )
    }

    /**
     * Retrieves a single record by its [id].
     */
    suspend fun getOne(
        id: String,
        expand: String? = null,
        fields: String? = null
    ): T {
        val query = mutableMapOf<String, Any?>()
        expand?.let { query["expand"] = it }
        fields?.let { query["fields"] = it }

        return http.request(
            method = HttpMethod.Get,
            path = "$basePath/$id",
            query = query,
            serializer = effectiveSerializer
        )
    }

    /**
     * Creates a new record in this collection with typed model [data].
     */
    suspend fun create(data: T): T {
        return http.request(
            method = HttpMethod.Post,
            path = basePath,
            body = data,
            serializer = effectiveSerializer,
            bodySerializer = effectiveSerializer
        )
    }

    /**
     * Creates a new record in this collection with arbitrary [body] fields.
     */
    suspend fun create(body: JsonObject): T {
        return http.request(
            method = HttpMethod.Post,
            path = basePath,
            body = body,
            serializer = effectiveSerializer
        )
    }

    /**
     * Partially updates an existing record identified by [id].
     */
    suspend fun update(id: String, data: T): T {
        return http.request(
            method = HttpMethod.Patch,
            path = "$basePath/$id",
            body = data,
            serializer = effectiveSerializer,
            bodySerializer = effectiveSerializer
        )
    }

    /**
     * Partially updates an existing record identified by [id] with arbitrary [body] fields.
     */
    suspend fun update(id: String, body: JsonObject): T {
        return http.request(
            method = HttpMethod.Patch,
            path = "$basePath/$id",
            body = body,
            serializer = effectiveSerializer
        )
    }

    /**
     * Completely replaces an existing record identified by [id].
     */
    suspend fun replace(id: String, data: T): T {
        return http.request(
            method = HttpMethod.Put,
            path = "$basePath/$id",
            body = data,
            serializer = effectiveSerializer,
            bodySerializer = effectiveSerializer
        )
    }

    /**
     * Completely replaces an existing record identified by [id] with arbitrary [body] fields.
     */
    suspend fun replace(id: String, body: JsonObject): T {
        return http.request(
            method = HttpMethod.Put,
            path = "$basePath/$id",
            body = body,
            serializer = effectiveSerializer
        )
    }

    /**
     * Deletes a record identified by [id].
     */
    suspend fun delete(id: String): Boolean {
        http.request<Unit>(
            method = HttpMethod.Delete,
            path = "$basePath/$id"
        )
        return true
    }

    /**
     * Counts records in this collection matching an optional [filter].
     */
    suspend fun count(filter: String? = null): Long {
        val query = mutableMapOf<String, Any?>()
        filter?.let { query["filter"] = it }

        val res = http.request(
            method = HttpMethod.Get,
            path = "$basePath:count",
            query = query,
            serializer = CountResult.serializer()
        )
        return res.count
    }

    /**
     * Subscribes a callback to change events for this collection.
     * Returns an unsubscribe function.
     */
    fun subscribe(
        listener: RealtimeListener<T>,
        filter: String? = null
    ): () -> Unit {
        val rt = realtime ?: throw IllegalStateException("RealtimeService is not available on this client instance.")
        return rt.subscribe(name, listener, filter, serializer)
    }

    /**
     * Subscribes to change events for this collection as a Kotlin [Flow].
     */
    fun subscribe(filter: String? = null): Flow<RealtimeEvent<T>> {
        val rt = realtime ?: throw IllegalStateException("RealtimeService is not available on this client instance.")
        return rt.subscribeFlow(name, filter, serializer)
    }

    /**
     * Subscribes to change events for this collection as a Kotlin [Flow].
     */
    fun subscribeFlow(filter: String? = null): Flow<RealtimeEvent<T>> = subscribe(filter)
}
