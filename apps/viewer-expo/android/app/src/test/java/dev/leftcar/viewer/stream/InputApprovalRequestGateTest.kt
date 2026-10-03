package dev.leftcar.viewer.stream

import org.junit.Assert.*
import org.junit.Test

class InputApprovalRequestGateTest {
    @Test fun `the first explicit request works even immediately after boot`() {
        assertTrue(InputApprovalRequestGate().claim(status = 0, nowMs = 0L))
    }

    @Test fun `pointer keyboard and accessibility attempts share the three second cooldown`() {
        val gate = InputApprovalRequestGate()
        assertTrue(gate.claim(0, 10_000L))
        assertFalse(gate.claim(0, 10_100L))
        assertFalse(gate.claim(0, 12_999L))
        assertTrue(gate.claim(0, 13_000L))
    }

    @Test fun `unknown and allowed permission never claim a request or consume its cooldown`() {
        val gate = InputApprovalRequestGate()
        assertFalse(gate.claim(-1, 0L))
        assertFalse(gate.claim(1, 100L))
        assertTrue(gate.claim(0, 200L))
    }
}
