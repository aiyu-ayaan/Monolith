package dev.monolith.client

import dev.monolith.client.analytics.AnalyticsService
import dev.monolith.client.auth.AppAuth
import dev.monolith.client.auth.AuthService
import dev.monolith.client.collections.Collection
import dev.monolith.client.files.FilesService
import dev.monolith.client.http.BatchOperation
import dev.monolith.client.http.BatchResponseItem
import dev.monolith.client.http.HttpClient
import dev.monolith.client.http.RecordModel
import dev.monolith.client.realtime.RealtimeService
import io.ktor.http.*
import kotlinx.serialization.KSerializer
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.serializer

@Serializable
private data class BatchRequest(
    val operations: List<BatchOperation>
)

/**
 * Main Monolith multiplatform client for database, auth, realtime, files, and analytics.
 */
class MonolithClient(val options: ClientOptions) {
    val http: HttpClient = HttpClient(options)
    val realtime: RealtimeService = RealtimeService(http)
    val files: FilesService = FilesService(http)
    val analytics: AnalyticsService = AnalyticsService(http)

    /**
     * Auth for the app's end users (sign up/in, providers, session, refresh). Use `auth("_superusers")`
     * for the system login.
     */
    val auth: AppAuth = AppAuth(http, options.session)

    private val authServices = mutableMapOf<String, AuthService>()
    private val collections = mutableMapOf<String, Collection<*>>()

    /**
     * Obtains an [AuthService] for the specified auth [collection] (defaults to "users").
     */
    fun auth(collection: String = "users"): AuthService {
        return authServices.getOrPut(collection) {
            AuthService(http, collection)
        }
    }

    /**
     * Obtains a typed [Collection] handle for performing CRUD operations and queries.
     */
    @Suppress("UNCHECKED_CAST")
    fun <T> collection(name: String, serializer: KSerializer<T>? = null): Collection<T> {
        val key = "$name:${serializer?.descriptor?.serialName ?: "RecordModel"}"
        return collections.getOrPut(key) {
            Collection(name, http, realtime, serializer)
        } as Collection<T>
    }

    /**
     * Obtains a typed [Collection] handle using reified type argument.
     */
    inline fun <reified T> collection(name: String): Collection<T> {
        return collection(name, serializer<T>())
    }

    /**
     * Obtains a schemaless [Collection] handle typed as [RecordModel].
     */
    @kotlin.jvm.JvmName("recordCollection")
    fun collection(name: String): Collection<RecordModel> {
        return collection(name, RecordModel.serializer())
    }

    /**
     * Executes atomic batch operations against the app database.
     */
    suspend fun batch(operations: List<BatchOperation>): List<BatchResponseItem> {
        val path = "/api/v1/apps/${http.app}/batch"
        return http.request(
            method = HttpMethod.Post,
            path = path,
            body = BatchRequest(operations),
            serializer = ListSerializer(BatchResponseItem.serializer()),
            bodySerializer = BatchRequest.serializer()
        )
    }

    /**
     * Closes network connections and realtime streams.
     */
    suspend fun close() {
        realtime.disconnect()
        http.close()
    }

    companion object {
        /**
         * Creates a new [MonolithClient] instance with the provided [options].
         */
        fun create(options: ClientOptions): MonolithClient = MonolithClient(options)

        /**
         * Creates a new [MonolithClient] instance with common parameters.
         */
        fun create(
            url: String,
            app: String = "default",
            key: String? = null,
            token: String? = null
        ): MonolithClient = MonolithClient(ClientOptions(url = url, app = app, key = key, token = token))
    }
}
