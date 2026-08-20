package dev.leftcar.viewer.shim

import android.view.Surface

/**
 * JNI bridge to the Rust viewer core (same libleftcar_viewer.so as the
 * viewer-android shim — Java package name in the symbol table is
 * dev.leftcar.viewer.shim.ViewerNative, kept for binary compatibility).
 *
 * Shim boundary (docs/09 §9): only Surface + lifecycle forwarding.
 */
object ViewerNative {
    init {
        System.loadLibrary("leftcar_viewer")
    }

    /** Returns the opaque process-state handle. */
    external fun start(): Long
    external fun updateWindowEvent(state: Long, instanceId: String, eventCode: Int, monotonicMs: Long): Int
    /**
     * Attach with an explicit media port + paired host IP. The Rust media
     * listener accepts TCP senders only from that host (strict peer IP
     * equality) — a window without a paired host never receives video.
     */
    external fun attachSurfacePort(
        state: Long,
        instanceId: String,
        surface: Surface,
        port: Int,
        host: String,
        width: Int,
        height: Int,
        fps: Int,
    ): Int
    external fun surfaceChanged(state: Long, instanceId: String, width: Int, height: Int): Int
    external fun detachSurface(state: Long, instanceId: String): Int
    external fun release(state: Long, instanceId: String): Int
}
