package dev.leftcar.viewer.km

import org.junit.Assert.*
import org.junit.Test

class KmLifecycleTest {
    @Test fun `KeyBridge remains enabled without external keyboard and attachment revokes unleased sharing`() {
        assertFalse(KmKeyboardInventory.conflicts(external = false, virtual = false, keyboard = true, own = false))
        assertFalse(KmKeyboardInventory.conflicts(external = true, virtual = true, keyboard = true, own = false))
        assertFalse(KmKeyboardInventory.conflicts(external = true, virtual = false, keyboard = true, own = true))
        assertTrue(KmKeyboardInventory.conflicts(external = true, virtual = false, keyboard = true, own = false))
        assertTrue(KmKeyboardInventory.mustStop(active = true, keyBridgeEnabled = true, leaseAcknowledged = false, physicalKeyboard = true))
        assertFalse(KmKeyboardInventory.mustStop(true, true, true, true))
        assertFalse(KmKeyboardInventory.mustStop(true, true, false, false))
    }

    @Test fun `active keyboard capture still observes unrelated pointer devices`() {
        assertTrue(KmCaptureMode.CONSUME.observesMouse())
        assertTrue(KmCaptureMode.OBSERVE.observesMouse())
        assertFalse(KmCaptureMode.OFF.observesMouse())
    }
    @Test fun `local return waits for native placement then releases before ACK`() {
        val events = mutableListOf<String>()
        val lifecycle = KmOwnerTransition({ events += "neutral" }, { events += "ungrab" }, { events += "restore:$it" }, { events += "ack" })
        lifecycle.apply("local", placementPending = true)
        assertTrue(events.isEmpty())
        lifecycle.placementCompleted()
        assertEquals(listOf("neutral", "ungrab", "restore:local", "ack"), events)
    }
    @Test fun `release failure never restores or acknowledges and disconnect attempts every cleanup`() {
        val events = mutableListOf<String>()
        val lifecycle = KmOwnerTransition({ events += "neutral"; error("failed") }, { events += "ungrab" }, { events += "restore:$it" }, { events += "ack" })
        assertThrows(IllegalStateException::class.java) { lifecycle.apply("local", false) }
        assertEquals(listOf("neutral"), events)
        events.clear()
        assertFalse(lifecycle.disconnect { events += "close" })
        assertEquals(listOf("neutral", "ungrab", "close", "restore:off"), events)
    }
    @Test fun `restoration failure after release cannot emit an applied ACK`() {
        val events = mutableListOf<String>()
        val lifecycle = KmOwnerTransition({ events += "neutral" }, { events += "ungrab" },
            { events += "restore"; error("readback denied") }, { events += "ack" })
        lifecycle.apply("local", true)
        assertThrows(IllegalStateException::class.java) { lifecycle.placementCompleted() }
        assertEquals(listOf("neutral", "ungrab", "restore"), events)
    }
    @Test fun `authority denies foreign UID and expires once after heartbeat or binder death`() {
        var now = 0L; var closes = 0
        val life = KmAuthorityLifetime(12345, { now }, { closes++ })
        assertThrows(IllegalStateException::class.java) { life.authorize(12346) }
        life.authorize(12345); life.prepared(); now = 900; life.heartbeat(); now = 1800
        life.tick(); assertEquals(0, closes)
        now = 1901; life.tick(); life.tick(); assertEquals(1, closes)
        assertThrows(IllegalStateException::class.java) { life.heartbeat() }
        life.prepared(); life.disconnected(); life.disconnected(); assertEquals(2, closes)
    }
    @Test fun `placement uses observed coordinates honors XY and has bounded failure`() {
        val p = KmCursorPlacement(700.0, 100.0, 0)
        val step = p.observe(100.0, 400.0, 1)!!
        assertTrue(step.first > 0 && step.second < 0)
        assertNull(p.observe(700.0, 100.0, 2))
        assertThrows(IllegalStateException::class.java) { KmCursorPlacement(700.0,100.0,0).observe(0.0,0.0,1001) }
    }
}
