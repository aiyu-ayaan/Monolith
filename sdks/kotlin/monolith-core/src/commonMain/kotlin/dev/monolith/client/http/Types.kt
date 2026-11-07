package dev.monolith.client.http

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject

/**
 * Pagination and query options for listing collection records.
 */
@Serializable
data class ListOptions(
    val page: Int? = null,
    val perPage: Int? = null,
    val cursor: String? = null,
    val limit: Int? = null,
    val filter: String? = null,
    val sort: String? = null,
    val expand: String? = null,
    val fields: String? = null,
    val total: String? = null // "none" | "exact" | "estimate"
)

/**
 * Result of a paginated collection list query.
 */
@Serializable
data class ListResult<T>(
    val items: List<T> = emptyList(),
    val total: Long? = null,
    val cursor: String? = null,
    val nextCursor: String? = null,
    val prevCursor: String? = null
)

/**
 * Result of a collection count query.
 */
@Serializable
data class CountResult(
    val count: Long
)

/**
 * Default generic record model for schemaless or un-modeled collection access.
 */
@Serializable
data class RecordModel(
    val id: String = "",
    val created: String = "",
    val updated: String = "",
    val _v: Long = 0,
    val data: JsonObject = JsonObject(emptyMap())
)

/**
 * Single operation in a transactional batch request.
 */
@Serializable
data class BatchOperation(
    val method: String,
    val path: String,
    val body: JsonElement? = null
)

/**
 * Single response item from a transactional batch request.
 */
@Serializable
data class BatchResponseItem(
    val status: Int,
    val body: JsonElement? = null
)
