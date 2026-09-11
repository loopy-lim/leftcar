package dev.leftcar.viewer.stream

import android.view.MotionEvent

/**
 * Touch-screen gesture state machine for the remote stream surface.
 *
 * One finger: tap = left click, drag = left drag. Press and hold without
 * moving converts the pending left click into a right click (down at
 * timeout, up at finger lift — movement after the timeout right-drags).
 * A second finger converts the gesture into a two-finger scroll: content
 * tracks the fingers, like a trackpad in natural scrolling.
 *
 * The machine is pure event logic: positions arrive in view pixels, the
 * owner normalizes and forwards the produced commands. Physical mouse and
 * stylus events never enter this machine.
 */
internal sealed interface TouchGestureCommand {
    /** Pointer moved to (x, y) view pixels with the given button mask held. */
    data class Move(val x: Float, val y: Float, val buttons: Int) : TouchGestureCommand

    data class Button(
        val x: Float,
        val y: Float,
        /** MotionEvent button constant: BUTTON_PRIMARY / BUTTON_SECONDARY. */
        val button: Int,
        val down: Boolean,
    ) : TouchGestureCommand

    /**
     * Scroll in wheel lines. Vertical follows content-tracks-fingers:
     * fingers up (negative dy) must reveal lower content, which the wheel
     * expresses as negative lines. Horizontal mirrors that on wheel2, whose
     * positive direction reveals right-hand content.
     */
    data class Scroll(val horizontalLines: Float, val verticalLines: Float) :
        TouchGestureCommand

    /**
     * 핀치줌 한 스텝. `factor`는 이벤트 간 span 비율(>1 벌림), (fx, fy)는
     * 현재 두 손가락 중심 — 포커스 고정 스케일이 중심을 따라가므로 별도
     * 팬 없이 줌인 중 이동도 자연스럽게 따라온다.
     */
    data class Zoom(val factor: Float, val focusX: Float, val focusY: Float) :
        TouchGestureCommand
}

