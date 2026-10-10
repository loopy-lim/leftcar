package dev.leftcar.viewer.km

import android.view.InputDevice

/**
 * API 34+ [android.accessibilityservice.AccessibilityServiceInfo.setMotionEventSources]
 * 허용 비트의 안전 부분집합 (설계 §2(d), wifi-km-connected-device-validation §5).
 *
 * - 물리 마우스 계열(SOURCE_MOUSE, SOURCE_MOUSE_RELATIVE, SOURCE_TOUCHPAD)만 구독한다.
 * - SOURCE_TOUCHSCREEN 은 절대 포함하지 않는다 — 태블릿 로컬 터치 입력 보존(설계 §5
 *   "동시 로컬 입력"). 클래스 전체 마스크(SOURCE_CLASS_*) 단독 구독도 금지한다.
 * - 키 이벤트 필터는 건드리지 않는다(키 경로는 KeyBridge 소유 — 기존 계약 유지).
 */
object KmMotionSources {
    /** setMotionEventSources / onMotionEvent 최소 API (Android 14). */
    const val MIN_API = 34

    const val SOURCE_MOUSE: Int = InputDevice.SOURCE_MOUSE
    const val SOURCE_MOUSE_RELATIVE: Int = InputDevice.SOURCE_MOUSE_RELATIVE
    const val SOURCE_TOUCHPAD: Int = InputDevice.SOURCE_TOUCHPAD

    /** 마우스 계열 전체 마스크. 캡처 활성 시 ServiceInfo 에 기록되는 유일한 값. */
    const val MOUSE_FAMILY: Int =
        SOURCE_MOUSE or SOURCE_MOUSE_RELATIVE or SOURCE_TOUCHPAD

    private const val FAMILY_DEVICE_BITS: Int =
        MOUSE_FAMILY and InputDevice.SOURCE_CLASS_MASK.inv()

    /**
     * 이벤트 소스가 마우스 계열인지. 클래스 비트만 있는 값(터치스크린 등)은 거부한다.
     */
    fun isMouseFamily(source: Int): Boolean {
        if (source == 0) return false
        val deviceBits = source and InputDevice.SOURCE_CLASS_MASK.inv()
        return deviceBits != 0 && (deviceBits and FAMILY_DEVICE_BITS) != 0
    }

    /** 임의 요청 마스크를 마우스 계열 비트로 제한한다(방어). 클래스 비트만 남으면 0. */
    fun sanitize(requested: Int): Int {
        val masked = requested and MOUSE_FAMILY
        if (masked and FAMILY_DEVICE_BITS == 0) return 0
        return masked
    }

    fun supportsMotionEvents(apiLevel: Int): Boolean = apiLevel >= MIN_API
}
