package dev.leftcar.viewer.km

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.AccessibilityServiceInfo
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.accessibility.AccessibilityEvent

/** Human-enabled service. No gesture injection, screenshots, window content or touchscreen capture. */
class KmAccessibilityService : AccessibilityService() {
    private val routing = KmPhysicalTransition()
    internal val mode: KmCaptureMode get() = routing.mode
    @Volatile internal var physicalMouseName: String? = null; private set

    override fun onServiceConnected() { instance = this; KmRuntime.changed() }
    override fun onAccessibilityEvent(event: AccessibilityEvent?) = Unit
    override fun onInterrupt() { KmRuntime.stop("접근성 서비스가 중단되었습니다") }
    override fun onDestroy() {
        invalidateObservation()
        instance = null
        KmRuntime.stop("접근성 권한 또는 서비스가 종료되었습니다")
        super.onDestroy()
    }

    internal fun hasLocalHolds(): Boolean = routing.hasLocalHolds()

    override fun onConfigurationChanged(newConfig: android.content.res.Configuration) {
        super.onConfigurationChanged(newConfig)
        if (mode != KmCaptureMode.OFF) KmRuntime.stop("화면 배치가 변경되었습니다. 입력 공유를 다시 시작하세요")
        KmRuntime.changed()
    }

    internal var observationPlatform: KmOwnObservationPlatform = KmTargetedOwnObservation
    internal var observationMain: KmObservationMain = KmBoundedObservationMain()
    private val observationAttempt = KmObservationAttempt()

    internal fun invalidateObservation() { observationAttempt.cancel() }

    /** Native worker transaction; main never waits for this worker. */
    internal fun configure(authority: IKmInputAuthority, next: KmCaptureMode,
        captureTicket: Long? = null, freshGeneration: () -> Boolean = { true },
        commitSession: (() -> Unit) -> Boolean = { action -> action(); true }, commitOwner: () -> Unit = {}) {
        val token = observationAttempt.begin()
        try {
            check(instance === this && freshGeneration())
            val connection = observationPlatform.connection(this)
            val info = serviceInfo ?: throw KmObservationUnavailable()
            check(info.flags and AccessibilityServiceInfo.FLAG_INPUT_METHOD_EDITOR == 0)
            observationPlatform.observed(info) // Required app-side cleanup reader, before any active write.
            fun fresh(): Boolean = instance === this && freshGeneration() && observationAttempt.current(token) &&
                observationPlatform.connection(this) == connection && connection.binder.isBinderAlive &&
                (next != KmCaptureMode.CONSUME || captureTicket == runCatching { routing.beginCapture() }.getOrNull())
            val mask = if (next == KmCaptureMode.OFF) 0 else KmMotionSources.MOUSE_FAMILY
            check(observationMain.run {
                check(observationAttempt.write(token, ::fresh) { observationPlatform.localMask(this, mask) })
            })
            check(fresh())
            if (!authority.configureOwnObservation(connection.binder, when (next) {
                    KmCaptureMode.OFF -> 0; KmCaptureMode.OBSERVE -> 1; KmCaptureMode.CONSUME -> 2
                })) throw KmObservationUnavailable()
            check(fresh() && observationPlatform.localMask(this) == mask)
            // Remote/member reads and main completion are already finished. These nested
            // short gates serialize cancellation with routing AND its owner callback.
            check(commitSession {
                check(observationAttempt.write(token, { instance === this && freshGeneration() }) {
                    if (next == KmCaptureMode.CONSUME) {
                        check(routing.commitCapture(checkNotNull(captureTicket), commitOwner)) { "로컬 입력이 눌려 있어 입력 소유권을 변경할 수 없습니다" }
                    } else { routing.setMode(next); commitOwner() }
                })
            })
        } catch (error: Throwable) {
            observationAttempt.cancel()
            // Always OFF, never restore a saved active mask. Runtime also neutralizes/releases.
            restoreLocal()
            throw if (error is KmObservationUnavailable) error else KmObservationUnavailable()
        }
    }

    internal fun beginCapture(): Long = routing.beginCapture()

