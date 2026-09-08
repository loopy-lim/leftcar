package dev.leftcar.viewer.stream

import android.view.MotionEvent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class StreamTouchGesturesTest {
    private val slop = 12f
    private val machine = TouchGestureStateMachine(
        touchSlopPx = slop,
        linesPerPixel = 0.04f,
    )

    private fun button(
        x: Float,
        y: Float,
        button: Int,
        down: Boolean,
    ) = TouchGestureCommand.Button(x, y, button, down)

    @Test
    fun `tap is a left press and release`() {
        val down = machine.onTouchEvent(MotionEvent.ACTION_DOWN, 1, 100f, 200f, 100f, 200f)
        assertEquals(listOf(button(100f, 200f, MotionEvent.BUTTON_PRIMARY, down = true)), down)
        assertTrue(machine.longPressPending)

        val up = machine.onTouchEvent(MotionEvent.ACTION_UP, 1, 100f, 200f, 100f, 200f)
        assertEquals(listOf(button(100f, 200f, MotionEvent.BUTTON_PRIMARY, down = false)), up)
        assertFalse(machine.longPressPending)
    }

    @Test
    fun `movement beyond slop cancels the long press but keeps left drag`() {
        machine.onTouchEvent(MotionEvent.ACTION_DOWN, 1, 100f, 200f, 100f, 200f)
        val small = machine.onTouchEvent(MotionEvent.ACTION_MOVE, 1, 105f, 203f, 105f, 203f)
        assertEquals(
            listOf(TouchGestureCommand.Move(105f, 203f, MotionEvent.BUTTON_PRIMARY)),
            small,
        )
        assertTrue(machine.longPressPending)

        val far = machine.onTouchEvent(MotionEvent.ACTION_MOVE, 1, 130f, 200f, 130f, 200f)
        assertEquals(
            listOf(TouchGestureCommand.Move(130f, 200f, MotionEvent.BUTTON_PRIMARY)),
            far,
        )
        assertFalse(machine.longPressPending)
    }

    @Test
    fun `long press converts left hold into right press and lift releases it`() {
        machine.onTouchEvent(MotionEvent.ACTION_DOWN, 1, 100f, 200f, 100f, 200f)
        val fired = machine.longPressFired(102f, 201f)
        assertEquals(
            listOf(
                button(102f, 201f, MotionEvent.BUTTON_PRIMARY, down = false),
                button(102f, 201f, MotionEvent.BUTTON_SECONDARY, down = true),
            ),
            fired,
        )
        assertFalse(machine.longPressPending)

        val drag = machine.onTouchEvent(MotionEvent.ACTION_MOVE, 1, 140f, 210f, 140f, 210f)
        assertEquals(
            listOf(TouchGestureCommand.Move(140f, 210f, MotionEvent.BUTTON_SECONDARY)),
            drag,
        )

        val up = machine.onTouchEvent(MotionEvent.ACTION_UP, 1, 140f, 210f, 140f, 210f)
        assertEquals(listOf(button(140f, 210f, MotionEvent.BUTTON_SECONDARY, down = false)), up)
    }

    @Test
    fun `long press after slop movement is inert`() {
        machine.onTouchEvent(MotionEvent.ACTION_DOWN, 1, 100f, 200f, 100f, 200f)
        machine.onTouchEvent(MotionEvent.ACTION_MOVE, 1, 160f, 200f, 160f, 200f)
        assertTrue(machine.longPressFired(160f, 200f).isEmpty())
    }

    @Test
    fun `second finger ends the left press and starts content-tracks-fingers scroll`() {
        machine.onTouchEvent(MotionEvent.ACTION_DOWN, 1, 100f, 200f, 100f, 200f)
        val second = machine.onTouchEvent(MotionEvent.ACTION_POINTER_DOWN, 2, 100f, 200f, 150f, 250f)
        assertEquals(
            listOf(button(100f, 200f, MotionEvent.BUTTON_PRIMARY, down = false)),
            second,
        )

        val scroll = machine.onTouchEvent(MotionEvent.ACTION_MOVE, 2, 100f, 200f, 150f, 220f)
        assertEquals(
            listOf(
                TouchGestureCommand.Scroll(horizontalLines = 0f, verticalLines = -30f * 0.04f),
            ),
            scroll,
        )

        val diagonal = machine.onTouchEvent(MotionEvent.ACTION_MOVE, 2, 100f, 200f, 180f, 250f)
        assertEquals(
            listOf(
                TouchGestureCommand.Scroll(horizontalLines = -30f * 0.04f, verticalLines = 30f * 0.04f),
            ),
            diagonal,
        )
    }

    @Test
    fun `scroll ignores zero-delta moves and stops when a finger lifts`() {
        machine.onTouchEvent(MotionEvent.ACTION_DOWN, 1, 100f, 200f, 100f, 200f)
        machine.onTouchEvent(MotionEvent.ACTION_POINTER_DOWN, 2, 100f, 200f, 150f, 250f)
        val still = machine.onTouchEvent(MotionEvent.ACTION_MOVE, 2, 100f, 200f, 150f, 250f)
        assertTrue(still.isEmpty())

        machine.onTouchEvent(MotionEvent.ACTION_MOVE, 2, 100f, 200f, 150f, 240f)
        val lift = machine.onTouchEvent(MotionEvent.ACTION_POINTER_UP, 1, 100f, 200f, 150f, 240f)
        assertTrue(lift.isEmpty())
        val lateMove = machine.onTouchEvent(MotionEvent.ACTION_MOVE, 1, 150f, 240f, 150f, 240f)
        assertTrue(lateMove.isEmpty())
        val up = machine.onTouchEvent(MotionEvent.ACTION_UP, 1, 150f, 240f, 150f, 240f)
        assertTrue(up.isEmpty())
    }

    @Test
    fun `cancel releases whatever button is held`() {
        machine.onTouchEvent(MotionEvent.ACTION_DOWN, 1, 100f, 200f, 100f, 200f)
        val cancel = machine.onTouchEvent(MotionEvent.ACTION_CANCEL, 1, 100f, 200f, 100f, 200f)
        assertEquals(listOf(button(100f, 200f, MotionEvent.BUTTON_PRIMARY, down = false)), cancel)

        machine.onTouchEvent(MotionEvent.ACTION_DOWN, 1, 50f, 60f, 50f, 60f)
        machine.longPressFired(50f, 60f)
        val rightCancel = machine.onTouchEvent(MotionEvent.ACTION_CANCEL, 1, 50f, 60f, 50f, 60f)
        assertEquals(listOf(button(50f, 60f, MotionEvent.BUTTON_SECONDARY, down = false)), rightCancel)
    }
}
