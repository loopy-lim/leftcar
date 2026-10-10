package dev.leftcar.viewer.km

import android.view.InputDevice
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** [KmMotionSources] 는 터치스크린·광역 마스크를 구독하지 않음을 고정한다. */
class KmMotionSourcesTest {
    @Test
    fun `mouse family mask is exactly mouse sources`() {
        assertEquals(
            InputDevice.SOURCE_MOUSE or
                InputDevice.SOURCE_MOUSE_RELATIVE or
                InputDevice.SOURCE_TOUCHPAD,
            KmMotionSources.MOUSE_FAMILY,
        )
    }

    @Test
    fun `mask contains no touchscreen device bits`() {
        val touchscreenDeviceBits =
            InputDevice.SOURCE_TOUCHSCREEN and InputDevice.SOURCE_CLASS_MASK.inv()
        assertEquals(0, KmMotionSources.MOUSE_FAMILY and touchscreenDeviceBits)
        // 마우스 계열 3 소스의 device bits 만 포함한다.
        val familyDeviceBits = listOf(
            InputDevice.SOURCE_MOUSE,
            InputDevice.SOURCE_MOUSE_RELATIVE,
            InputDevice.SOURCE_TOUCHPAD,
        ).fold(0) { acc, source -> acc or (source and InputDevice.SOURCE_CLASS_MASK.inv()) }
        assertEquals(
            familyDeviceBits,
            KmMotionSources.MOUSE_FAMILY and InputDevice.SOURCE_CLASS_MASK.inv(),
        )
    }

    @Test
    fun `isMouseFamily accepts mouse sources and rejects touchscreen garbage and empty`() {
        assertTrue(KmMotionSources.isMouseFamily(InputDevice.SOURCE_MOUSE))
        assertTrue(KmMotionSources.isMouseFamily(InputDevice.SOURCE_MOUSE_RELATIVE))
        assertTrue(KmMotionSources.isMouseFamily(InputDevice.SOURCE_TOUCHPAD))

        assertFalse(KmMotionSources.isMouseFamily(InputDevice.SOURCE_TOUCHSCREEN))
        assertFalse(KmMotionSources.isMouseFamily(InputDevice.SOURCE_STYLUS))
        assertFalse(KmMotionSources.isMouseFamily(InputDevice.SOURCE_CLASS_POINTER))
        assertFalse(KmMotionSources.isMouseFamily(0))
        assertFalse(KmMotionSources.isMouseFamily(0x1000)) // touchscreen device bits only
    }

    @Test
    fun `sanitize keeps only mouse family bits`() {
        assertEquals(
            KmMotionSources.MOUSE_FAMILY,
            KmMotionSources.sanitize(KmMotionSources.MOUSE_FAMILY or 0xF000_0000.toInt()),
        )
        assertEquals(InputDevice.SOURCE_MOUSE, KmMotionSources.sanitize(InputDevice.SOURCE_MOUSE))
        assertEquals(0, KmMotionSources.sanitize(InputDevice.SOURCE_TOUCHSCREEN))
        assertEquals(0, KmMotionSources.sanitize(0))
        assertEquals(0, KmMotionSources.sanitize(InputDevice.SOURCE_CLASS_POINTER))
        // -1 (모든 비트) → 마우스 계열 전체로 제한
        assertEquals(KmMotionSources.MOUSE_FAMILY, KmMotionSources.sanitize(-1))
    }

    @Test
    fun `motion events supported only at api 34 plus`() {
        assertFalse(KmMotionSources.supportsMotionEvents(33))
        assertTrue(KmMotionSources.supportsMotionEvents(34))
        assertTrue(KmMotionSources.supportsMotionEvents(35))
        assertTrue(KmMotionSources.supportsMotionEvents(36))
    }
}