internal class TouchGestureStateMachine(
    private val touchSlopPx: Float,
    private val linesPerPixel: Float = DEFAULT_LINES_PER_PIXEL,
) {
    companion object {
        /** ~25 view pixels per wheel line: a full-height swipe scrolls a
         * screenful without making small gestures unusably coarse. */
        const val DEFAULT_LINES_PER_PIXEL = 1f / 25f

        /**
         * 스크롤과 줌의 구분선: 두 손가락 span이 이 비율(18%)보다 많이
         * 변하면 줌으로 전환한다. 그 아래 요동은 재정렬·손떨림으로 본다.
         */
        const val ZOOM_ENTER_DRIFT = 0.18f
    }

    private enum class Phase {
        Idle,
        /** One finger down, left button held, long-press pending. */
        Tap,
        /** Two or more fingers, left button released, scrolling. */
        Scroll,
        /** Pinch in progress: zoom/pan, no remote input. */
        Zoom,
        /** Long-press fired: right button held until lift. */
        RightHold,
        /** Fingers lifted mid-scroll; remaining contact is inert. */
        Remain,
    }

    private var phase: Phase = Phase.Idle

    /**
     * True while the owner must keep the long-press timer armed. Cleared by
     * movement past the slop, a second finger, lift, or cancel.
     */
    var longPressPending: Boolean = false
        private set

    private var anchorX = 0f
    private var anchorY = 0f
    private var centroidX = 0f
    private var centroidY = 0f

    /** 두 손가락 span(거리) — 줌 제스처 판별과 배율 계산에 쓴다. */
    private var spanPx = 0f

    /** 줌 판정이 난 뒤의 기준 span(연속 배율 계산용). */
    private var zoomAnchorSpan = 0f

    /**
     * Feed one MotionEvent (with the centroid of all pointers precomputed and
     * the two-finger span, 0 when fewer than two pointers). Returns the
     * commands to forward, in order.
     */
    fun onTouchEvent(
        actionMasked: Int,
        pointerCount: Int,
        pointerX: Float,
        pointerY: Float,
        allCentroidX: Float,
        allCentroidY: Float,
        twoFingerSpanPx: Float = 0f,
    ): List<TouchGestureCommand> {
        when (actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                phase = Phase.Tap
                anchorX = pointerX
                anchorY = pointerY
                longPressPending = true
                return listOf(button(pointerX, pointerY, MotionEvent.BUTTON_PRIMARY, down = true))
            }
            MotionEvent.ACTION_POINTER_DOWN -> {
                longPressPending = false
                if (phase == Phase.Tap && pointerCount >= 2) {
                    phase = Phase.Scroll
                    centroidX = allCentroidX
                    centroidY = allCentroidY
                    spanPx = twoFingerSpanPx
                    // The left press from the landing finger must end before
                    // scrolling, or the Mac would drag-select while wheeling.
                    return listOf(button(pointerX, pointerY, MotionEvent.BUTTON_PRIMARY, down = false))
                }
                if (phase == Phase.Scroll || phase == Phase.Zoom) {
                    centroidX = allCentroidX
                    centroidY = allCentroidY
                    spanPx = twoFingerSpanPx
                    if (phase == Phase.Zoom) zoomAnchorSpan = twoFingerSpanPx
                }
                return emptyList()
            }
            MotionEvent.ACTION_MOVE -> when (phase) {
                Phase.Tap -> {
                    val dx = pointerX - anchorX
                    val dy = pointerY - anchorY
                    if (dx * dx + dy * dy > touchSlopPx * touchSlopPx) {
                        longPressPending = false
                    }
                    return listOf(move(pointerX, pointerY, MotionEvent.BUTTON_PRIMARY))
                }
                Phase.Scroll -> {
                    // 드리프트는 제스처 시작 span 기준으로 잰다 — 이벤트마다
                    // 갱신하면 느린 핀치(이벤트당 5%)가 스크롤로 오인된다.
                    if (twoFingerSpanPx > 0f && spanPx > 0f) {
                        val drift = kotlin.math.abs(twoFingerSpanPx - spanPx) / spanPx
                        if (drift > ZOOM_ENTER_DRIFT) {
                            // 벌리기/오므리기로 판명 — 이 제스처는 스크롤이
                            // 아니라 줌이다. 이후 손가락을 뗄 때까지 줌으로
                            // 남는다(중간에 스크롤과 섞이지 않게).
                            phase = Phase.Zoom
                            zoomAnchorSpan = twoFingerSpanPx
                            return emptyList()
                        }
                    }
                    val dx = allCentroidX - centroidX
                    val dy = allCentroidY - centroidY
                    centroidX = allCentroidX
                    centroidY = allCentroidY
                    if (dx == 0f && dy == 0f) return emptyList()
                    return listOf(
                        TouchGestureCommand.Scroll(
                            // -0f != 0f in data-class equality; normalize it.
                            horizontalLines = (-dx * linesPerPixel).zeroAsPositive(),
                            verticalLines = (dy * linesPerPixel).zeroAsPositive(),
                        )
                    )
                }
                Phase.Zoom -> {
                    if (twoFingerSpanPx <= 0f || zoomAnchorSpan <= 0f) return emptyList()
                    val factor = twoFingerSpanPx / zoomAnchorSpan
                    zoomAnchorSpan = twoFingerSpanPx
                    centroidX = allCentroidX
                    centroidY = allCentroidY
                    return listOf(
                        TouchGestureCommand.Zoom(
                            factor = factor.zeroAsPositive(),
                            focusX = allCentroidX,
                            focusY = allCentroidY,
                        )
                    )
                }
                Phase.RightHold ->
                    return listOf(move(pointerX, pointerY, MotionEvent.BUTTON_SECONDARY))
                Phase.Idle, Phase.Remain -> return emptyList()
            }
            MotionEvent.ACTION_POINTER_UP -> {
                longPressPending = false
                if ((phase == Phase.Scroll || phase == Phase.Zoom) && pointerCount - 1 < 2) {
                    phase = Phase.Remain
                }
                return emptyList()
            }
            MotionEvent.ACTION_UP -> when (phase) {
                Phase.Tap -> {
                    longPressPending = false
                    phase = Phase.Idle
                    return listOf(button(pointerX, pointerY, MotionEvent.BUTTON_PRIMARY, down = false))
                }
                Phase.RightHold -> {
                    phase = Phase.Idle
                    return listOf(button(pointerX, pointerY, MotionEvent.BUTTON_SECONDARY, down = false))
                }
                else -> {
                    phase = Phase.Idle
                    return emptyList()
                }
            }
            MotionEvent.ACTION_CANCEL -> {
                val commands = when (phase) {
                    Phase.Tap ->
                        listOf(button(pointerX, pointerY, MotionEvent.BUTTON_PRIMARY, down = false))
                    Phase.RightHold ->
                        listOf(button(pointerX, pointerY, MotionEvent.BUTTON_SECONDARY, down = false))
                    else -> emptyList()
                }
                phase = Phase.Idle
                longPressPending = false
                return commands
            }
            else -> return emptyList()
        }
    }

    /**
     * The owner invokes this when the long-press timer elapses: it converts
     * the held left click into a right press at the current position. The
     * right button stays down until the finger lifts (movement in between
     * right-drags, matching a Mac trackpad press-and-hold).
     */
    fun longPressFired(pointerX: Float, pointerY: Float): List<TouchGestureCommand> {
        if (phase != Phase.Tap || !longPressPending) return emptyList()
        longPressPending = false
        phase = Phase.RightHold
        return listOf(
            button(pointerX, pointerY, MotionEvent.BUTTON_PRIMARY, down = false),
            button(pointerX, pointerY, MotionEvent.BUTTON_SECONDARY, down = true),
        )
    }

    private fun move(x: Float, y: Float, buttons: Int) = TouchGestureCommand.Move(x, y, buttons)

    private fun button(x: Float, y: Float, button: Int, down: Boolean) =
        TouchGestureCommand.Button(x, y, button, down)

    private fun Float.zeroAsPositive(): Float = if (this == 0f) 0f else this
}
