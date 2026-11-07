package dev.monolith.client.http

import kotlinx.serialization.Serializable

/**
 * Field-level problem details for validation errors in RFC 9457 format.
 */
@Serializable
data class FieldProblem(
    val field: String,
    val code: String,
    val message: String
)

/**
 * RFC 9457 Problem Details representation returned by Monolith.
 */
@Serializable
data class Problem(
    val type: String = "https://monolith.dev/errors/internal",
    val title: String = "Internal Error",
    val status: Int = 500,
    val detail: String? = null,
    val requestId: String? = null,
    val errors: List<FieldProblem> = emptyList()
)

/**
 * Exception thrown when the Monolith server returns an RFC 9457 problem response
 * or when an HTTP network error occurs.
 */
class MonolithException(
    val problem: Problem,
    cause: Throwable? = null
) : Exception(problem.detail ?: problem.title, cause) {

    val status: Int get() = problem.status
    val type: String get() = problem.type
    val detail: String? get() = problem.detail
    val requestId: String? get() = problem.requestId
    val errors: List<FieldProblem> get() = problem.errors

    /**
     * Finds the error message associated with a specific field if present.
     */
    fun getFieldError(field: String): String? =
        errors.firstOrNull { it.field == field }?.message

    override fun toString(): String {
        return "MonolithException(status=$status, type='$type', title='${problem.title}', detail=$detail, requestId=$requestId, errors=$errors)"
    }
}
