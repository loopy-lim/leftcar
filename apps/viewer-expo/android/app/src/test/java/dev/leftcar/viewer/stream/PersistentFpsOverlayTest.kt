package dev.leftcar.viewer.stream

import android.view.Gravity
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PersistentFpsOverlayTest {
    @Test
    fun initialSampleShowsUnknownFps() {
        val sample = nextRenderedFpsSample(null, renderedFrames = 100L, nowMs = 1_000L)

        assertNull(sample.displayedFps)
        assertEquals("-- FPS", persistentFpsText(sample.displayedFps))
    }

    @Test
    fun renderedFrameDeltaUsesElapsedSampleTime() {
        val previous = RenderedFpsSample(frames = 100L, sampledAtMs = 1_000L, displayedFps = null)

        val sample = nextRenderedFpsSample(previous, renderedFrames = 110L, nowMs = 1_250L)

        assertEquals(40.0, sample.displayedFps!!, 0.001)
        assertEquals("40 FPS", persistentFpsText(sample.displayedFps))
    }

    @Test
    fun counterResetShowsUnknownUntilANewDeltaExists() {
        val previous = RenderedFpsSample(frames = 110L, sampledAtMs = 1_250L, displayedFps = 40.0)

        val reset = nextRenderedFpsSample(previous, renderedFrames = 3L, nowMs = 1_500L)
        val resumed = nextRenderedFpsSample(reset, renderedFrames = 13L, nowMs = 1_750L)

        assertNull(reset.displayedFps)
        assertEquals("-- FPS", persistentFpsText(reset.displayedFps))
        assertEquals(40.0, resumed.displayedFps!!, 0.001)
    }

    @Test
    fun smoothingKeepsExistingBoundedFormula() {
        val previous = RenderedFpsSample(frames = 100L, sampledAtMs = 1_000L, displayedFps = 40.0)

        val sample = nextRenderedFpsSample(previous, renderedFrames = 200L, nowMs = 1_250L)

        // The 400 FPS raw sample is capped at 240 before 0.65/0.35 smoothing.
        assertEquals(110.0, sample.displayedFps!!, 0.001)
    }

    @Test
    fun zeroFpsRetainsTheExistingInitialSampleBehavior() {
        val previous = RenderedFpsSample(frames = 100L, sampledAtMs = 1_000L, displayedFps = 0.0)

        val sample = nextRenderedFpsSample(previous, renderedFrames = 110L, nowMs = 1_250L)

        assertEquals(40.0, sample.displayedFps!!, 0.001)
    }

    @Test
    fun persistentOverlayPolicyIsBottomEndNonInteractiveAndDescribesRoundedRate() {
        val policy = persistentFpsOverlayPolicy

        assertEquals(Gravity.BOTTOM or Gravity.END, policy.gravity)
        assertFalse(policy.touchable)
        assertFalse(policy.focusable)
        assertFalse(policy.outsideTouchable)
        assertFalse(policy.animate)
        assertEquals(0f, policy.elevationDp, 0f)
        assertEquals("실제 렌더링 속도 41 FPS", persistentFpsContentDescription(40.6))
        assertEquals("실제 렌더링 속도 -- FPS", persistentFpsContentDescription(null))
    }

    @Test
    fun persistentOverlayRemainsReadableOnAHighDensityXrDisplay() {
        val policy = persistentFpsOverlayPolicy

        assertTrue(policy.textSizeSp >= 13f)
        assertTrue(policy.textColorAlpha >= 224)
        assertTrue(policy.backgroundAlpha >= 128)
        assertTrue(policy.horizontalPaddingDp >= 8)
        assertTrue(policy.verticalPaddingDp >= 4)
        assertTrue(policy.edgeOffsetDp >= 16)
    }

    @Test
    fun cachedTerminationIsIgnoredUntilAttachedThenFreshReasonEmitsOnce() {
        val gate = TerminationPollGate()

        assertNull(gate.consume(4))

        gate.armAfterRendererAttached()

        assertNull(gate.consume(-1))
        assertEquals(5, gate.consume(5))
        assertNull(gate.consume(5))
    }

    @Test
    fun terminationEventContextMustStillBeCurrentAndActiveWhenQueued() {
        val queued = Any()

        assertTrue(isCurrentActiveTerminationContext(queued, queued, active = true))
        assertFalse(isCurrentActiveTerminationContext(null, queued, active = true))
        assertFalse(isCurrentActiveTerminationContext(Any(), queued, active = true))
        assertFalse(isCurrentActiveTerminationContext(queued, queued, active = false))
    }
}
