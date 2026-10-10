package dev.leftcar.viewer.km

import java.io.DataInputStream
import java.io.DataOutputStream
import java.net.InetSocketAddress
import java.net.Socket
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/** Dedicated bounded duplex connection; native workers own all high-rate messages. */
internal class KmDuplex(
    host: String, port: Int, seed: ByteArray, private val sessionId: String,
    private val received: (KmWire) -> Unit, private val failed: (String) -> Unit,
    private val probe: KmHandshakeProbe? = null,
) : AutoCloseable {
    private val socket = Socket()
    private val stopped = AtomicBoolean(false)
    private val queue = ArrayBlockingQueue<ByteArray>(256)
    private val keys = KmCipher.keys(seed, sessionId)
    private val tx = KmCipher(keys.first)
    private val rx = KmCipher(keys.second)

    init {
        try {
            require(port in 1..65535)
            socket.tcpNoDelay = true
            socket.soTimeout = 1000
            probe?.record(KmHandshakeProbe.Stage.SOCKET_CONNECT, KmHandshakeProbe.Code.BEGIN)
            socket.connect(InetSocketAddress(host, port), 2000)
            probe?.record(KmHandshakeProbe.Stage.SOCKET_CONNECTED, KmHandshakeProbe.Code.OK)
            Thread(::readLoop, "km-tcp-reader").apply { isDaemon = true }.start()
            Thread(::writeLoop, "km-tcp-writer").apply { isDaemon = true }.start()
        } catch (e: Exception) {
            probe?.record(KmHandshakeProbe.Stage.SOCKET_CONNECT, KmHandshakeProbe.Code.FAILED, e)
            close(); throw e
        }
    }

    fun send(bytes: ByteArray) {
        check(!stopped.get() && bytes.size in 1..8192 && queue.offer(bytes)) { "KM output queue unavailable" }
    }

    /** Called before applying a new host epoch; an in-flight old frame remains stale at the peer. */
    fun discardQueued() { queue.clear() }

    private fun readLoop() {
        var first = true
        var stage = KmHandshakeProbe.Stage.RECEIVE_FRAME
        try {
            val input = DataInputStream(socket.getInputStream())
            probe?.record(stage, KmHandshakeProbe.Code.BEGIN)
            while (!stopped.get()) {
                val length = input.readInt()
                require(length in 25..8216) { "Invalid KM frame size" }
                val frame = ByteArray(length)
                input.readFully(frame)
                if (first) {
                    stage = KmHandshakeProbe.Stage.RECEIVE_AUTH
                    probe?.record(stage, KmHandshakeProbe.Code.BEGIN)
                }
                val plain = rx.open(frame)
                if (first) {
                    stage = KmHandshakeProbe.Stage.RECEIVE_DECODE
                    probe?.record(stage, KmHandshakeProbe.Code.BEGIN)
                }
                val message = KmWire.decode(plain, sessionId)
                if (first) {
                    probe?.record(KmHandshakeProbe.Stage.RECEIVE_DECODED, KmHandshakeProbe.Code.OK)
                    first = false
                    stage = KmHandshakeProbe.Stage.RECEIVE_FRAME
                }
                received(message)
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
            val output = DataOutputStream(socket.getOutputStream())
            while (!stopped.get()) {
                val plain = queue.poll(100, TimeUnit.MILLISECONDS) ?: continue
                if (first) {
                    stage = KmHandshakeProbe.Stage.HELLO_ENCRYPT
                    probe?.record(stage, KmHandshakeProbe.Code.BEGIN)
                }
                val frame = tx.seal(plain)
                if (first) {
                    stage = KmHandshakeProbe.Stage.HELLO_WRITE
                    probe?.record(stage, KmHandshakeProbe.Code.BEGIN)
                }
                output.writeInt(frame.size); output.write(frame); output.flush()
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
        if (!stopped.get()) { probe?.finish(KmHandshakeProbe.Code.STOPPED); close(); failed(reason) }
    }

    override fun close() {
        if (!stopped.compareAndSet(false, true)) return
        runCatching { socket.close() }
        queue.clear(); tx.close(); rx.close()
    }
}
