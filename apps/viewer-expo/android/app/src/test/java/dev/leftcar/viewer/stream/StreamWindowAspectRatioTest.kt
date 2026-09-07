package dev.leftcar.viewer.stream

import org.junit.Assert.assertEquals
import org.junit.Test

class StreamWindowAspectRatioTest {
    @Test fun landscapePresetsKeepDeclaredRatio() {
        assertEquals(1.6f, normalizedAspectRatio(1920, 1200, WindowAspectRatioPreset.WIDE_16_10), 0.001f)
        assertEquals(16f / 9f, normalizedAspectRatio(1920, 1080, WindowAspectRatioPreset.WIDE_16_9), 0.001f)
        assertEquals(4f / 3f, normalizedAspectRatio(1920, 1080, WindowAspectRatioPreset.CLASSIC_4_3), 0.001f)
    }

    @Test fun portraitPresetStaysPortraitOnAnySource() {
        // 9:16은 세로 창 지정 — 소스 방향과 무관하게 세로 비율을 유지한다.
        assertEquals(9f / 16f, normalizedAspectRatio(1920, 1080, WindowAspectRatioPreset.PORTRAIT_9_16), 0.001f)
        assertEquals(9f / 16f, normalizedAspectRatio(2800, 1752, WindowAspectRatioPreset.PORTRAIT_9_16), 0.001f)
        assertEquals(9f / 16f, normalizedAspectRatio(1080, 1920, WindowAspectRatioPreset.PORTRAIT_9_16), 0.001f)
    }

    @Test fun portraitSourceFlipsLandscapePresets() {
        // 세로 소스(태블릿 세로 화면)는 가로 프리셋 비율을 뒤집어 적용한다.
        assertEquals(1f / 1.6f, normalizedAspectRatio(1080, 1920, WindowAspectRatioPreset.WIDE_16_10), 0.001f)
        assertEquals(3f / 4f, normalizedAspectRatio(1080, 2400, WindowAspectRatioPreset.CLASSIC_4_3), 0.001f)
    }

    @Test fun ratioStaysInsideClampWindow() {
        // 0.5~2.0 clamp: 프리셋 비율은 항상 이 범위 안에 머문다.
        for (preset in WindowAspectRatioPreset.entries) {
            val landscape = normalizedAspectRatio(1920, 1080, preset)
            val portrait = normalizedAspectRatio(1080, 1920, preset)
            for (ratio in listOf(landscape, portrait)) {
                assert(ratio >= 0.5f) { "$preset landscape ratio $ratio below clamp" }
                assert(ratio <= 2.0f) { "$preset ratio $ratio above clamp" }
            }
        }
    }

    @Test fun degenerateSourceFallsBackToPresetRatio() {
        // 0·음수 소스는 프리셋 비율 자체로 폴백한다.
        assertEquals(1.6f, normalizedAspectRatio(0, 1080, WindowAspectRatioPreset.WIDE_16_10), 0.001f)
        assertEquals(1.6f, normalizedAspectRatio(1920, -3, WindowAspectRatioPreset.WIDE_16_10), 0.001f)
    }

    @Test fun clampsDirectlyProvidedRatioValues() {
        assertEquals(0.5f, normalizedAspectRatio(1920, 1080, 0.1f), 0.001f)
        assertEquals(2.0f, normalizedAspectRatio(1920, 1080, 9f), 0.001f)
        assertEquals(1.6f, normalizedAspectRatio(1920, 1080, 1.6f), 0.001f)
    }

    @Test fun symmetricSourcesShareRatioMagnitude() {
        // 세로/가로 대칭성: 같은 픽셀 수의 세로·가로 소스는 서로 뒤집힌 비율.
        val landscape = normalizedAspectRatio(1920, 1200, WindowAspectRatioPreset.WIDE_16_9)
        val portrait = normalizedAspectRatio(1200, 1920, WindowAspectRatioPreset.WIDE_16_9)
        assertEquals(1f, landscape * portrait, 0.001f)
    }
}