    internal fun restoreLocal(): Boolean {
        observationAttempt.cancel()
        val token = observationAttempt.begin()
        routing.setMode(KmCaptureMode.OFF)
        var expectedFlags: Int? = null
        val restored = runCatching {
            check(instance === this)
            val connection = observationPlatform.connection(this)
            check(observationMain.run {
                // OFF writes cannot rearm, even if a timed-out completion arrives later.
                check(observationAttempt.write(token, { instance === this && observationPlatform.connection(this) == connection }) {
                    observationPlatform.localMask(this, 0)
                })
                check(observationAttempt.current(token))
                val info = serviceInfo ?: error("접근성 설정을 읽을 수 없습니다")
                info.motionEventSources = 0
                info.flags = info.flags and AccessibilityServiceInfo.FLAG_REQUEST_FILTER_KEY_EVENTS.inv()
                expectedFlags = info.flags
                setServiceInfo(info)
            })
            check(instance === this && observationPlatform.connection(this) == connection)
            val applied = serviceInfo ?: error("접근성 설정을 확인할 수 없습니다")
            check(applied.motionEventSources == 0 && observationPlatform.observed(applied) == 0 &&
                applied.flags == expectedFlags &&
                applied.flags and AccessibilityServiceInfo.FLAG_REQUEST_FILTER_KEY_EVENTS == 0 &&
                observationPlatform.localMask(this) == 0 && mode == KmCaptureMode.OFF)
        }.isSuccess
        if (!restored) observationAttempt.cancel()
        return restored
    }

    override fun onMotionEvent(event: MotionEvent) {
        if (mode == KmCaptureMode.OFF || !KmMotionSources.isMouseFamily(event.source) || event.isFromSource(InputDevice.SOURCE_TOUCHSCREEN)) return
        val device = InputDevice.getDevice(event.deviceId)
        val own = device?.name?.startsWith("Leftcar KM ") == true
        val physical = device != null && !device.isVirtual && !own
        if (!own && !physical) return
        if (physical) {
            physicalMouseName = device!!.name
            routing.pointer(event.buttonState, event.metaState and MODIFIERS)
        }
        val bounds = getSystemService(android.view.WindowManager::class.java).maximumWindowMetrics.bounds
        if (!event.x.isFinite() || !event.y.isFinite()) return
        KmRuntime.motion(KmObservedPointer(
            event.x.toDouble(), event.y.toDouble(), bounds.width(), bounds.height(),
            own, physical, device?.name, event.buttonState,
        ))
    }

    override fun onKeyEvent(event: KeyEvent): Boolean {
        val device = InputDevice.getDevice(event.deviceId) ?: return false
        if (!KmKeyboardInventory.eligible(device)) return false
        // Permission queries can cross Binder; keep them outside the routing gate.
        val allowed = KmRuntime.physicalKeyAllowed()
        if (event.action != KeyEvent.ACTION_DOWN && event.action != KeyEvent.ACTION_UP) return mode == KmCaptureMode.CONSUME && allowed
        val consume = routing.key(event.keyCode, event.action == KeyEvent.ACTION_DOWN, event.metaState and MODIFIERS, allowed)
        if (!consume || !allowed) return false
        // A held HID key repeats in the destination OS. Do not manufacture duplicate down events.
        if (event.action == KeyEvent.ACTION_DOWN && event.repeatCount > 0) return true
        val usage = KmHidReports.androidUsage(event.keyCode)
        if (usage == null) KmRuntime.stop("공유할 수 없는 키가 입력되어 로컬 입력으로 복귀했습니다")
        else KmRuntime.key(usage, event.action == KeyEvent.ACTION_DOWN)
        return true
    }

    companion object {
        private const val MODIFIERS = KeyEvent.META_SHIFT_MASK or KeyEvent.META_ALT_MASK or
            KeyEvent.META_CTRL_MASK or KeyEvent.META_META_MASK or KeyEvent.META_SYM_ON or KeyEvent.META_FUNCTION_ON
        @Volatile internal var instance: KmAccessibilityService? = null
    }
}

internal class KmObservationUnavailable : IllegalStateException(REASON) {
    companion object {
        const val REASON = "UHID 또는 Android 16 마우스 관찰 기능을 사용할 수 없습니다"
    }
}

internal data class KmObservedPointer(
    val x: Double, val y: Double, val width: Int, val height: Int,
    val own: Boolean, val physical: Boolean, val deviceName: String?, val buttons: Int,
)
