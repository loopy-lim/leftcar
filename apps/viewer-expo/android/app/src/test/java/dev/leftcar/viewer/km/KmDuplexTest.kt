package dev.leftcar.viewer.km

import java.io.DataInputStream
import java.io.DataOutputStream
import java.net.ServerSocket
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE)
class KmDuplexTest {
    @Test fun `native socket exchanges independently encrypted frames without video or React`() {
        val seed = ByteArray(32) { it.toByte() }
        val keys = KmCipher.keys(seed, "km-socket-test")
        val received = CountDownLatch(1)
        val peerRead = CountDownLatch(1)
        val peerClose = CountDownLatch(1)
        val failure = AtomicReference<Throwable?>()
        ServerSocket(0).use { server ->
            val peer = Thread {
                try {
                    server.accept().use { socket ->
                        socket.soTimeout = 2000
                        val input = DataInputStream(socket.getInputStream())
                        val frame = ByteArray(input.readInt()); input.readFully(frame)
                        val hello = String(KmCipher(keys.first).open(frame))
                        assertTrue(hello.contains("\"origin\":\"android\""))
                        assertTrue(hello.contains("\"kind\":\"hello\""))
                        peerRead.countDown()
                        val state = """{"version":1,"sessionId":"km-socket-test","epoch":1,"seq":1,"origin":"mac","payload":{"kind":"state","owner":"local"}}"""
                        val response = KmCipher(keys.second).seal(state.toByteArray())
                        val output = DataOutputStream(socket.getOutputStream())
                        output.writeInt(response.size); output.write(response); output.flush()
                        peerClose.await(2, TimeUnit.SECONDS)
                    }
                } catch (error: Throwable) { failure.set(error) }
            }.apply { start() }
            KmDuplex("127.0.0.1", server.localPort, seed, "km-socket-test", {
                if (it.kind == "state" && it.epoch == 1L && it.payload.getString("owner") == "local") received.countDown()
            }, { failure.set(AssertionError(it)) }).use { client ->
                client.send(KmWire.encode("km-socket-test", 1, 1, KmWire.payload("hello")))
                assertTrue(peerRead.await(2, TimeUnit.SECONDS))
                assertTrue(received.await(2, TimeUnit.SECONDS))
            }
            peerClose.countDown(); peer.join(2000)
            failure.get()?.let { throw AssertionError("Native socket failed", it) }
        }
    }

    @Test fun `oversized ciphertext terminates socket without allocating its requested payload`() {
        val stopped = CountDownLatch(1)
        ServerSocket(0).use { server ->
            val peer = Thread {
                server.accept().use { socket -> DataOutputStream(socket.getOutputStream()).apply { writeInt(Int.MAX_VALUE); flush() } }
            }.apply { start() }
            KmDuplex("127.0.0.1", server.localPort, ByteArray(32), "bounded", {}, { stopped.countDown() }).use {
                assertTrue(stopped.await(2, TimeUnit.SECONDS))
            }
            peer.join(2000)
        }
    }

    @Test fun `wire rejects unknown fields and numeric string coercion`() {
        fun frame(payload: String) = """{"version":1,"sessionId":"s","epoch":1,"seq":1,"origin":"mac","payload":$payload}""".toByteArray()
        assertThrows(IllegalArgumentException::class.java) { KmWire.decode(frame("""{"kind":"key","code":"4","down":true}"""), "s") }
        assertThrows(IllegalArgumentException::class.java) { KmWire.decode(frame("""{"kind":"heartbeat","ignored":true}"""), "s") }
        assertThrows(IllegalArgumentException::class.java) { KmWire.decode(frame("""{"kind":"move","dx":1e100,"dy":0}"""), "s") }
    }
}
