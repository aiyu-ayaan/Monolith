package dev.monolith.client.files

import dev.monolith.client.http.HttpClient
import io.ktor.http.*
import kotlinx.serialization.Serializable

/**
 * Metadata representation of an object stored in a Monolith bucket.
 */
@Serializable
data class StorageObject(
    val bucket: String,
    val key: String,
    val size: Long,
    val contentType: String? = null,
    val blake3: String? = null,
    val created: String? = null,
    val updated: String? = null
)

/**
 * Options for listing objects in a storage bucket.
 */
@Serializable
data class StorageListOptions(
    val prefix: String? = null,
    val cursor: String? = null,
    val limit: Int? = null
)

/**
 * Paginated list result of objects stored in a bucket.
 */
@Serializable
data class StorageListResult(
    val items: List<StorageObject> = emptyList(),
    val nextCursor: String? = null
)

/**
 * Service for uploading, downloading, listing, and deleting files and bucket objects.
 */
class FilesService(private val http: HttpClient) {

    private fun bucketPath(bucket: String): String =
        "/api/v1/apps/${http.app}/buckets/$bucket/objects"

    /**
     * Builds the public or authenticated URL to retrieve an object.
     * Optional [thumb] parameter requests an on-the-fly resized image thumbnail.
     */
    fun getURL(bucket: String, key: String, thumb: String? = null): String {
        val path = "${bucketPath(bucket)}/$key"
        val query = if (thumb != null) mapOf("thumb" to thumb) else null
        return http.buildUrl(path, query)
    }

    /**
     * Uploads binary [data] to the given [bucket] and [key].
     */
    suspend fun upload(
        bucket: String,
        key: String,
        data: ByteArray,
        contentType: String? = null
    ): StorageObject {
        val path = "${bucketPath(bucket)}/$key"
        val headers = mutableMapOf<String, String>()
        if (contentType != null) {
            headers["Content-Type"] = contentType
        }

        return http.request(
            method = HttpMethod.Put,
            path = path,
            body = data,
            headers = headers,
            serializer = StorageObject.serializer()
        )
    }

    /**
     * Uploads text content to the given [bucket] and [key].
     */
    suspend fun upload(
        bucket: String,
        key: String,
        text: String,
        contentType: String = "text/plain"
    ): StorageObject {
        return upload(bucket, key, text.encodeToByteArray(), contentType)
    }

    /**
     * Deletes an object from the specified [bucket].
     */
    suspend fun delete(bucket: String, key: String): Boolean {
        val path = "${bucketPath(bucket)}/$key"
        http.request<Unit>(
            method = HttpMethod.Delete,
            path = path
        )
        return true
    }

    /**
     * Lists objects in a [bucket] matching prefix or pagination options.
     */
    suspend fun list(
        bucket: String,
        options: StorageListOptions = StorageListOptions()
    ): StorageListResult {
        val query = mutableMapOf<String, Any?>()
        options.prefix?.let { query["prefix"] = it }
        options.cursor?.let { query["cursor"] = it }
        options.limit?.let { query["limit"] = it }

        return http.request(
            method = HttpMethod.Get,
            path = bucketPath(bucket),
            query = query,
            serializer = StorageListResult.serializer()
        )
    }
}
