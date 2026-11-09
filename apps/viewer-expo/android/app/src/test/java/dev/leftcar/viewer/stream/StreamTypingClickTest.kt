package dev.leftcar.viewer.stream

import android.content.Intent
import android.os.Looper
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import dev.leftcar.viewer.input.TypingClickGuard
import org.junit.Assert.*
import org.junit.After
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.util.ReflectionHelpers
import org.robolectric.util.ReflectionHelpers.ClassParameter.from
import java.util.concurrent.TimeUnit

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE, shadows = [PresentationNativeShadow::class, ValidSurfaceShadow::class, PresentationSurfaceViewShadow::class])
class StreamTypingClickTest {
    @After fun resetInputGate() { PresentationNativeShadow.inputEnabled = 0 }

    /** StreamMouseInputTest 와 같은 승격 — Robolectric 에서는 KeyBridge 핸드셰이크를 완주할 수 없다. */
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

    private fun keyDown(): KeyEvent = KeyEvent(1, 2, KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_A, 0)

    private fun buildActivity(instance: String) =
        Robolectric.buildActivity(StreamActivity::class.java, Intent().putExtra("instance", instance).putExtra("host", "192.168.0.2")).create().start().resume()

    private fun layoutSurfaces(activity: StreamActivity) {
        val surfaces = ReflectionHelpers.getField<StreamSurfaces>(activity, "streamSurfaces")
        surfaces.root.measure(View.MeasureSpec.makeMeasureSpec(1920, View.MeasureSpec.EXACTLY), View.MeasureSpec.makeMeasureSpec(1080, View.MeasureSpec.EXACTLY))
        surfaces.root.layout(0, 0, 1920, 1080)
    }

    private fun surfaces(activity: StreamActivity): StreamSurfaces =
        ReflectionHelpers.getField(activity, "streamSurfaces")

    private fun advanceWindow() {
        shadowOf(Looper.getMainLooper()).idleFor(TypingClickGuard.DEFAULT_WINDOW_MS + 100, TimeUnit.MILLISECONDS)
    }

    /** 캡처 경로는 실제 포인터 캡처가 유지될 때만 OnCapturedPointerListener 로
     *  들어오므로 Robolectric 디스패치로는 닿지 않는다 — 비공개 메서드를 직접
     *  호출해 forwardCapturedPointer 의 억제를 검증한다. */
    private fun capturedForward(activity: StreamActivity, action: Int, buttons: Int, button: Int): Boolean =
        ReflectionHelpers.callInstanceMethod(
            activity, "forwardCapturedPointer",
            from(MotionEvent::class.java, event(action, buttons, button)),
            from(View::class.java, surfaces(activity).left),
        )

    @Test fun `타이핑 직후 캡처 경로의 press와 release는 전송되지 않는다`() {
        PresentationNativeShadow.inputEnabled = 1
        PresentationNativeShadow.pointerButtons.clear()
        val controller = buildActivity("typing-capture")
        layoutSurfaces(controller.get())
        promoteToRemoteMouse(controller.get())
        assertTrue(controller.get().dispatchKeyEvent(keyDown()))
        assertTrue(capturedForward(controller.get(), MotionEvent.ACTION_BUTTON_PRESS, 1, 1))
        assertTrue(capturedForward(controller.get(), MotionEvent.ACTION_BUTTON_RELEASE, 0, 1))
        assertTrue(PresentationNativeShadow.pointerButtons.isEmpty())
        // 창 만료 뒤에는 같은 클릭이 정상 전달된다.
        advanceWindow()
        assertTrue(capturedForward(controller.get(), MotionEvent.ACTION_BUTTON_PRESS, 1, 1))
        assertTrue(capturedForward(controller.get(), MotionEvent.ACTION_BUTTON_RELEASE, 0, 1))
        assertEquals(listOf(Triple(2, 1, 1), Triple(3, 1, 0)), PresentationNativeShadow.pointerButtons)
        controller.pause().stop().destroy()
    }

    @Test fun `타이핑 직후 직접 매핑 경로의 press도 억제된다`() {
        PresentationNativeShadow.inputEnabled = 1
        PresentationNativeShadow.pointerButtons.clear()
        val controller = buildActivity("typing-direct")
        layoutSurfaces(controller.get())
        assertTrue(controller.get().dispatchKeyEvent(keyDown()))
        surfaces(controller.get()).left.dispatchGenericMotionEvent(event(MotionEvent.ACTION_BUTTON_PRESS, 1, 1))
        assertTrue(PresentationNativeShadow.pointerButtons.isEmpty())
        controller.pause().stop().destroy()
    }

    @Test fun `키 활동이 없으면 직접 매핑 press는 종전대로 전달된다`() {
        PresentationNativeShadow.inputEnabled = 1
        PresentationNativeShadow.pointerButtons.clear()
        val controller = buildActivity("no-typing")
        layoutSurfaces(controller.get())
        surfaces(controller.get()).left.dispatchGenericMotionEvent(event(MotionEvent.ACTION_BUTTON_PRESS, 1, 1))
        surfaces(controller.get()).left.dispatchGenericMotionEvent(event(MotionEvent.ACTION_BUTTON_RELEASE, 0, 1))
        assertEquals(listOf(Triple(2, 1, 1), Triple(3, 1, 0)), PresentationNativeShadow.pointerButtons)
        controller.pause().stop().destroy()
    }

    @Test fun `타이핑 창 안의 클릭은 원격 소유권 획득을 시작하지 않는다`() {
        PresentationNativeShadow.inputEnabled = 1
        val controller = buildActivity("typing-activation")
        layoutSurfaces(controller.get())
        val activity = controller.get()
        assertTrue(activity.dispatchKeyEvent(keyDown()))
        val result = ReflectionHelpers.callInstanceMethod<Boolean>(
            activity, "activateRemoteInput",
            from(MotionEvent::class.java, event(MotionEvent.ACTION_BUTTON_PRESS, 1, 1)),
            from(View::class.java, surfaces(activity).left),
        )
        assertTrue(result)
        val ownership = ReflectionHelpers.getField<InputOwnershipController>(activity, "inputOwnership")
        assertEquals(InputOwner.LOCAL_ANDROID, ownership.owner)
        controller.pause().stop().destroy()
    }
}
