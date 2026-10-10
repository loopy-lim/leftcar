package dev.leftcar.viewer.km

import android.view.InputDevice
import android.view.MotionEvent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/** [KmMotionSample.from] 의 이벤트 정규화 계약. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE)
class KmMotionSampleTest {
    private fun pointerEvent(
        action: Int,
        source: Int,
        x: Float = 300f,
        y: Float = 200f,
        scroll: Float? = null,
        buttons: Int = 0,
    ): MotionEvent {
        val properties = MotionEvent.PointerProperties().apply {
            id = 0
            toolType = MotionEvent.TOOL_TYPE_MOUSE
        }
        val coords = MotionEvent.PointerCoords().apply {
            this.x = x
            this.y = y
            scroll?.let { setAxisValue(MotionEvent.AXIS_VSCROLL, it) }
        }
        return MotionEvent.obtain(
            10, 20, action, 1,
            arrayOf(properties), arrayOf(coords),
            0, buttons, 1f, 1f, 3, 0, source, 0,
        )
    }

    @Test
    fun `null event is safely ignored`() {
        assertNull(KmMotionSample.from(null))
    }

    @Test
    fun `hover move is normalized with source and coordinates`() {
        val sample = KmMotionSample.from(
            pointerEvent(MotionEvent.ACTION_HOVER_MOVE, InputDevice.SOURCE_MOUSE, x = 120f, y = 40f),
        )!!
        assertEquals(InputDevice.SOURCE_MOUSE, sample.source)
        assertEquals(MotionEvent.ACTION_HOVER_MOVE, sample.action)
        assertEquals(120f, sample.x)
        assertEquals(40f, sample.y)
        assertEquals(0f, sample.scroll)
        assertEquals(0, sample.buttons)
        assertEquals(10L, sample.downTimeMs)
        assertEquals(20L, sample.eventTimeMs)
    }

    @Test
    fun `button press captures button state`() {
        val sample = KmMotionSample.from(
            pointerEvent(
                MotionEvent.ACTION_BUTTON_PRESS,
                InputDevice.SOURCE_MOUSE,
                buttons = MotionEvent.BUTTON_PRIMARY,
            ),
        )!!
        assertEquals(MotionEvent.ACTION_BUTTON_PRESS, sample.action)
        assertEquals(MotionEvent.BUTTON_PRIMARY, sample.buttons)
    }

    @Test
    fun `scroll captures vertical axis value`() {
        val sample = KmMotionSample.from(
            pointerEvent(MotionEvent.ACTION_SCROLL, InputDevice.SOURCE_MOUSE, scroll = -2.5f),
        )!!
        assertEquals(MotionEvent.ACTION_SCROLL, sample.action)
        assertEquals(-2.5f, sample.scroll)
    }

    @Test
    fun `pointer index bits are masked out of action`() {
        val indexedAction =
            MotionEvent.ACTION_POINTER_DOWN or (2 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT)
        val sample = KmMotionSample.from(pointerEvent(indexedAction, InputDevice.SOURCE_TOUCHPAD))!!
        assertEquals(MotionEvent.ACTION_POINTER_DOWN, sample.action)
        assertEquals(InputDevice.SOURCE_TOUCHPAD, sample.source)
    }
}
