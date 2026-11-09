package dev.leftcar.viewer.km

import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

/**
 * Thin Kotlin facade over the Rust KM TCP duplex (`secure_channel` + framing).
 * Sockets and AEAD stay in native code (architecture L0).
 */
internal class KmDuplex(
    host: String,
    port: Int,
    seed: ByteArray,
    private val sessionId: String,
    private val received: (KmWire) -> Unit,
    private val failed: (String) -> Unit,
    private val probe: KmHandshakeProbe? = null,
) : AutoCloseable {
    private val handle = AtomicLong(0)
    private val stopped = AtomicBoolean(false)
    private val queue = ArrayBlockingQueue<ByteArray>(256)

    init {
        try {
            require(port in 1..65535)
            require(seed.size == 32)
            probe?.record(KmHandshakeProbe.Stage.SOCKET_CONNECT, KmHandshakeProbe.Code.BEGIN)
            val opened = KmNativeBridge.open(host, port, seed, sessionId)
            check(opened != 0L) { "KM native duplex open failed" }
            handle.set(opened)
            probe?.record(KmHandshakeProbe.Stage.SOCKET_CONNECTED, KmHandshakeProbe.Code.OK)
            Thread(::readLoop, "km-tcp-reader").apply { isDaemon = true }.start()
            Thread(::writeLoop, "km-tcp-writer").apply { isDaemon = true }.start()
        } catch (e: Exception) {
            probe?.record(KmHandshakeProbe.Stage.SOCKET_CONNECT, KmHandshakeProbe.Code.FAILED, e)
            close()
            throw e
        }
    }

    fun send(bytes: ByteArray) {
        check(!stopped.get() && bytes.size in 1..8192 && queue.offer(bytes)) {
            "KM output queue unavailable"
        }
    }

    /** Called before applying a new host epoch; an in-flight old frame remains stale at the peer. */
    fun discardQueued() {
        queue.clear()
        KmNativeBridge.discard(handle.get())
    }

    private fun readLoop() {
        var first = true
        var stage = KmHandshakeProbe.Stage.RECEIVE_FRAME
        val buffer = ByteArray(8216)
        try {
            probe?.record(stage, KmHandshakeProbe.Code.BEGIN)
            while (!stopped.get()) {
                val native = handle.get()
                if (native == 0L) break
                val got = KmNativeBridge.recv(native, buffer, 200)
                when {
                    got == -3 -> continue // timeout
                    got < 0 -> throw IllegalStateException("KM recv failed: $got")
                    else -> {
                        if (first) {
                            stage = KmHandshakeProbe.Stage.RECEIVE_AUTH
                            probe?.record(stage, KmHandshakeProbe.Code.BEGIN)
                            stage = KmHandshakeProbe.Stage.RECEIVE_DECODE
                            probe?.record(stage, KmHandshakeProbe.Code.BEGIN)
                        }
                        val plain = buffer.copyOf(got)
                        val message = KmWire.decode(plain, sessionId)
                        if (first) {
                            probe?.record(KmHandshakeProbe.Stage.RECEIVE_DECODED, KmHandshakeProbe.Code.OK)
                            first = false
                            stage = KmHandshakeProbe.Stage.RECEIVE_FRAME
                        }
                        received(message)
                    }
                }
            }
        } catch (error: Exception) {
            probe?.record(stage, KmHandshakeProbe.Code.FAILED, error)
            fail("암호화 입력 연결이 종료되었거나 응답이 없습니다")
        }
    }

    private fun writeLoop() {
        var first = true
        var stage = KmHandshakeProbe.Stage.HELLO_WRITE
        try {
            while (!stopped.get()) {
                val plain = queue.poll(100, TimeUnit.MILLISECONDS) ?: continue
                val native = handle.get()
                if (native == 0L) break
                if (first) {
                    stage = KmHandshakeProbe.Stage.HELLO_ENCRYPT
                    probe?.record(stage, KmHandshakeProbe.Code.BEGIN)
                    stage = KmHandshakeProbe.Stage.HELLO_WRITE
                    probe?.record(stage, KmHandshakeProbe.Code.BEGIN)
                }
                val code = KmNativeBridge.send(native, plain)
                check(code == 0) { "KM send failed: $code" }
                if (first) {
                    probe?.record(KmHandshakeProbe.Stage.HELLO_WRITTEN, KmHandshakeProbe.Code.OK)
                    first = false
                }
            }
        } catch (error: Exception) {
            probe?.record(stage, KmHandshakeProbe.Code.FAILED, error)
            fail("입력 전송 연결이 종료되었습니다")
        }
    }

    private fun fail(reason: String) {
        if (!stopped.get()) {
            probe?.finish(KmHandshakeProbe.Code.STOPPED)
            close()
            failed(reason)
        }
    }

    override fun close() {
        if (!stopped.compareAndSet(false, true)) return
        queue.clear()
        val native = handle.getAndSet(0L)
        if (native != 0L) KmNativeBridge.close(native)
    }
}
