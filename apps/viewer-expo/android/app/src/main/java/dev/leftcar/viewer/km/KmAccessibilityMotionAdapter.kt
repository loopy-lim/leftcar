package dev.leftcar.viewer.km

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.AccessibilityServiceInfo
import android.os.Build
import android.view.MotionEvent

/**
 * 라이브 [AccessibilityService.serviceInfo] 에 구독 마스크를 기록하는 커밋 seam.
 * 프로덕션 구현은 API 34+ read-modify-write
 * ([AccessibilityServiceInfo.setMotionEventSources]) 를 수행한다.
 */
internal interface KmMotionSourceWriter {
    /** 마스크 적용. 어떤 실패든 false (fail-closed). */
    fun write(mask: Int): Boolean
}

/**
 * 프로덕션 writer: 서비스의 현재 ServiceInfo 를 읽어 motionEventSources 만 교체하고
 * 다른 필드는 보존한다. serviceInfo 가 아직 공급되지 않은(미연결) 서비스는 쓰기
 * 실패로 처리한다 — 실패는 어댑터가 fail-closed 로 승격시킨다.
 */
internal class ServiceInfoMotionSourceWriter(private val service: AccessibilityService) :
    KmMotionSourceWriter {
    override fun write(mask: Int): Boolean {
        val info = try {
            service.serviceInfo
        } catch (t: Throwable) {
            null
        } ?: return false
        info.motionEventSources = mask
        return try {
            service.setServiceInfo(info)
            true
        } catch (t: Throwable) {
            false
        }
    }
}

/**
 * API 34+ [AccessibilityService.onMotionEvent] 통합 어댑터.
 *
 * - 아래는 서비스가 [KmCaptureController] 의 게이트를 통과했을 때만
 *   [AccessibilityServiceInfo.setMotionEventSources] 로 마우스 계열 구독을 켠다.
 * - [onMotionEvent] 는 소스 필터링·정규화 후 sink 로 전달하며, sink 예외 시
 *   fail-closed 로 구독을 즉시 해제한다.
 * - 쓰기 실패는 즉시 fail-closed(해제 재시도 포함)로 전환되고, 그 사유는 같은
 *   제어 이벤트의 반환 효과에 포함된다(재귀 없는 커밋 경로).
 * - 키 이벤트 경로는 건드리지 않는다(KeyBridge 계약 유지). Touchscreen 은 구독하지
 *   않는다([KmMotionSources]).
 *
 * 남는 배선(서비스 서브클래스·manifest 등록은 이 패키지 범위 밖 — 별도 결정):
 * ```kotlin
 * override fun onMotionEvent(event: MotionEvent) = kmAdapter.onMotionEvent(event)
 * // lifecycle 콜백에서 kmAdapter.dispatch(...) 로 게이트 입력을 전달
 * ```
 */
