package dev.leftcar.viewer.stream

import android.view.View

/**
 * 스트림 뷰의 핀치줌 상태. 어핀 변환 `view = content × scale + t`를 유지하고
 * 경계를 클램프해 줌인된 내용이 뷰를 항상 덮게 한다. 입력 좌표는
 * [toContent]로 역변환해 원격 포인터에 정확한 위치를 보낸다.
 *
 * 순수 상태 기계라 JVM 단위 테스트로 검증한다(뷰 적용은 [applyToView]만).
 */
internal class StreamZoomState {
    companion object {
        /** 최대 배율 — 6배면 텍스트 픽셀을 직접 들여다보기에 충분하다. */
        const val MAX_SCALE = 6f

        /** 이 값 미만으로 핀치를 닫으면 원상 복귀(스냅아웃)한다. */
        const val SNAP_OUT_SCALE = 1.02f

        /**
         * 분할 타일의 이음선 고정 피벗 X. 타일 중앙 피벗으로 같은 배율을
         * 적용하면 이음선 맞편이 (s−1)·w만큼 겹치므로, 왼쪽 타일은 오른쪽
         * 모서리(이음선), 오른쪽 타일은 왼쪽 모서리에 둔다.
         */
        fun seamPivotX(leftTile: Boolean, tileWidth: Int): Float =
            if (leftTile) tileWidth.toFloat() else 0f
    }

    var scale: Float = 1f
        private set
    var translationX: Float = 0f
        private set
    var translationY: Float = 0f
        private set

    val isZoomed: Boolean
        get() = scale > 1f

    /**
     * 포인트 (fx, fy) 아래의 내용이 그 자리에 머물게 배율을 바꾼다. 뷰
     * 크기가 필요하다 — 경계 클램프가 내용이 뷰를 벗어나지 않게 묶는다.
     */
    fun applyScale(factor: Float, focusX: Float, focusY: Float, viewWidth: Int, viewHeight: Int) {
        if (factor <= 0f || viewWidth <= 0 || viewHeight <= 0) return
        val next = (scale * factor).coerceIn(1f, MAX_SCALE)
        translationX = focusX - (focusX - translationX) * (next / scale)
        translationY = focusY - (focusY - translationY) * (next / scale)
        scale = next
        clamp(viewWidth, viewHeight)
        if (scale < SNAP_OUT_SCALE) reset()
    }

    fun reset() {
        scale = 1f
        translationX = 0f
        translationY = 0f
    }

    /** 뷰 좌표 → 줌 이전 내용 좌표(입력 역변환). */
    fun toContent(x: Float, y: Float): Pair<Float, Float> =
        ((x - translationX) / scale) to ((y - translationY) / scale)

    /** SurfaceView에 변환을 적용한다. 검은 레터박스도 함께 확대되지만,
     * 배율이 커질수록 비중이 줄어 들어 시각적으로 문제없다. */
    fun applyToView(view: View) {
        view.scaleX = scale
        view.scaleY = scale
        view.translationX = translationX
        view.translationY = translationY
    }

    /**
     * 분할 모드의 인접 타일 하나에 줌을 적용한다. 피벗을 [seamPivotX]처럼
     * 이음선 모서리에 두면 같은 배율·이동이 타일 두 개에서 이음선이 고정된
     * 하나의 연속 변환이 된다(가로는 이음선, 세로는 중앙 기준).
     */
    fun applyToTile(view: View, leftTile: Boolean) {
        view.pivotX = seamPivotX(leftTile, view.width)
        view.pivotY = view.height / 2f
        applyToView(view)
    }

    private fun clamp(viewWidth: Int, viewHeight: Int) {
        // 변환된 내용 사각형 [t, t + scale×size]가 [0, size]를 덮어야 한다.
        translationX = translationX.coerceIn((1f - scale) * viewWidth, 0f)
        translationY = translationY.coerceIn((1f - scale) * viewHeight, 0f)
    }
}
