package dev.leftcar.viewer.stream

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class StreamRecoveryRetryPolicyTest {
    @Test
    fun retryAttemptsUseShortBackoffAndStopAfterTheBoundedBudget() {
        val policy = StreamRecoveryRetryPolicy()

        assertEquals(RebindRetryAttempt(1, 0L), policy.nextAttempt())
        assertEquals(RebindRetryAttempt(2, 250L), policy.nextAttempt())
        assertEquals(RebindRetryAttempt(3, 500L), policy.nextAttempt())
        assertEquals(RebindRetryAttempt(4, 1_000L), policy.nextAttempt())
        assertEquals(RebindRetryAttempt(5, 2_000L), policy.nextAttempt())
        assertEquals(RebindRetryAttempt(6, 4_000L), policy.nextAttempt())
        assertNull(policy.nextAttempt())
    }

    @Test
    fun resetAllowsAHealthyRebindToStartAFreshRetryBudget() {
        val policy = StreamRecoveryRetryPolicy()

        repeat(6) { policy.nextAttempt() }
        assertNull(policy.nextAttempt())

        policy.reset()

        assertEquals(RebindRetryAttempt(1, 0L), policy.nextAttempt())
    }

    @Test
    fun hostUnreachableAndRenderStalledKeepTheExistingWindowAlive() {
        assertTrue(isSameWindowRecoveryReason(4))
        assertTrue(isSameWindowRecoveryReason(5))
        assertFalse(isSameWindowRecoveryReason(1))
        assertFalse(isSameWindowRecoveryReason(3))
    }
}
