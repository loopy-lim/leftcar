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
