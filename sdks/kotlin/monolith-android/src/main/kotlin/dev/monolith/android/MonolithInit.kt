package dev.monolith.android

import android.content.Context
import dev.monolith.android.auth.CustomTabLauncher
import dev.monolith.android.storage.EncryptedSessionStorage
import dev.monolith.client.Monolith
import dev.monolith.client.MonolithClient
import dev.monolith.client.auth.BrowserLauncher
import dev.monolith.client.auth.SessionStorage

/** `monolith-<app>://auth`, the redirect URI used when none is given. */
fun defaultRedirectUri(app: String): String = "monolith-$app://auth"

/**
 * Android start in one line:
 * ```
 * val mono = Monolith.init(context, url = "https://api.example.com", app = "shop")
 * ```
 * The session lives in encrypted storage, tokens refresh by themselves, and provider sign-in opens a
 * Custom Tab. Declare `MonolithAuthRedirectActivity` for the redirect scheme (see its docs).
 * Calling it again with the same url and app returns the same client.
 */
fun Monolith.init(
    context: Context,
    url: String,
    app: String = "default",
    key: String? = null,
    redirectUri: String = defaultRedirectUri(app),
    storage: SessionStorage = EncryptedSessionStorage(context.applicationContext),
    browserLauncher: BrowserLauncher = CustomTabLauncher(context)
): MonolithClient = init(
    url = url,
    app = app,
    storage = storage,
    key = key,
    redirectUri = redirectUri,
    browserLauncher = browserLauncher
)
