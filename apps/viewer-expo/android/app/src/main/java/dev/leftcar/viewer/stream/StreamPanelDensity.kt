package dev.leftcar.viewer.stream

/**
 * 패널 폭 기반 UI 밀도 곡선. apps/viewer-expo/src/panel-density.ts가 같은
 * 곡선을 미러링한다 — 좁은 창(≤600dp)은 1.0으로 오늘의 폰 UI를 유지하고,
 * 600→1500dp에 걸쳐 선형 상승해 그 이상은 1.25 상한. 태블릿(≈900dp)은
 * 1.08 수준으로 폰 정체감을 유지하고 XR 대형 패널에서만 최대 배율 —
 * 최대 배지 텍스트 12sp × 1.25 = 15dp로 XR 가독 기준(1.75m에서 14dp)을
 * 계속 충족한다.
 */
internal object StreamPanelDensity {
    const val BASE_WIDTH_DP = 600f
    const val RAMP_WIDTH_DP = 900f
    const val MAX_SCALE = 1.25f

    fun scale(widthDp: Float): Float {
        if (!widthDp.isFinite() || widthDp <= 0f) return 1f
        if (widthDp <= BASE_WIDTH_DP) return 1f
        val ramp = ((widthDp - BASE_WIDTH_DP) / RAMP_WIDTH_DP).coerceIn(0f, 1f)
        return 1f + (MAX_SCALE - 1f) * ramp
    }

    /** 스트림 창(Activity)의 현재 폭에 대한 배율. */
    fun scaleOf(activity: android.app.Activity): Float =
        scale(activity.resources.configuration.screenWidthDp.toFloat())

    /** 밀도 배율을 곱한 dp→px. 최소 1px은 보장한다. */
    fun dp(value: Float, density: Float, scale: Float): Int =
        (value * density * scale).toInt().coerceAtLeast(1)
}
