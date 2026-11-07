package dev.monolith.android.storage

import android.content.Context
import android.content.SharedPreferences
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import dev.monolith.client.auth.SessionStorage

/**
 * Keeps the app-user session (access and refresh token, user) in [EncryptedSharedPreferences].
 * If the Android Keystore is unavailable it falls back to app-private [SharedPreferences].
 */
class EncryptedSessionStorage(
    context: Context,
    prefFileName: String = "monolith_encrypted_session"
) : SessionStorage {

    private val prefs: SharedPreferences = try {
        val masterKey = MasterKey.Builder(context).setKeyScheme(MasterKey.KeyScheme.AES256_GCM).build()
        EncryptedSharedPreferences.create(
            context,
            prefFileName,
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
        )
    } catch (_: Throwable) {
        context.getSharedPreferences(prefFileName, Context.MODE_PRIVATE)
    }

    override fun load(): String? = prefs.getString(KEY, null)

    override fun save(value: String?) {
        prefs.edit().apply { if (value == null) remove(KEY) else putString(KEY, value) }.apply()
    }

    private companion object {
        const val KEY = "session"
    }
}
