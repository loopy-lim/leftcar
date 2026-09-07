package dev.leftcar.viewer.stream

/**
 * XR 창 비율 프리셋. 태블릿 카드에서 선택한 비율을 SpatialWindow에 그대로
 * 재적용한다. Mac 가상 화면 해상도는 이 값의 영향을 받지 않는다.
 */
internal enum class WindowAspectRatioPreset(val ratio: Float) {
    WIDE_16_10(1.6f),
    WIDE_16_9(16f / 9f),
    CLASSIC_4_3(4f / 3f),
    PORTRAIT_9_16(9f / 16f),
}

/**
 * 프리셋 비율을 소스 방향에 맞춰 보정한다. ratio ≥ 1(가로 창 지정)은 세로
 * 소스에서 뒤집혀 10:16 창이 되고, ratio < 1(세로 창 지정, 9:16)은 소스
 * 방향과 무관하게 세로 비율을 유지한다. 결과는 0.5~2.0으로 clamp된다.
 */
internal fun normalizedAspectRatio(
    sourceWidth: Int,
    sourceHeight: Int,
    preset: WindowAspectRatioPreset,
): Float = normalizedAspectRatio(sourceWidth, sourceHeight, preset.ratio)

/** 비율을 [MIN_RATIO, MAX_RATIO]로 clamp하고 세로/가로 대칭을 보정한다. */
internal fun normalizedAspectRatio(
    sourceWidth: Int,
    sourceHeight: Int,
    ratio: Float,
): Float {
    val clamped = ratio.coerceIn(MIN_RATIO, MAX_RATIO)
    val portraitSource = sourceWidth > 0 && sourceHeight > 0 && sourceHeight > sourceWidth
    val normalized = if (portraitSource && clamped > 1f) 1f / clamped else clamped
    return normalized.coerceIn(MIN_RATIO, MAX_RATIO)
}

private const val MIN_RATIO = 0.5f
private const val MAX_RATIO = 2.0f
