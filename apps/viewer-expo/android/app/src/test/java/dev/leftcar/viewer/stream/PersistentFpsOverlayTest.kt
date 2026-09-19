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
    fun windowShorterThanMinimumHoldsThePreviousDisplay() {
        val seeded = nextRenderedFpsSample(null, renderedFrames = 100L, nowMs = 1_000L)

        val sample = nextRenderedFpsSample(seeded, renderedFrames = 110L, nowMs = 1_250L)

        assertNull(sample.displayedFps)
    }

    @Test
    fun windowIntegratesTotalFramesOverTheAnchorSpan() {
        var sample = nextRenderedFpsSample(null, renderedFrames = 100L, nowMs = 1_000L)
        sample = nextRenderedFpsSample(sample, renderedFrames = 110L, nowMs = 1_250L)
        sample = nextRenderedFpsSample(sample, renderedFrames = 121L, nowMs = 1_500L)

        // 500ms 창 동안 +21프레임 = 42fps
        assertEquals(42.0, sample.displayedFps!!, 0.001)
        assertEquals("42 FPS", persistentFpsText(sample.displayedFps))
    }

    @Test
    fun rawSampleIsCappedAt240() {
        var sample = nextRenderedFpsSample(null, renderedFrames = 100L, nowMs = 1_000L)
        sample = nextRenderedFpsSample(sample, renderedFrames = 400L, nowMs = 1_250L)
        sample = nextRenderedFpsSample(sample, renderedFrames = 1_300L, nowMs = 1_500L)

        // 500ms 창 동안 +1,200프레임 → 상한 240fps
        assertEquals(240.0, sample.displayedFps!!, 0.001)
    }

    @Test
    fun counterResetShowsUnknownUntilTheWindowRebuilds() {
        val seeded = nextRenderedFpsSample(null, renderedFrames = 100L, nowMs = 1_000L)
        val live = nextRenderedFpsSample(seeded, renderedFrames = 110L, nowMs = 1_250L)
        val live2 = nextRenderedFpsSample(live, renderedFrames = 121L, nowMs = 1_500L)
        assertEquals(42.0, live2.displayedFps!!, 0.001)

        val reset = nextRenderedFpsSample(live2, renderedFrames = 3L, nowMs = 1_750L)
        val resumed1 = nextRenderedFpsSample(reset, renderedFrames = 13L, nowMs = 2_000L)
        val resumed2 = nextRenderedFpsSample(resumed1, renderedFrames = 23L, nowMs = 2_250L)

        assertNull(reset.displayedFps)
        assertEquals("-- FPS", persistentFpsText(reset.displayedFps))
        assertNull(resumed1.displayedFps)
        assertEquals(40.0, resumed2.displayedFps!!, 0.001)
    }

    @Test
    fun fullyStalledWindowDisplaysZeroInsteadOfHoldingTheStaleRate() {
        var sample = nextRenderedFpsSample(null, renderedFrames = 100L, nowMs = 1_000L)
        sample = nextRenderedFpsSample(sample, renderedFrames = 100L, nowMs = 1_250L)
        sample = nextRenderedFpsSample(sample, renderedFrames = 100L, nowMs = 1_500L)

        assertEquals(0.0, sample.displayedFps!!, 0.001)
        assertEquals("0 FPS", persistentFpsText(sample.displayedFps))
    }

    @Test
    fun deadTickDilutesInsideTheWindowAndExitsWithoutEmaStickiness() {
        var sample = nextRenderedFpsSample(null, renderedFrames = 0L, nowMs = 0L)
        var frames = 0L
        fun tick(nowMs: Long, delta: Long) {
            frames += delta
            sample = nextRenderedFpsSample(sample, renderedFrames = frames, nowMs = nowMs)
        }
        for (offset in 1..8) tick(offset * 250L, 14L)
        assertEquals(56.0, sample.displayedFps!!, 0.001)

        // 죽은 틱 하나: EMA(0.35 가중)라면 즉시 ~36까지 떨어졌다. 창 적분은
        // 창 안에서 희석될 뿐이다.
        tick(2_250L, 0L)
        tick(2_500L, 14L)
        assertEquals(48.0, sample.displayedFps!!, 0.001)

        // 죽은 틱이 창에서 나가면 정상 속도로 완전히 복귀한다.
        tick(2_750L, 14L)
        tick(3_000L, 14L)
        tick(3_250L, 14L)
        tick(3_500L, 14L)
        tick(3_750L, 14L)
        assertEquals(56.0, sample.displayedFps!!, 0.001)
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
    fun `fps 접근성 문구는 언어를 따라 영어로 바뀐다`() {
        assertEquals("Actual render rate 41 FPS", persistentFpsContentDescription(40.6, "en"))
        assertEquals("Actual render rate -- FPS", persistentFpsContentDescription(null, "en"))
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
