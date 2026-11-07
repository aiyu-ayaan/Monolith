package dev.monolith.android.auth

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.browser.customtabs.CustomTabsIntent
import dev.monolith.client.auth.BrowserLauncher

/** Opens the provider sign-in page in a Custom Tab (falls back to the default browser). */
class CustomTabLauncher(context: Context) : BrowserLauncher {
    private val appContext = context.applicationContext

    override fun launch(url: String) {
        val intent = CustomTabsIntent.Builder().build().intent.apply {
            data = Uri.parse(url)
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        appContext.startActivity(intent)
    }
}
