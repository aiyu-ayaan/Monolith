package dev.monolith.compose

import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import dev.monolith.client.auth.AppUser
import dev.monolith.client.auth.AuthOptions
import dev.monolith.client.auth.AuthProviderInfo
import dev.monolith.client.auth.AuthUiError

/** How the sign-in screen is framed. */
enum class AuthLayout { Card, Fullscreen, Plain }

/** Every string of the screen; override any of them to translate. */
data class AuthLabels(
    val signInTitle: String = "Sign in",
    val signUpTitle: String = "Create account",
    val email: String = "Email",
    val password: String = "Password",
    val name: String = "Name (optional)",
    val signInButton: String = "Sign in",
    val signUpButton: String = "Create account",
    val switchToSignUp: String = "No account? Create one",
    val switchToSignIn: String = "Have an account? Sign in",
    val or: String = "or",
    val continueWith: (providerName: String) -> String = { "Continue with $it" },
    val signOut: String = "Sign out",
    val dismiss: String = "Dismiss",
    val retry: String = "Try again",
    val terms: String = "Terms",
    val privacy: String = "Privacy",
    val nothingToOffer: String = "No sign-in method is enabled for this app.",
    val showPassword: String = "Show password",
    val hidePassword: String = "Hide password"
)

/** Colors for the screen; null keeps the Material 3 theme value. */
data class AuthTheme(
    val primary: Color? = null,
    val onPrimary: Color? = null,
    val surface: Color? = null,
    val cornerRadiusDp: Int = 12
)

/** What a custom provider button receives. */
class ProviderButtonContext(
    val provider: AuthProviderInfo,
    val busy: Boolean,
    val enabled: Boolean,
    val onClick: () -> Unit
)

/**
 * Options of [MonolithAuth]. [auth] holds the behavior shared with the headless controller; the rest is looks.
 * Three levels of customization: drop-in (defaults), slots ([header], [providerButton], [footer], [errorBanner]),
 * and headless (use `AuthController` and draw everything yourself).
 */
data class AuthUiOptions(
    val auth: AuthOptions = AuthOptions(),
    val title: String? = null,
    val subtitle: String? = null,
    val termsUrl: String? = null,
    val privacyUrl: String? = null,
    val layout: AuthLayout = AuthLayout.Card,
    val theme: AuthTheme = AuthTheme(),
    val labels: AuthLabels = AuthLabels(),
    /** Replaces the header (logo, title, subtitle). */
    val header: (@Composable ColumnScope.(titleText: String) -> Unit)? = null,
    /** Replaces the built-in branded button of a provider. */
    val providerButton: (@Composable (ProviderButtonContext) -> Unit)? = null,
    /** Replaces the footer; receives the terms and privacy URLs. */
    val footer: (@Composable ColumnScope.(termsUrl: String?, privacyUrl: String?) -> Unit)? = null,
    /** Replaces the error banner. */
    val errorBanner: (@Composable (error: AuthUiError, dismiss: () -> Unit) -> Unit)? = null,
    val onSignedIn: ((AppUser) -> Unit)? = null,
    val onError: ((AuthUiError) -> Unit)? = null
)