internal class KmAccessibilityMotionAdapter(
    private val controller: KmCaptureController,
    private val onSample: (KmMotionSample) -> Unit,
    private val apiLevel: Int = Build.VERSION.SDK_INT,
    private var writer: KmMotionSourceWriter? = null,
) {
    sealed interface KmAttachResult {
        data object Attached : KmAttachResult
        data object ApiUnsupported : KmAttachResult
        data object AlreadyAttached : KmAttachResult
    }

    private var attachedService: AccessibilityService? = null

    /** 커밋 실패 사유 유보. 해제 동작은 즉시, 사유 보고는 동일 dispatch 반환값에 포함. */
    private var pendingWriteFailure: Boolean = false

    /** 프로덕션 결합 지점. API 34 미만과 중복 결합, 기결합(writer 주입 포함)은 거부한다. */
    fun attach(service: AccessibilityService): KmAttachResult {
        if (apiLevel < KmMotionSources.MIN_API) return KmAttachResult.ApiUnsupported
        if (writer != null || attachedService != null) return KmAttachResult.AlreadyAttached
        attachedService = service
        // 명시 주입 writer(테스트/대체 결합)가 있으면 그것을 유지한다.
        if (writer == null) writer = ServiceInfoMotionSourceWriter(service)
        return KmAttachResult.Attached
    }

    /** 컨트롤러 이벤트를 위임하고 그 효과를 라이브 구독에 적용한다. */
    fun dispatch(event: KmCaptureEvent): List<KmCaptureEffect> {
        val effects = when (event) {
            is KmCaptureEvent.SessionConsent -> controller.onSessionConsent(event.granted)
            is KmCaptureEvent.ConnectionAuthenticated ->
                controller.onConnectionAuthenticated(event.authenticated)
            is KmCaptureEvent.OwnershipActive -> controller.onOwnershipActive(event.active)
            is KmCaptureEvent.ServiceCapability -> controller.onServiceCapability(event.capability)
            KmCaptureEvent.WatchdogTimeout -> controller.onWatchdogTimeout()
            KmCaptureEvent.PermissionLost -> controller.onPermissionLost()
            KmCaptureEvent.SinkError -> controller.onSinkError()
            KmCaptureEvent.WriteFailed -> controller.onWriteFailed()
            KmCaptureEvent.Teardown -> controller.onTeardown()
        }
        apply(effects)
        return effects + consumeOwed()
    }

    /**
     * 서비스 서브클래스의 [AccessibilityService.onMotionEvent] 오버라이드가 위임하는
     * 진입점. 구독 비활성 상태이거나 마우스 계열 소스가 아니면 이벤트를 폐기한다.
     */
    fun onMotionEvent(event: MotionEvent?) {
        if (!controller.captureActive) return
        val sample = KmMotionSample.from(event) ?: return
        if (!KmMotionSources.isMouseFamily(sample.source)) return
        try {
            onSample(sample)
        } catch (t: Throwable) {
            // sink 예외 = fail-closed: 구독을 즉시 해제하고 사유를 남긴다.
            apply(controller.onSinkError())
        }
    }

    /** 해제: 구독 해제(fail-closed) 후 writer 를 떼어낸다. 멱등. */
    fun detach() {
        apply(controller.onTeardown())
        writer = null
        attachedService = null
    }

    private fun apply(effects: List<KmCaptureEffect>) {
        for (effect in effects) {
            when (effect) {
                is KmCaptureEffect.ApplySources -> commit(effect.mask)
                is KmCaptureEffect.ReleaseSources -> commit(0)
                is KmCaptureEffect.FailClosed -> Unit // 사유는 컨트롤러/유보 경로에서 보고됨
            }
        }
    }

    /**
     * 마스크 커밋(방어적 sanitize 포함). 실패 시 즉시 controller.onWriteFailed() 로
     * fail-closed 시키고 그 해제 효과를 실행한다. 해제 재시도마저 실패하면 더는
     * 재귀하지 않는다(사유는 [consumeOwed] 로 같은 제어 이벤트 반환값에 1회 보고).
     */
    private fun commit(mask: Int) {
        val w = writer ?: return
        if (w.write(KmMotionSources.sanitize(mask))) return
        pendingWriteFailure = true
        for (effect in controller.onWriteFailed()) {
            when (effect) {
                is KmCaptureEffect.ApplySources ->
                    runCatching { w.write(KmMotionSources.sanitize(effect.mask)) }
                is KmCaptureEffect.ReleaseSources -> runCatching { w.write(0) }
                is KmCaptureEffect.FailClosed -> Unit
            }
        }
    }

    /** 유보된 쓰기 실패 사유가 있으면 반환하고 소비한다. */
    private fun consumeOwed(): List<KmCaptureEffect> {
        if (!pendingWriteFailure) return emptyList()
        pendingWriteFailure = false
        return listOf(KmCaptureEffect.FailClosed(KmFailClosedReason.WRITE_FAILED))
    }
}
