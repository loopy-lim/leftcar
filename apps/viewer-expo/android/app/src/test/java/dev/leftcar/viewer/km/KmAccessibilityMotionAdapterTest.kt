package dev.leftcar.viewer.km

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.AccessibilityServiceInfo
import android.view.accessibility.AccessibilityEvent
import android.view.InputDevice
import android.view.MotionEvent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/** 서비스 결합 없이 결정적으로 검증하기 위한 기록용 writer. */
private class RecordingWriter : KmMotionSourceWriter {
    val attempts = mutableListOf<Int>()
    val committed = mutableListOf<Int>()
    var failWrites = false

    override fun write(mask: Int): Boolean {
        attempts += mask
        if (failWrites) return false
        committed += mask
        return true
    }
}

/** serviceInfo 가 아직 공급되지 않은 라이브 서비스의 fail-closed 경로 검증용. */
private class BareAccessibilityService : AccessibilityService() {
    override fun onAccessibilityEvent(event: AccessibilityEvent?) = Unit

    override fun onInterrupt() = Unit
}

private fun mouseEvent(action: Int = MotionEvent.ACTION_HOVER_MOVE): MotionEvent {
    val properties = MotionEvent.PointerProperties().apply {
        id = 0
        toolType = MotionEvent.TOOL_TYPE_MOUSE
    }
    val coords = MotionEvent.PointerCoords().apply { x = 5f; y = 6f }
    return MotionEvent.obtain(
        1, 2, action, 1,
        arrayOf(properties), arrayOf(coords),
        0, 0, 1f, 1f, 3, 0, InputDevice.SOURCE_MOUSE, 0,
    )
}

private fun touchscreenEvent(): MotionEvent {
    val properties = MotionEvent.PointerProperties().apply {
        id = 0
        toolType = MotionEvent.TOOL_TYPE_FINGER
    }
    val coords = MotionEvent.PointerCoords().apply { x = 1f; y = 2f }
    return MotionEvent.obtain(
        1, 2, MotionEvent.ACTION_MOVE, 1,
        arrayOf(properties), arrayOf(coords),
        0, 0, 1f, 1f, 3, 0, InputDevice.SOURCE_TOUCHSCREEN, 0,
    )
}

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE)
class KmAccessibilityMotionAdapterTest {
    private class Harness(
        val adapter: KmAccessibilityMotionAdapter,
        val controller: KmCaptureController,
        val writer: RecordingWriter,
        val samples: MutableList<KmMotionSample>,
    )

    private fun armedHarness(
        writer: RecordingWriter = RecordingWriter(),
        api: Int = 35,
        onSample: ((KmMotionSample) -> Unit)? = null,
    ): Harness {
        val controller = KmCaptureController(apiLevel = api)
        val samples = mutableListOf<KmMotionSample>()
        val adapter = KmAccessibilityMotionAdapter(
            controller,
            onSample = onSample ?: { samples += it },
            apiLevel = api,
            writer = writer,
        )
        adapter.dispatch(KmCaptureEvent.ConnectionAuthenticated(true))
        adapter.dispatch(KmCaptureEvent.OwnershipActive(true))
        adapter.dispatch(KmCaptureEvent.SessionConsent(true))
        adapter.dispatch(KmCaptureEvent.ServiceCapability(KmServiceCapability.ATTACHED_READY))
        return Harness(adapter, controller, writer, samples)
    }

    @Test
    fun `attach below api 34 is refused and nothing is written`() {
        val controller = KmCaptureController(apiLevel = 33)
        val writer = RecordingWriter()
        val adapter = KmAccessibilityMotionAdapter(controller, {}, apiLevel = 33, writer = writer)
        assertEquals(
            KmAccessibilityMotionAdapter.KmAttachResult.ApiUnsupported,
            adapter.attach(BareAccessibilityService()),
        )
        adapter.dispatch(KmCaptureEvent.ConnectionAuthenticated(true))
        adapter.dispatch(KmCaptureEvent.OwnershipActive(true))
        adapter.dispatch(KmCaptureEvent.SessionConsent(true))
        adapter.dispatch(KmCaptureEvent.ServiceCapability(KmServiceCapability.ATTACHED_READY))
        assertEquals(0, controller.desiredMask())
        assertTrue(writer.attempts.isEmpty())
    }

    @Test
    fun `attach twice returns already attached`() {
        val adapter = KmAccessibilityMotionAdapter(KmCaptureController(apiLevel = 35), {}, writer = RecordingWriter())
        assertEquals(
            KmAccessibilityMotionAdapter.KmAttachResult.AlreadyAttached,
            adapter.attach(BareAccessibilityService()),
        )
    }

    @Test
    fun `attach with api 35 accepts real service`() {
        val adapter = KmAccessibilityMotionAdapter(KmCaptureController(apiLevel = 35), {})
        assertEquals(
            KmAccessibilityMotionAdapter.KmAttachResult.Attached,
            adapter.attach(BareAccessibilityService()),
        )
    }

    @Test
    fun `real writer fails closed when service has no serviceInfo yet`() {
        val writer = ServiceInfoMotionSourceWriter(BareAccessibilityService())
        assertFalse(writer.write(KmMotionSources.MOUSE_FAMILY))
    }

