package dev.leftcar.viewer.stream

import org.junit.Assert.assertEquals
import org.junit.Test

class StreamPanelDensityTest {
    @Test
    fun `600dp 이하에서는 배율 1을 유지한다`() {
        assertEquals(1f, StreamPanelDensity.scale(360f))
        assertEquals(1f, StreamPanelDensity.scale(600f))
    }

    @Test
    fun `완만한 선형 곡선을 따라 커지고 1_25에서 상한이 걸린다`() {
        // 태블릿 대역: 1.0 → 1.08 수준 — 폰 UI 정체감 유지
        assertEquals(1.0417f, StreamPanelDensity.scale(750f), 1e-4f)
        assertEquals(1.0833f, StreamPanelDensity.scale(900f), 1e-4f)
        assertEquals(1.1667f, StreamPanelDensity.scale(1200f), 1e-4f)
        assertEquals(1.25f, StreamPanelDensity.scale(1500f))
        assertEquals(1.25f, StreamPanelDensity.scale(2560f))
    }

    @Test
    fun `비정상 입력은 배율 1로 방어한다`() {
        assertEquals(1f, StreamPanelDensity.scale(0f))
        assertEquals(1f, StreamPanelDensity.scale(-10f))
        assertEquals(1f, StreamPanelDensity.scale(Float.NaN))
    }

    @Test
    fun `dp 계산은 배율을 곱하고 최소 1px을 보장한다`() {
        val density = 2f
        assertEquals(24, StreamPanelDensity.dp(12f, density, 1f))
        assertEquals(36, StreamPanelDensity.dp(12f, density, 1.5f))
        assertEquals(1, StreamPanelDensity.dp(0.1f, density, 1f))
    }
}
