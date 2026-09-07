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
    /** Bind the UDP media port before Host reachability proof starts. */
    external fun prepareStream(port: Int, host: String, mediaTransport: String): Int
    /** Bind both consecutive UDP ports for the exact 4K vertical split. */
    external fun prepareSplitStream(port: Int, host: String, mediaTransport: String): Int
    /** Claim an Android UsbAccessory fd and start the native mux bridge. */
    external fun prepareUsb(fd: Int): Int
    /** Loopback TCP port used by the JS control client for USB sessions. */
    external fun usbControlPort(): Int
    /** Roll back a prepared port when Host start or Activity launch fails. */
    external fun cancelPreparedStream(port: Int): Int
    external fun cancelPreparedSplitStream(port: Int): Int
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
    /** Replace the renderer while retaining the Activity-owned Surface. */
    external fun rebindSurfacePort(
        state: Long,
        instanceId: String,
        surface: Surface,
        port: Int,
        host: String,
        width: Int,
        height: Int,
        fps: Int,
    ): Int
    external fun attachSplitSurfaces(
        state: Long,
        instanceId: String,
        leftSurface: Surface,
        rightSurface: Surface,
        port: Int,
        host: String,
        width: Int,
        height: Int,
        fps: Int,
        decoderName: String,
    ): Int
    external fun surfaceChanged(state: Long, instanceId: String, width: Int, height: Int): Int
    external fun detachSurface(state: Long, instanceId: String): Int
    external fun sendPointer(
        instanceId: String,
        action: Int,
        x: Float,
        y: Float,
        buttons: Int,
        actionButton: Int,
        horizontalScroll: Float,
        verticalScroll: Float,
    ): Int
    external fun sendKey(
        instanceId: String,
        keyCode: Int,
        scanCode: Int,
        metaState: Int,
        down: Boolean,
        repeat: Int,
    ): Int
    external fun releaseInput(instanceId: String): Int
    /** -1 waiting/unknown, 0 Host-locked, 1 remote input enabled. */
    external fun inputStatus(instanceId: String): Int
    /**
     * Drain pending host audio PCM into [out]. Blob layout:
     * rate u16 BE | channels u8 | rsv | frames u16 BE | PCM int16 LE.
     * Returns the written byte length; 0 when nothing is buffered.
     */
    external fun pollAudio(instanceId: String, out: ByteArray): Int
    /**
     * Newest LCD1 cursor sample packed into a Long: bits 0..15 x, 16..31 y,
     * 32..61 sequence (low 30 bits), 63 visible. -1 while the Host has not
     * opted in or no sample has arrived yet.
     */
    external fun cursorState(instanceId: String): Long
    /** Record the cursor stream opt-in; applied at the next control token. */
    external fun setCursorStream(instanceId: String, enabled: Boolean): Int
    /** Compact native renderer diagnostics; -1 when the stream is unavailable. */
    external fun streamStats(instanceId: String): Long
    /** LAN RTT + capture/encode/wire-to-decoder stage latency. */
    external fun streamLatency(instanceId: String): Long
    /**
     * Host-initiated termination reason, or -1 while the stream is alive.
     * 1 = connection lost (host health check), 2 = host operator forced stop,
     * 3 = ordinary host stop.
     */
    external fun terminationReason(instanceId: String): Int
    /** Capture to native decoder Surface release approximation; 0xffff = unmeasured. */
    external fun surfaceReleaseLatency(instanceId: String): Int
    external fun release(state: Long, instanceId: String): Int
}
