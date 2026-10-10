package dev.leftcar.viewer.km

import android.view.MotionEvent

/**
 * [AccessibilityService.onMotionEvent] 로 들어온 제네릭 [MotionEvent]의 불변 스냅샷.
 * 프레임워크 타입을 포함하지 않아 순수 JVM 테스트 대상이다.
 */
data class KmMotionSample(
    val source: Int,
    /** [MotionEvent.getActionMasked] 로 정규화된 액션. */
    val action: Int,
    val x: Float,
    val y: Float,
    /** AXIS_VSCROLL 값(휠). 없으면 0. */
    val scroll: Float,
    /** [MotionEvent.getButtonState] */
    val buttons: Int,
    val downTimeMs: Long,
    val eventTimeMs: Long,
) {
    companion object {
        /** null·비정상 이벤트는 안전하게 무시한다. */
        fun from(event: MotionEvent?): KmMotionSample? {
            if (event == null) return null
            return runCatching {
                KmMotionSample(
                    source = event.source,
                    action = event.actionMasked,
                    x = event.x,
                    y = event.y,
                    scroll = event.getAxisValue(MotionEvent.AXIS_VSCROLL),
                    buttons = event.buttonState,
                    downTimeMs = event.downTime,
                    eventTimeMs = event.eventTime,
                )
            }.getOrNull()
        }
    }
}
