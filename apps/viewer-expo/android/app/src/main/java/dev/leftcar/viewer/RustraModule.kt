package dev.leftcar.viewer

import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.Arguments


/**
 * Rustra native module (docs/02 §9, docs/05 §8).
 *
 * JSI-style protocol expected by @rustra/react-native's
 * createReactNativeEngine: invoke(payload: ArrayBuffer): ArrayBuffer where
 * payload is UTF-8 JSON {command, args} and the return is UTF-8 JSON
 * {ok: true, result} | {ok: false, error}.
 *
 * SHIM BOUNDARY: encode/decode + JNI forwarding only. Command dispatch,
 * state and policy live in the Rust rustra package behind libleftcar_rustra.
 */
class RustraModule(reactContext: ReactApplicationContext) :
    ReactContextBaseJavaModule(reactContext) {

    override fun getName() = "Rustra"

    init {
        System.loadLibrary("leftcar_rustra")
        nativeStart()
    }

    /**
     * Bridge-style async variant used by the app (Promise<string>) — the JS
     * side wraps it into the same engine interface without JSI turbomodule
     * codegen, keeping the shim at minimum surface.
     */
    @ReactMethod
    fun invoke(command: String, argsJson: String, promise: Promise) {
        try {
            val raw = nativeInvoke(command, argsJson)
            promise.resolve(raw)
        } catch (t: Throwable) {
            promise.reject("ERR_RUSTRA", t.message, t)
        }
    }

    @ReactMethod
    fun contractHash(promise: Promise) {
        try {
            promise.resolve(nativeContractHash())
        } catch (t: Throwable) {
            promise.reject("ERR_RUSTRA", t.message, t)
        }
    }

    // -- JNI (libleftcar_rustra.so) --------------------------------------
    private external fun nativeStart()
    private external fun nativeInvoke(command: String, argsJson: String): String
    private external fun nativeContractHash(): String
}