    @Test
    fun `gates opening commits mouse family mask and consent revoke commits zero`() {
        val harness = armedHarness()
        assertEquals(listOf(KmMotionSources.MOUSE_FAMILY), harness.writer.committed)

        assertEquals(
            listOf(KmCaptureEffect.ReleaseSources),
            harness.adapter.dispatch(KmCaptureEvent.SessionConsent(false)),
        )
        assertEquals(listOf(KmMotionSources.MOUSE_FAMILY, 0), harness.writer.committed)
    }

    @Test
    fun `onMotionEvent delivers only mouse samples while capture is active`() {
        val harness = armedHarness()

        // 캡처 활성: 마우스는 전달, 터치스크린은 폐기.
        assertTrue(harness.controller.captureActive)
        harness.adapter.onMotionEvent(touchscreenEvent())
        assertTrue(harness.samples.isEmpty())
        harness.adapter.onMotionEvent(mouseEvent(MotionEvent.ACTION_HOVER_MOVE))
        assertEquals(1, harness.samples.size)
        assertEquals(InputDevice.SOURCE_MOUSE, harness.samples[0].source)
        assertEquals(5f, harness.samples[0].x)

        // null 이벤트 안전 폐기.
        harness.adapter.onMotionEvent(null)
        assertEquals(1, harness.samples.size)

        // 구독 해제 후: 마우스 이벤트도 폐기.
        harness.adapter.dispatch(KmCaptureEvent.SessionConsent(false))
        harness.adapter.onMotionEvent(mouseEvent())
        assertEquals(1, harness.samples.size)
    }

    @Test
    fun `sink exception clears subscriptions fail closed`() {
        val controller = KmCaptureController(apiLevel = 35)
        val writer = RecordingWriter()
        val adapter = KmAccessibilityMotionAdapter(
            controller,
            onSample = { error("sink broken") },
            apiLevel = 35,
            writer = writer,
        )
        adapter.dispatch(KmCaptureEvent.ConnectionAuthenticated(true))
        adapter.dispatch(KmCaptureEvent.OwnershipActive(true))
        adapter.dispatch(KmCaptureEvent.SessionConsent(true))
        adapter.dispatch(KmCaptureEvent.ServiceCapability(KmServiceCapability.ATTACHED_READY))
        assertTrue(controller.captureActive)

        adapter.onMotionEvent(mouseEvent())

        assertFalse(controller.captureActive)
        assertFalse(controller.sessionConsentGranted)
        assertEquals(listOf(KmMotionSources.MOUSE_FAMILY, 0), writer.committed)

        // fail-closed 후 새 이벤트는 전달되지 않는다.
        adapter.onMotionEvent(mouseEvent())
        assertEquals(listOf(KmMotionSources.MOUSE_FAMILY, 0), writer.committed)
    }

    @Test
    fun `write failure fails closed with release attempt`() {
        val controller = KmCaptureController(apiLevel = 35)
        val writer = RecordingWriter().apply { failWrites = true }
        val adapter = KmAccessibilityMotionAdapter(controller, {}, apiLevel = 35, writer = writer)
        adapter.dispatch(KmCaptureEvent.ConnectionAuthenticated(true))
        adapter.dispatch(KmCaptureEvent.OwnershipActive(true))
        val effects = listOf(
            adapter.dispatch(KmCaptureEvent.SessionConsent(true)),
            adapter.dispatch(KmCaptureEvent.ServiceCapability(KmServiceCapability.ATTACHED_READY)),
        ).last()

        assertTrue(effects.contains(KmCaptureEffect.FailClosed(KmFailClosedReason.WRITE_FAILED)))
        assertFalse(controller.captureActive)
        // ApplySources 시도 후 해제(0) 재시도까지 두 번 시도했다.
        assertEquals(listOf(KmMotionSources.MOUSE_FAMILY, 0), writer.attempts)
    }

    @Test
    fun `detach releases subscriptions and detaches writer`() {
        val harness = armedHarness()
        assertTrue(harness.controller.captureActive)

        harness.adapter.detach()
        assertEquals(listOf(KmMotionSources.MOUSE_FAMILY, 0), harness.writer.committed)
        assertFalse(harness.controller.captureActive)
        assertFalse(harness.controller.sessionConsentGranted)

        // detach 후 재설정 입력으로도 writer 커밋이 없다 (어댑터 분리 상태).
        harness.adapter.dispatch(KmCaptureEvent.ConnectionAuthenticated(true))
        harness.adapter.dispatch(KmCaptureEvent.OwnershipActive(true))
        assertEquals(listOf(KmMotionSources.MOUSE_FAMILY, 0), harness.writer.committed)
    }

    @Test
    fun `serviceInfo mask round trip uses real setMotionEventSources and preserves other fields`() {
        val info = AccessibilityServiceInfo()
        assertEquals(0, info.motionEventSources)

        info.flags = 123
        info.motionEventSources = KmMotionSources.MOUSE_FAMILY
        assertEquals(KmMotionSources.MOUSE_FAMILY, info.motionEventSources)
        assertEquals(123, info.flags)

        info.motionEventSources = 0
        assertEquals(0, info.motionEventSources)
        assertEquals(123, info.flags)
    }
}
