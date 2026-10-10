package dev.leftcar.viewer.km

import org.junit.Assert.*
import org.junit.Test

class KmNativeSessionTest {
    @Test fun `held keys include modifiers and release produces neutral report`() {
        val hid = KmHidReports()
        hid.key(0xe1, true)
        hid.key(4, true)
        assertEquals(2, hid.keyboard()[0].toInt())
        assertEquals(4, hid.keyboard()[2].toInt())
        hid.release()
        assertArrayEquals(ByteArray(8), hid.keyboard())
        assertArrayEquals(ByteArray(7), hid.mouse(0, 0, 0, 0))
    }

    @Test fun `mouse report preserves buttons signed deltas and both wheels`() {
        val hid = KmHidReports()
        hid.button(1, true)
        assertArrayEquals(byteArrayOf(1, -2, -1, 3, 0, -1, 2), hid.mouse(-2, 3, -1, 2))
        hid.button(1, false)
        assertEquals(0, hid.mouse(0, 0, 0, 0)[0].toInt())
    }

    @Test fun `keyboard overflow fails without dropping held key releases`() {
        val hid = KmHidReports()
        for (key in 4..9) hid.key(key, true)
        assertThrows(IllegalArgumentException::class.java) { hid.key(10, true) }
        hid.key(4, false)
        assertEquals(0, hid.keyboard().last().toInt())
    }

    @Test fun `armed never consumes and unsupported observation stays off`() {
        assertEquals(KmCaptureMode.OFF, KmCapturePolicy.mode(false, "local"))
        assertEquals(KmCaptureMode.OBSERVE, KmCapturePolicy.mode(true, "local"))
        assertEquals(KmCaptureMode.OBSERVE, KmCapturePolicy.mode(true, "macToAndroid"))
        assertEquals(KmCaptureMode.CONSUME, KmCapturePolicy.mode(true, "androidToMac"))
    }

    @Test fun `sealer interoperability replay rejection and failed authentication do not advance counter`() {
        val keys = KmCipher.keys(ByteArray(32) { it.toByte() }, "km-test")
        assertFalse(keys.first.contentEquals(keys.second))
        val sender = KmCipher(keys.first)
        val receiver = KmCipher(keys.first)
        val frame = sender.seal("hello".toByteArray())
        val corrupt = frame.clone().apply { this[lastIndex] = (this[lastIndex].toInt() xor 1).toByte() }
        assertThrows(Exception::class.java) { receiver.open(corrupt) }
        assertEquals("hello", String(receiver.open(frame)))
        assertThrows(IllegalArgumentException::class.java) { receiver.open(frame) }
    }

    @Test fun `Rust and Swift fixed ciphertext matches byte for byte`() {
        val keys = KmCipher.keys(ByteArray(32) { it.toByte() }, "km-vector-v1")
        fun hex(bytes: ByteArray) = bytes.joinToString("") { "%02x".format(it.toInt() and 255) }
        assertEquals("c1832723e3708ccaba0b6556606315f47bed62a617ea2f5245c70bf03d154e9d", hex(keys.first))
        val plain = """{"version":1,"sessionId":"km-vector-v1","epoch":1,"seq":1,"origin":"android","payload":{"kind":"hello"}}"""
        assertEquals("0000000000000001466bd7edde80cbaa55c15a07434f23b12d7c5221fb465c4eb0977abaf4c145e950919376679c11da9fc5ee46e873b766a8a6bbf6cb81cde9f6a580a134a8875a7e284131812f27006f516f5b3e95fabb9218916b58d8970ee6ec8779f83766fe0c42f4ac631a7aebe8c2c18ec109b297b98e51119ff21759", hex(KmCipher(keys.first).seal(plain.toByteArray())))
    }

    @Test fun `only state advances epoch and replay cannot change authority`() {
        val gate = KmEpochGate(1)
        gate.accept(1, 1, "state")
        assertThrows(IllegalArgumentException::class.java) { gate.accept(2, 2, "move") }
        gate.accept(2, 2, "state")
        assertEquals(2L, gate.epoch)
        assertThrows(IllegalArgumentException::class.java) { gate.accept(1, 3, "key") }
        assertThrows(IllegalArgumentException::class.java) { gate.accept(2, 2, "state") }
    }
}
