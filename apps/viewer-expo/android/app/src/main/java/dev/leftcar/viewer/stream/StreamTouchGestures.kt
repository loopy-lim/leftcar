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
}

internal class TouchGestureStateMachine(
    private val touchSlopPx: Float,
    private val linesPerPixel: Float = DEFAULT_LINES_PER_PIXEL,
) {
    companion object {
        /** ~25 view pixels per wheel line: a full-height swipe scrolls a
         * screenful without making small gestures unusably coarse. */
        const val DEFAULT_LINES_PER_PIXEL = 1f / 25f
    }

    private enum class Phase {
        Idle,
        /** One finger down, left button held, long-press pending. */
        Tap,
        /** Two or more fingers, left button released, scrolling. */
        Scroll,
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

    /**
     * Feed one MotionEvent (with the centroid of all pointers precomputed).
     * Returns the commands to forward, in order.
     */
    fun onTouchEvent(
        actionMasked: Int,
        pointerCount: Int,
        pointerX: Float,
        pointerY: Float,
        allCentroidX: Float,
        allCentroidY: Float,
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
                    // The left press from the landing finger must end before
                    // scrolling, or the Mac would drag-select while wheeling.
                    return listOf(button(pointerX, pointerY, MotionEvent.BUTTON_PRIMARY, down = false))
                }
                if (phase == Phase.Scroll) {
                    centroidX = allCentroidX
                    centroidY = allCentroidY
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
                Phase.RightHold ->
                    return listOf(move(pointerX, pointerY, MotionEvent.BUTTON_SECONDARY))
                Phase.Idle, Phase.Remain -> return emptyList()
            }
            MotionEvent.ACTION_POINTER_UP -> {
                longPressPending = false
                if (phase == Phase.Scroll && pointerCount - 1 < 2) {
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
