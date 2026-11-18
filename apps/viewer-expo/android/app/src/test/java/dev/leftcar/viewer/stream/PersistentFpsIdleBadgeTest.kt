package dev.leftcar.viewer.stream

import org.junit.Assert.*
import org.junit.Test

/**
 * HUD fps 배지의 유휴 판별(docs/2026-11-18-macos-promotion-idle-capture-cadence.md).
 * ProMotion 호스트의 정적 화면은 캡처 프레임 자체가 줄어 렌더 fps가 40~50으로
 * 떨어지지만 손실·갭은 없다 — 이 조합을 "유휴"로 표시해 성능 저하와 구분한다.
 */
class PersistentFpsIdleBadgeTest {
    private val start = HudFpsBadgeState()

    @Test
    fun `sustained low fps without loss enters idle after hold`() {
        var state = nextHudFpsBadgeState(start, displayedFps = 41.0, sourceFps = 60, lossDelta = 0, staleDelta = 0, nowMs = 1_000)
        assertEquals(HudFpsBadgeMode.ACTIVE, state.mode)
        state = nextHudFpsBadgeState(state, displayedFps = 41.0, sourceFps = 60, lossDelta = 0, staleDelta = 0, nowMs = 2_500)
        assertEquals(HudFpsBadgeMode.IDLE, state.mode)
    }

    @Test
    fun `loss evidence keeps the badge active even at low fps`() {
        var state = nextHudFpsBadgeState(start, displayedFps = 40.0, sourceFps = 60, lossDelta = 0, staleDelta = 0, nowMs = 1_000)
        state = nextHudFpsBadgeState(state, displayedFps = 40.0, sourceFps = 60, lossDelta = 0, staleDelta = 0, nowMs = 2_000)
        state = nextHudFpsBadgeState(state, displayedFps = 40.0, sourceFps = 60, lossDelta = 2, staleDelta = 0, nowMs = 3_000)
        assertEquals(HudFpsBadgeMode.ACTIVE, state.mode)
        assertNull(state.lowFpsSinceMs)
    }

    @Test
    fun `stale frame growth keeps the badge active`() {
        var state = nextHudFpsBadgeState(start, displayedFps = 40.0, sourceFps = 60, lossDelta = 0, staleDelta = 0, nowMs = 1_000)
        state = nextHudFpsBadgeState(state, displayedFps = 40.0, sourceFps = 60, lossDelta = 0, staleDelta = 1, nowMs = 3_000)
        assertEquals(HudFpsBadgeMode.ACTIVE, state.mode)
    }

    @Test
    fun `recovery above exit ratio returns to active`() {
        var state = nextHudFpsBadgeState(start, displayedFps = 41.0, sourceFps = 60, lossDelta = 0, staleDelta = 0, nowMs = 1_000)
        state = nextHudFpsBadgeState(state, displayedFps = 41.0, sourceFps = 60, lossDelta = 0, staleDelta = 0, nowMs = 2_500)
        assertEquals(HudFpsBadgeMode.IDLE, state.mode)
        state = nextHudFpsBadgeState(state, displayedFps = 58.0, sourceFps = 60, lossDelta = 0, staleDelta = 0, nowMs = 3_000)
        assertEquals(HudFpsBadgeMode.ACTIVE, state.mode)
    }

    @Test
    fun `middle band holds the previous mode with hysteresis`() {
        var state = nextHudFpsBadgeState(start, displayedFps = 41.0, sourceFps = 60, lossDelta = 0, staleDelta = 0, nowMs = 1_000)
        state = nextHudFpsBadgeState(state, displayedFps = 41.0, sourceFps = 60, lossDelta = 0, staleDelta = 0, nowMs = 2_500)
        assertEquals(HudFpsBadgeMode.IDLE, state.mode)
        state = nextHudFpsBadgeState(state, displayedFps = 52.0, sourceFps = 60, lossDelta = 0, staleDelta = 0, nowMs = 3_000)
        assertEquals("enter과 exit 사이는 상태를 유지한다", HudFpsBadgeMode.IDLE, state.mode)
    }

    @Test
    fun `unknown fps resets to active`() {
        var state = nextHudFpsBadgeState(start, displayedFps = 41.0, sourceFps = 60, lossDelta = 0, staleDelta = 0, nowMs = 1_000)
        state = nextHudFpsBadgeState(state, displayedFps = 41.0, sourceFps = 60, lossDelta = 0, staleDelta = 0, nowMs = 2_500)
        assertEquals(HudFpsBadgeMode.IDLE, state.mode)
        state = nextHudFpsBadgeState(state, displayedFps = null, sourceFps = 60, lossDelta = 0, staleDelta = 0, nowMs = 3_000)
        assertEquals(HudFpsBadgeMode.ACTIVE, state.mode)
    }

    @Test
    fun `badge text marks idle in korean and english`() {
        assertEquals("유휴(41 FPS)", persistentFpsBadgeText(41.0, HudFpsBadgeMode.IDLE, language = "ko"))
        assertEquals("IDLE(41 FPS)", persistentFpsBadgeText(41.0, HudFpsBadgeMode.IDLE, language = "en"))
        assertEquals("41 FPS", persistentFpsBadgeText(41.0, HudFpsBadgeMode.ACTIVE, language = "ko"))
        assertEquals("-- FPS", persistentFpsBadgeText(null, HudFpsBadgeMode.ACTIVE, language = "en"))
    }

    @Test
    fun `idle content description names the state`() {
        val ko = persistentFpsContentDescription(41.0, HudFpsBadgeMode.IDLE, language = "ko")
        val en = persistentFpsContentDescription(41.0, HudFpsBadgeMode.IDLE, language = "en")
        assertTrue(ko.contains("유휴"))
        assertTrue(en.contains("idle"))
    }
}
