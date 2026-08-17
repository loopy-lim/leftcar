package dev.leftcar.viewer.shim

import android.view.Surface

/**
 * JNI bridge to the Rust viewer core (C ABI of docs/05 §8.2).
 *
 * Shim boundary (docs/09 §9): only Surface + lifecycle forwarding. No codec
 * policy, no network, no business logic in Kotlin.
 */
object ViewerNative {
    init {
        System.loadLibrary("leftcar_viewer")
    }

    /** Returns the opaque process-state handle. */
    external fun start(): Long
    external fun attachSurface(state: Long, instanceId: String, surface: Surface): Int
    external fun surfaceChanged(state: Long, instanceId: String, width: Int, height: Int): Int
    external fun detachSurface(state: Long, instanceId: String): Int
    external fun updateWindowEvent(state: Long, instanceId: String, eventCode: Int, monotonicMs: Long): Int
    external fun release(state: Long, instanceId: String): Int
}
