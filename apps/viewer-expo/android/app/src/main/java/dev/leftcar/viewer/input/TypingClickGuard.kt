package dev.leftcar.viewer.input

import android.os.SystemClock

/**
 * 타이핑 직후의 핀치 오클릭 억제.
 *
 * GXR 손 추적 포인터는 마우스 계열 MotionEvent 로 들어오고, 키보드 타이핑
 * 손동작이 핀치(주 버튼 press/release)로 오탐된다. 앱에는 핀치 오탐과 실제
 * 클릭을 구분할 신호가 없으므로, 마지막 키 활동 뒤 [windowMs] 안의 포인터
 * 버튼 press 전달을 억제하고 그 release 도 쌍으로 억제한다. 이동·휠·스크롤은
 * 억제하지 않는다 — 커서 추적과 엣지 감지는 유지된다.
 *
 * 쌍 보존: 실제로 전송된 press 의 release 는 언제나 전송된다 — 호스트에 버튼이
 * 눌린 채 남는 상태를 만들지 않는다. 같은 물리 버튼은 자기 release 도착 전에
 * 다시 눌릴 수 없으므로 억제 쌍은 스스로 해소된다.
 *
 * 스트리밍은 main 스레드, KM은 main(키 활동 기록)과 worker(버튼 억제)가 같은
 * 인스턴스를 쓰므로 메서드를 동기화한다.
 */
class TypingClickGuard(
    private val clock: () -> Long = SystemClock::elapsedRealtime,
    private val windowMs: Long = DEFAULT_WINDOW_MS,
) {
    private var lastTypingAt = -1L
    private val pendingReleaseDrops = HashSet<Int>()

    /** 물리 키 DOWN·IME 텍스트·키페어 전송 등 키 활동 시각을 기록한다. */
    @Synchronized
    fun onTyping() {
        lastTypingAt = clock()
    }

    @Synchronized
    fun typingRecently(): Boolean = lastTypingAt >= 0 && clock() - lastTypingAt < windowMs

    /** press 전달 여부. false 면 쌍이 되는 release 도 억제된다. [bit]는 BUTTON_* 비트값. */
    @Synchronized
    fun shouldForwardPress(bit: Int): Boolean {
        if (typingRecently()) {
            pendingReleaseDrops.add(bit)
            return false
        }
        // 실제로 전송되는 press 는 남아 있는 미완료 쌍을 무효화한다. 세션·소유권
        // 경계에서 짝 release 가 이 guard 를 지나지 못하고 사라진 경우(유령 쌍),
        // 무효화하지 않으면 나중에 온 release 가 흡수돼 호스트에 눌린 버튼이
        // 남는다.
        pendingReleaseDrops.remove(bit)
        return true
    }

    /** release 전달 여부. 억제된 press 의 쌍 release 도 억제한다. */
    @Synchronized
    fun shouldForwardRelease(bit: Int): Boolean = pendingReleaseDrops.remove(bit).not()

    /** 억제 중인 버튼을 전송 이벤트의 buttonState 에서 뺀다. */
    @Synchronized
    fun sanitizeButtons(buttons: Int): Int =
        pendingReleaseDrops.fold(buttons) { state, bit -> state and bit.inv() }

    companion object {
        /** 핀치 오탐은 키 스트로크 뒤 수백 ms 안에 온다. 마지막 키 활동 뒤 이 창 동안 press 를 억제한다. */
        const val DEFAULT_WINDOW_MS = 800L
    }
}
