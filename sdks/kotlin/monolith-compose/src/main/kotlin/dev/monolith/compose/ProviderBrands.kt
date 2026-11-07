package dev.monolith.compose

import androidx.compose.ui.graphics.Color

/** Name, color and monogram of a sign-in provider. */
data class ProviderBrand(val name: String, val color: Color, val letter: String)

/**
 * Brand colors by provider id. The built-in button draws a colored monogram, not the official logo,
 * so no brand asset is altered; replace it with [AuthUiOptions.providerButton] to use your own logo.
 */
object ProviderBrands {
    private val known = mapOf(
        "google" to ProviderBrand("Google", Color(0xFF4285F4), "G"),
        "github" to ProviderBrand("GitHub", Color(0xFF24292F), "G"),
        "apple" to ProviderBrand("Apple", Color(0xFF000000), "A"),
        "microsoft" to ProviderBrand("Microsoft", Color(0xFF00A4EF), "M"),
        "facebook" to ProviderBrand("Facebook", Color(0xFF1877F2), "f"),
        "discord" to ProviderBrand("Discord", Color(0xFF5865F2), "D"),
        "gitlab" to ProviderBrand("GitLab", Color(0xFFFC6D26), "G"),
        "x" to ProviderBrand("X", Color(0xFF000000), "X"),
        "linkedin" to ProviderBrand("LinkedIn", Color(0xFF0A66C2), "in"),
        "twitch" to ProviderBrand("Twitch", Color(0xFF9146FF), "T")
    )

    /** The brand for [id]; unknown ids (including `oidc`) get a neutral brand named [serverName]. */
    fun forProvider(id: String, serverName: String): ProviderBrand =
        known[id.lowercase()] ?: ProviderBrand(serverName.ifBlank { id }, Color(0xFF5F6368), serverName.firstOrNull()?.uppercase() ?: "?")
}
