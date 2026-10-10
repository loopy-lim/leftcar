package dev.leftcar.viewer.km

import android.os.Build

/** 접근성 서비스가 KM 모션 구독을 받아들일 준비가 됐는지. */
enum class KmServiceCapability { NOT_ATTACHED, ATTACHED_READY }

/** fail-closed 사유. */
enum class KmFailClosedReason {
    DISCONNECTED,
    WATCHDOG_TIMEOUT,
    PERMISSION_LOST,
    TEARDOWN,
    SINK_ERROR,
    WRITE_FAILED,
}

/** 컨트롤러가 어댑터에 요구하는 구독 변경. */
sealed interface KmCaptureEffect {
    /** 마우스 계열 소스 구독 적용. */
    data class ApplySources(val mask: Int) : KmCaptureEffect

    /** 구독 해제(motionEventSources = 0). */
    data object ReleaseSources : KmCaptureEffect

    /** fail-closed 사유 보고(어댑터/글루가 로깅·사용자 고지에 사용). */
    data class FailClosed(val reason: KmFailClosedReason) : KmCaptureEffect
}

/** 어댑터가 컨트롤러에 전달하는 라이프사이클 입력. */
sealed interface KmCaptureEvent {
    /** 세션별 명시적 동의(기본 OFF). */
    data class SessionConsent(val granted: Boolean) : KmCaptureEvent

    /** 인증된 연결 상태. */
    data class ConnectionAuthenticated(val authenticated: Boolean) : KmCaptureEvent

    /** KM 소유권 활성(도메인 상태 기계의 Armed/Active 대응). */
    data class OwnershipActive(val active: Boolean) : KmCaptureEvent

    data class ServiceCapability(val capability: KmServiceCapability) : KmCaptureEvent

    data object WatchdogTimeout : KmCaptureEvent
    data object PermissionLost : KmCaptureEvent
    data object SinkError : KmCaptureEvent
    data object WriteFailed : KmCaptureEvent
    data object Teardown : KmCaptureEvent
}

/**
 * 마우스 모션 구독 캡처 게이트.
 *
 * 캡처는 다섯 게이트가 모두 열릴 때만 원한다 (기본 OFF):
 * API 34+ / 서비스 ATTACHED_READY / 명시적 세션 동의 / 인증된 연결 / 활성 소유권.
 *
 * fail-closed 규율: timeout·permission loss·teardown·sink/쓰기 실패·단절은 모든
 * 게이트 입력을 무효화한다 — 세션 동의를 폐기하므로 재연결이나 게이트 재설정만으로는
 * 재활성화되지 않고 새 명시 동의가 필요하다 (설계 §3 비정상 종료 재검토, §9 워치독).
 * 반면 동의 철회·소유권 해제는 정상 disarm 이며 다른 게이트 상태를 보존한다.
 */
class KmCaptureController(private val apiLevel: Int = Build.VERSION.SDK_INT) {
    var sessionConsentGranted: Boolean = false
        private set

    var connectionAuthenticated: Boolean = false
        private set

    var ownershipActive: Boolean = false
        private set

    var serviceCapability: KmServiceCapability = KmServiceCapability.NOT_ATTACHED
        private set

    /** 구독이 실제로 적용된 상태인지. false 면 이벤트를 절대 전달하지 않는다. */
    var captureActive: Boolean = false
        private set

    private var appliedMask: Int = 0

    /** 지금 원하는 구독 마스크 (0 = OFF). */
    fun desiredMask(): Int {
        val gatesOpen = apiLevel >= KmMotionSources.MIN_API &&
            sessionConsentGranted &&
            connectionAuthenticated &&
            ownershipActive &&
            serviceCapability == KmServiceCapability.ATTACHED_READY
        return if (gatesOpen) KmMotionSources.MOUSE_FAMILY else 0
    }

    fun onSessionConsent(granted: Boolean): List<KmCaptureEffect> {
        sessionConsentGranted = granted
        return converge()
    }

    fun onConnectionAuthenticated(authenticated: Boolean): List<KmCaptureEffect> {
        connectionAuthenticated = authenticated
        // 단절은 정상 해제가 아니라 fail-closed 이다: 동의를 폐기한다.
        return if (authenticated) converge() else failClosed(KmFailClosedReason.DISCONNECTED)
    }

    fun onOwnershipActive(active: Boolean): List<KmCaptureEffect> {
        ownershipActive = active
        // 소유권 해제는 정상 disarm — 동의·인증은 보존한다.
        return converge()
    }

    fun onServiceCapability(capability: KmServiceCapability): List<KmCaptureEffect> {
        serviceCapability = capability
        return converge()
    }

    fun onWatchdogTimeout(): List<KmCaptureEffect> = failClosed(KmFailClosedReason.WATCHDOG_TIMEOUT)

    fun onPermissionLost(): List<KmCaptureEffect> = failClosed(KmFailClosedReason.PERMISSION_LOST)

    fun onTeardown(): List<KmCaptureEffect> = failClosed(KmFailClosedReason.TEARDOWN)

    fun onSinkError(): List<KmCaptureEffect> = failClosed(KmFailClosedReason.SINK_ERROR)

    fun onWriteFailed(): List<KmCaptureEffect> = failClosed(KmFailClosedReason.WRITE_FAILED)

    /** 원하는 마스크와 적용된 마스크를 일치시킨다. 변화가 있을 때만 효과를 낸다(멱등). */
    private fun converge(): List<KmCaptureEffect> {
        val desired = desiredMask()
        if (desired == appliedMask) return emptyList()
        appliedMask = desired
        captureActive = desired != 0
        return if (desired != 0) {
            listOf(KmCaptureEffect.ApplySources(desired))
        } else {
            listOf(KmCaptureEffect.ReleaseSources)
        }
    }

    /**
     * fail-closed: 해제가 사유 보고보다 먼저이고, 세션 동의는 폐기된다.
     * 이미 해제된 상태에서는 ReleaseSources 중복 없이 사유 보고만 한다.
     */
    private fun failClosed(reason: KmFailClosedReason): List<KmCaptureEffect> {
        sessionConsentGranted = false
        val effects = mutableListOf<KmCaptureEffect>()
        if (appliedMask != 0) {
            appliedMask = 0
            captureActive = false
            effects += KmCaptureEffect.ReleaseSources
        }
        effects += KmCaptureEffect.FailClosed(reason)
        return effects
    }
}
