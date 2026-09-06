package dev.leftcar.viewer.stream

import kotlin.math.roundToInt

internal data class StreamWindowBounds(
    val left: Int,
    val top: Int,
    val right: Int,
    val bottom: Int,
) {
    val width: Int get() = right - left
    val height: Int get() = bottom - top
}

internal fun sameAspectRatio(widthA: Int, heightA: Int, widthB: Int, heightB: Int): Boolean {
    if (widthA <= 0 || heightA <= 0 || widthB <= 0 || heightB <= 0) return false
    return widthA.toLong() * heightB.toLong() == widthB.toLong() * heightA.toLong()
}

internal fun initialStreamWindowBounds(
    sourceWidth: Int,
    sourceHeight: Int,
    availableWidth: Int,
    availableHeight: Int,
    insetLeft: Int = 0,
    insetTop: Int = 0,
    insetRight: Int = 0,
    insetBottom: Int = 0,
    maxFraction: Double = 0.9,
): StreamWindowBounds? {
    if (availableWidth <= 0 || availableHeight <= 0) return null
    val width = sourceWidth.toLong().coerceAtLeast(1L)
    val height = sourceHeight.toLong().coerceAtLeast(1L)
    val usableWidth = (availableWidth - insetLeft - insetRight).coerceAtLeast(1)
    val usableHeight = (availableHeight - insetTop - insetBottom).coerceAtLeast(1)
    val fraction = maxFraction.coerceIn(0.1, 1.0)
    val maxWidth = (usableWidth * fraction).roundToInt().coerceAtLeast(1)
    val maxHeight = (usableHeight * fraction).roundToInt().coerceAtLeast(1)
    var resultWidth = maxWidth.toLong()
    var resultHeight = (resultWidth * height / width).coerceAtLeast(1L)
    if (resultHeight > maxHeight) {
        resultHeight = maxHeight.toLong()
        resultWidth = (resultHeight * width / height).coerceAtLeast(1L)
    }
    val boundedWidth = resultWidth.coerceIn(1L, usableWidth.toLong()).toInt()
    val boundedHeight = resultHeight.coerceIn(1L, usableHeight.toLong()).toInt()
    val left = insetLeft + ((usableWidth - boundedWidth) / 2)
    val top = insetTop + ((usableHeight - boundedHeight) / 2)
    return StreamWindowBounds(left, top, left + boundedWidth, top + boundedHeight)
}

internal fun mapAspectFitPoint(
    x: Float,
    y: Float,
    viewWidth: Int,
    viewHeight: Int,
    sourceWidth: Int,
    sourceHeight: Int,
): Pair<Float, Float> {
    if (viewWidth <= 0 || viewHeight <= 0 || sourceWidth <= 0 || sourceHeight <= 0) {
        return 0f to 0f
    }
    val scale = minOf(viewWidth.toFloat() / sourceWidth, viewHeight.toFloat() / sourceHeight)
    val displayedWidth = sourceWidth * scale
    val displayedHeight = sourceHeight * scale
    val offsetX = (viewWidth - displayedWidth) / 2f
    val offsetY = (viewHeight - displayedHeight) / 2f
    return ((x - offsetX) / displayedWidth).coerceIn(0f, 1f) to
        ((y - offsetY) / displayedHeight).coerceIn(0f, 1f)
}
