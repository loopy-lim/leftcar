package dev.leftcar.viewer.km

import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.Assert.*
import org.junit.Test

class KmPhysicalTransitionTest {
    @Test fun `local down during unlocked capture acquisition prevents consume commit and up remains local`() {
        val gate = KmPhysicalTransition()
        gate.setMode(KmCaptureMode.OBSERVE)
        val acquiring = CountDownLatch(1)
        val downPassed = CountDownLatch(1)
        var committed = true
        val worker = Thread {
            val ticket = gate.beginCapture()
            acquiring.countDown()
            check(downPassed.await(2, TimeUnit.SECONDS))
            committed = gate.commitCapture(ticket) {}
        }
        worker.start()
        assertTrue(acquiring.await(2, TimeUnit.SECONDS))
        assertFalse(gate.key(29, true, 0))
        downPassed.countDown(); worker.join(2000)
        assertFalse(worker.isAlive)
        assertFalse(committed)
        assertFalse(gate.key(29, false, 0))
        assertTrue(gate.commitCapture(gate.beginCapture()) {})
        assertTrue(gate.key(29, true, 0))
    }
    @Test fun `lost KeyBridge permission atomically returns keys to local and defers reacquisition`() {
        val gate = KmPhysicalTransition(); gate.setMode(KmCaptureMode.OBSERVE)
        assertTrue(gate.commitCapture(gate.beginCapture()) {})
        assertFalse(gate.key(29, true, 0, allowed = false))
        assertThrows(IllegalStateException::class.java) { gate.beginCapture() }
        assertFalse(gate.key(29, false, 0, allowed = false))
    }
    @Test fun `modifiers buttons and a completed local tap invalidate pending capture`() {
        val gate = KmPhysicalTransition(); gate.setMode(KmCaptureMode.OBSERVE)
        val ticket = gate.beginCapture()
        gate.key(29, true, 0); gate.key(29, false, 0)
        assertFalse(gate.commitCapture(ticket) {})
        gate.pointer(1, 0)
        assertThrows(IllegalStateException::class.java) { gate.beginCapture() }
        gate.pointer(0, 1)
        assertThrows(IllegalStateException::class.java) { gate.beginCapture() }
        gate.pointer(0, 0)
        assertTrue(gate.commitCapture(gate.beginCapture()) {})
    }
}
