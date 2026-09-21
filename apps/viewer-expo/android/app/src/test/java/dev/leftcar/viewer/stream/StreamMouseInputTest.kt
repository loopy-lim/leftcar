package dev.leftcar.viewer.stream

import android.content.Intent
import android.view.KeyEvent
import android.view.InputDevice
import android.view.MotionEvent
import android.view.View
import org.junit.Assert.*
import org.junit.After
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.util.ReflectionHelpers
import org.robolectric.util.ReflectionHelpers.ClassParameter.from

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE, shadows = [PresentationNativeShadow::class, ValidSurfaceShadow::class, PresentationSurfaceViewShadow::class])
class StreamMouseInputTest {
    @After fun resetInputGate() { PresentationNativeShadow.inputEnabled = 0 }

    /**
     * 입력 소유권 아키텍처(6a4ae1b)에서 물리 마우스·하드웨어 키는 KeyBridge
     * 획득 → 포인터 캡처 확인 후에만 원격으로 승격된다. Robolectric에서는
     * KeyBridge 바인딩이 즉시 실패해 실제 핸드셰이크를 완주할 수 없으므로,
     * 컨트롤러를 원격 상태로 직접 승격한 뒤 원격 라우팅 동작을 검증한다.
     */
    private fun promoteToRemoteMouse(activity: StreamActivity) {
        val ownership = ReflectionHelpers.getField<InputOwnershipController>(activity, "inputOwnership")
        ReflectionHelpers.setField(ownership, "owner", InputOwner.REMOTE_MAC)
    }

