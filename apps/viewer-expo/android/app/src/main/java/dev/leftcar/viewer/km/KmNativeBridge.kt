package dev.leftcar.viewer.km

import dev.leftcar.viewer.shim.ViewerNative

/**
 * Indirection over [ViewerNative] KM duplex entry points so JVM unit tests can
 * install an in-memory fake without java.net sockets (architecture L0).
 */
internal object KmNativeBridge {
    @Volatile
    var open: (String, Int, ByteArray, String) -> Long =
        { host, port, seed, sessionId -> ViewerNative.kmDuplexOpen(host, port, seed, sessionId) }

    @Volatile
    var send: (Long, ByteArray) -> Int = { handle, plain -> ViewerNative.kmDuplexSend(handle, plain) }

    @Volatile
    var recv: (Long, ByteArray, Int) -> Int =
        { handle, out, timeoutMs -> ViewerNative.kmDuplexRecv(handle, out, timeoutMs) }

    @Volatile
    var discard: (Long) -> Unit = { handle -> ViewerNative.kmDuplexDiscard(handle) }

    @Volatile
    var close: (Long) -> Unit = { handle -> ViewerNative.kmDuplexClose(handle) }

    fun reset() {
        open = { host, port, seed, sessionId -> ViewerNative.kmDuplexOpen(host, port, seed, sessionId) }
        send = { handle, plain -> ViewerNative.kmDuplexSend(handle, plain) }
        recv = { handle, out, timeoutMs -> ViewerNative.kmDuplexRecv(handle, out, timeoutMs) }
        discard = { handle -> ViewerNative.kmDuplexDiscard(handle) }
        close = { handle -> ViewerNative.kmDuplexClose(handle) }
    }
}
