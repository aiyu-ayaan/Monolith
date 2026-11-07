package dev.monolith.android.auth

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import dev.monolith.client.Monolith

/**
 * Receives the provider redirect (`<scheme>://auth#token=...`), completes the sign-in on every client
 * made by [Monolith.init], and finishes. Declare it in the app manifest with the redirect scheme:
 *
 * ```xml
 * <activity android:name="dev.monolith.android.auth.MonolithAuthRedirectActivity"
 *           android:exported="true" android:launchMode="singleTask" android:theme="@android:style/Theme.NoDisplay">
 *   <intent-filter>
 *     <action android:name="android.intent.action.VIEW"/>
 *     <category android:name="android.intent.category.DEFAULT"/>
 *     <category android:name="android.intent.category.BROWSABLE"/>
 *     <data android:scheme="monolith-myapp" android:host="auth"/>
 *   </intent-filter>
 * </activity>
 * ```
 * A launcher activity can bring the app to the front; this one only hands the URI over.
 */
class MonolithAuthRedirectActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        handle(intent)
        finish()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handle(intent)
        finish()
    }

    private fun handle(intent: Intent?) {
        val uri = intent?.data?.toString() ?: return
        completeRedirect(uri)
        packageManager.getLaunchIntentForPackage(packageName)?.let {
            it.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT)
            startActivity(it)
        }
    }
}

/** Hands a redirect URI to every client created by [Monolith.init]; call it from your own activity if you prefer. */
fun completeRedirect(uri: String) {
    Monolith.all().forEach { it.auth.handleRedirectAsync(uri) }
}