    private fun event(action: Int, buttons: Int, button: Int = 0): MotionEvent {
        val properties = MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_MOUSE }
        val coordinates = MotionEvent.PointerCoords().apply { x = 300f; y = 200f; pressure = if (buttons == 0) 0f else 1f }
        return MotionEvent.obtain(1, 2, action, 1, arrayOf(properties), arrayOf(coordinates), 0, buttons, 1f, 1f, 3, 0, InputDevice.SOURCE_MOUSE, 0).also {
            ReflectionHelpers.callInstanceMethod<Void>(it, "setActionButton", from(Int::class.javaPrimitiveType!!, button))
        }
    }

    private fun touchpadScroll(action: Int, dx: Float, dy: Float): MotionEvent {
        val properties = MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_FINGER }
        val coordinates = MotionEvent.PointerCoords().apply {
            x = 300f; y = 200f
            setAxisValue(MotionEvent.AXIS_GESTURE_SCROLL_X_DISTANCE, dx)
            setAxisValue(MotionEvent.AXIS_GESTURE_SCROLL_Y_DISTANCE, dy)
        }
        return requireNotNull(MotionEvent.obtain(1, 2, action, 1, arrayOf(properties), arrayOf(coordinates), 0, 0, 1f, 1f, 3, 0, InputDevice.SOURCE_MOUSE, 0, 0, MotionEvent.CLASSIFICATION_TWO_FINGER_SWIPE))
    }

    @Test fun `Lenovo classified scroll without distance axes uses gesture position changes and resets between gestures`() {
        PresentationNativeShadow.inputEnabled = 1
        PresentationNativeShadow.pointerActions.clear()
        PresentationNativeShadow.scrollDeltas.clear()
        val controller = Robolectric.buildActivity(StreamActivity::class.java, Intent().putExtra("instance", "lenovo-scroll").putExtra("host", "192.168.0.2")).create().start().resume()
        val surfaces = ReflectionHelpers.getField<StreamSurfaces>(controller.get(), "streamSurfaces")
        surfaces.root.measure(View.MeasureSpec.makeMeasureSpec(1920, View.MeasureSpec.EXACTLY), View.MeasureSpec.makeMeasureSpec(1080, View.MeasureSpec.EXACTLY))
        surfaces.root.layout(0, 0, 1920, 1080)
        surfaces.root.dispatchTouchEvent(touchpadScroll(MotionEvent.ACTION_DOWN, 0f, 0f))
        val move = touchpadScroll(MotionEvent.ACTION_MOVE, 0f, 0f).apply { setLocation(312f, 218f) }
        move.addBatch(3, arrayOf(MotionEvent.PointerCoords().apply { x = 318f; y = 224f }), 0)
        surfaces.root.dispatchTouchEvent(move)
        surfaces.root.dispatchTouchEvent(touchpadScroll(MotionEvent.ACTION_UP, 0f, 0f))
        val restart = touchpadScroll(MotionEvent.ACTION_DOWN, 0f, 0f).apply { setLocation(900f, 700f) }
        surfaces.root.dispatchTouchEvent(restart)
        surfaces.root.dispatchTouchEvent(touchpadScroll(MotionEvent.ACTION_MOVE, 0f, 0f).apply { setLocation(900f, 700f) })
        surfaces.root.dispatchTouchEvent(touchpadScroll(MotionEvent.ACTION_CANCEL, 0f, 0f))
        assertEquals(listOf(4 to 0), PresentationNativeShadow.pointerActions)
        assertEquals(-0.72f, PresentationNativeShadow.scrollDeltas.single().first, 0.001f)
        assertEquals(-0.96f, PresentationNativeShadow.scrollDeltas.single().second, 0.001f)
        controller.pause().stop().destroy()
    }

    @Test fun `classified touchpad scrolling forwards accumulated gesture distance without moving or clicking the pointer`() {
        PresentationNativeShadow.inputEnabled = 1
        PresentationNativeShadow.pointerActions.clear()
        PresentationNativeShadow.scrollDeltas.clear()
        val controller = Robolectric.buildActivity(StreamActivity::class.java, Intent().putExtra("instance", "touchpad-scroll").putExtra("host", "192.168.0.2")).create().start().resume()
        val surfaces = ReflectionHelpers.getField<StreamSurfaces>(controller.get(), "streamSurfaces")
        surfaces.root.measure(View.MeasureSpec.makeMeasureSpec(1920, View.MeasureSpec.EXACTLY), View.MeasureSpec.makeMeasureSpec(1080, View.MeasureSpec.EXACTLY))
        surfaces.root.layout(0, 0, 1920, 1080)
        assertTrue(surfaces.root.dispatchTouchEvent(touchpadScroll(MotionEvent.ACTION_DOWN, 0f, 0f)))
        val move = touchpadScroll(MotionEvent.ACTION_MOVE, 8f, 12f)
        val latest = MotionEvent.PointerCoords().apply {
            x = 320f; y = 240f
            setAxisValue(MotionEvent.AXIS_GESTURE_SCROLL_X_DISTANCE, 4f)
            setAxisValue(MotionEvent.AXIS_GESTURE_SCROLL_Y_DISTANCE, 6f)
        }
        move.addBatch(3, arrayOf(latest), 0)
        assertEquals(1, move.historySize)
        assertTrue(surfaces.root.dispatchTouchEvent(move))
        assertTrue(surfaces.root.dispatchTouchEvent(touchpadScroll(MotionEvent.ACTION_UP, 0f, 0f)))
        assertEquals(listOf(4 to 0), PresentationNativeShadow.pointerActions)
        assertEquals(0.48f, PresentationNativeShadow.scrollDeltas.single().first, 0.001f)
        assertEquals(0.72f, PresentationNativeShadow.scrollDeltas.single().second, 0.001f)
        controller.pause().stop().destroy()
    }

    @Test fun `middle mouse button keeps its tertiary identity on press and release`() {
        PresentationNativeShadow.inputEnabled = 1
        PresentationNativeShadow.pointerButtons.clear()
        val controller = Robolectric.buildActivity(StreamActivity::class.java, Intent().putExtra("instance", "middle-click").putExtra("host", "192.168.0.2")).create().start().resume()
        val surfaces = ReflectionHelpers.getField<StreamSurfaces>(controller.get(), "streamSurfaces")
        promoteToRemoteMouse(controller.get())
        assertTrue(surfaces.left.dispatchGenericMotionEvent(event(MotionEvent.ACTION_BUTTON_PRESS, 4, 4)))
        assertTrue(surfaces.left.dispatchGenericMotionEvent(event(MotionEvent.ACTION_BUTTON_RELEASE, 0, 4)))
        assertEquals(listOf(Triple(2, 4, 4), Triple(3, 4, 0)), PresentationNativeShadow.pointerButtons)
        controller.pause().stop().destroy()
    }

    @Test fun `lowercase hardware keys retain down repeat and up instead of becoming text taps`() {
        PresentationNativeShadow.inputEnabled = 1
        PresentationNativeShadow.keyEvents.clear()
        PresentationNativeShadow.textEvents.clear()
        val controller = Robolectric.buildActivity(StreamActivity::class.java, Intent().putExtra("instance", "hardware-key-hold").putExtra("host", "192.168.0.2")).create().start().resume()
        promoteToRemoteMouse(controller.get())
        for ((action, repeat) in listOf(KeyEvent.ACTION_DOWN to 0, KeyEvent.ACTION_DOWN to 1, KeyEvent.ACTION_UP to 0)) {
            assertTrue(controller.get().dispatchKeyEvent(KeyEvent(1, 2, action, KeyEvent.KEYCODE_A, repeat, 0, -1, 30, 0, InputDevice.SOURCE_KEYBOARD)))
        }
        assertEquals(listOf(Triple(KeyEvent.KEYCODE_A, true, 0), Triple(KeyEvent.KEYCODE_A, true, 1), Triple(KeyEvent.KEYCODE_A, false, 0)), PresentationNativeShadow.keyEvents)
        assertTrue(PresentationNativeShadow.textEvents.isEmpty())
        controller.pause().stop().destroy()
    }

    @Test fun `mouse touch dispatch retains drag without duplicating generic button edges`() {
        PresentationNativeShadow.pointerActions.clear()
        PresentationNativeShadow.inputEnabled = 1
        val controller = Robolectric.buildActivity(StreamActivity::class.java, Intent().putExtra("instance", "mouse-drag").putExtra("host", "192.168.0.2")).create().start().resume()
        val surfaces = ReflectionHelpers.getField<StreamSurfaces>(controller.get(), "streamSurfaces")
        surfaces.root.measure(View.MeasureSpec.makeMeasureSpec(1920, View.MeasureSpec.EXACTLY), View.MeasureSpec.makeMeasureSpec(1080, View.MeasureSpec.EXACTLY))
        surfaces.root.layout(0, 0, 1920, 1080)
        promoteToRemoteMouse(controller.get())
        assertTrue("DOWN must retain the Surface as the drag gesture target", surfaces.root.dispatchTouchEvent(event(MotionEvent.ACTION_DOWN, 1)))
        assertTrue(surfaces.left.dispatchGenericMotionEvent(event(MotionEvent.ACTION_BUTTON_PRESS, 1, 1)))
        assertTrue(surfaces.root.dispatchTouchEvent(event(MotionEvent.ACTION_MOVE, 1)))
        assertTrue(surfaces.left.dispatchGenericMotionEvent(event(MotionEvent.ACTION_BUTTON_RELEASE, 0, 1)))
        assertTrue(surfaces.root.dispatchTouchEvent(event(MotionEvent.ACTION_UP, 0)))
        assertEquals("Only generic events send button edges; MOVE keeps the held button", listOf(2 to 1, 1 to 1, 3 to 0), PresentationNativeShadow.pointerActions)
        assertFalse(controller.get().isFinishing)
        controller.pause().stop().destroy()
    }
}
