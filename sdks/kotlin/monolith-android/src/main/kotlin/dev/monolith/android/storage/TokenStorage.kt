package dev.monolith.android.storage

import android.content.Context
import android.content.SharedPreferences
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey

/**
 * Interface for persisting authentication tokens on Android.
 */
interface TokenStorage {
    /**
     * Retrieves the stored authentication token, or null if absent.
     */
    fun getToken(): String?

    /**
     * Stores or updates the authentication token.
     */
    fun setToken(token: String?)

    /**
     * Clears the stored token.
     */
    fun clearToken()
}

/**
 * Token storage implementation using AndroidX [EncryptedSharedPreferences] backed by Android Keystore.
 * Automatically falls back to standard [SharedPreferences] if Keystore is unavailable.
 */
class EncryptedTokenStorage(
    context: Context,
    prefFileName: String = "monolith_encrypted_auth_prefs"
) : TokenStorage {

    private val prefs: SharedPreferences = try {
        val masterKey = MasterKey.Builder(context)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()

        EncryptedSharedPreferences.create(
            context,
            prefFileName,
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
        )
    } catch (_: Throwable) {
        // Fallback to mode private shared preferences if Keystore is unavailable (e.g. test environments)
        context.getSharedPreferences(prefFileName, Context.MODE_PRIVATE)
    }

    override fun getToken(): String? {
        return prefs.getString(KEY_TOKEN, null)
    }

    override fun setToken(token: String?) {
        prefs.edit().apply {
            if (token != null) {
                putString(KEY_TOKEN, token)
            } else {
                remove(KEY_TOKEN)
            }
            apply()
        }
    }

    override fun clearToken() {
        prefs.edit().remove(KEY_TOKEN).apply()
    }

    companion object {
        private const val KEY_TOKEN = "monolith_auth_token"
    }
}

/**
 * Token storage backed by standard Android [SharedPreferences].
 */
class SharedPreferencesTokenStorage(
    context: Context,
    prefFileName: String = "monolith_auth_prefs"
) : TokenStorage {

    private val prefs: SharedPreferences =
        context.getSharedPreferences(prefFileName, Context.MODE_PRIVATE)

    override fun getToken(): String? {
        return prefs.getString(KEY_TOKEN, null)
    }

    override fun setToken(token: String?) {
        prefs.edit().apply {
            if (token != null) {
                putString(KEY_TOKEN, token)
            } else {
                remove(KEY_TOKEN)
            }
            apply()
        }
    }

    override fun clearToken() {
        prefs.edit().remove(KEY_TOKEN).apply()
    }

    companion object {
        private const val KEY_TOKEN = "monolith_auth_token"
    }
}
