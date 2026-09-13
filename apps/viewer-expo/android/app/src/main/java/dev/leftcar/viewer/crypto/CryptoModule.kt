package dev.leftcar.viewer.crypto

import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.bridge.Promise
import java.security.SecureRandom
import android.util.Base64

/**
 * CSPRNG for the secure control channel (secure-channel.ts). Returns bytes as
 * a Base64 NO_WRAP string to dodge the legacy bridge's array boxing.
 */
class CryptoModule(reactContext: ReactApplicationContext) :
    ReactContextBaseJavaModule(reactContext) {
    override fun getName() = "LeftcarCrypto"

    @ReactMethod
    fun randomBytes(length: Int, promise: Promise) {
        try {
            val clamped = length.coerceIn(0, 65536)
            val bytes = ByteArray(clamped)
            SecureRandom().nextBytes(bytes)
            promise.resolve(Base64.encodeToString(bytes, Base64.NO_WRAP))
        } catch (e: Exception) {
            promise.reject("random_bytes_failed", e)
        }
    }
}
