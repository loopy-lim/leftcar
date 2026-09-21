package dev.leftcar.viewer.stream

/**
 * 뷰어 네이티브 UI 문자열. 언어는 JS에서 스트림 창을 열 때 인텐트 extra로
 * 전달받아 "leftcar_viewer" SharedPreferences에 저장하므로, 창이 재생성돼도
 * 직전 선택이 유지된다. 순수 데이터로만 구성해 JVM 단위 테스트로 검증한다.
 */
object ViewerStrings {
    const val PREF_LANGUAGE = "language"

    @Volatile
    var language: String = "ko"
        private set

    fun applyLanguage(value: String?) {
        language = if (value == "en") "en" else "ko"
    }

    private val en: Boolean get() = language == "en"

    val gestureHintTitle: String get() = if (en) "Touch Gestures" else "터치 제스처"

    val rebindReconnecting: String
        get() = if (en) "Reconnecting this window" else "화면을 같은 창에서 다시 연결하는 중"
    val rebindReconnectingControl: String
        get() = if (en) "Reconnecting to the computer in this window"
        else "컴퓨터 연결을 같은 창에서 다시 연결하는 중"
    val rebindPreparing: String
        get() = if (en) "Preparing to reconnect" else "화면을 다시 연결할 준비 중"
    val rebindFailed: String
        get() = if (en) "Could not reconnect. Retrying in this window"
        else "화면을 다시 연결하지 못했습니다. 현재 창에서 재시도합니다"
    val rebindDescription: String
        get() = if (en) "Reconnecting stream" else "화면 공유 재연결 중"

    val inputAllowed: String
        get() = if (en) "Remote mouse and keyboard input enabled"
        else "원격 마우스와 키보드 입력 가능"
    val inputLocked: String
        get() = if (en) "Remote input locked" else "원격 마우스와 키보드 입력 잠김"
    /** 잠김 동안 사라지지 않는 배너 문구. 탭하면 호스트에 허용 요청을
     * 보낸다(2026-09-21) — 요청의 결정은 여전히 호스트 운용자 몫이다. */
    val inputLockedBanner: String
        get() = if (en) "Input locked · Tap to request access"
        else "입력 잠김 · 탭하여 허용 요청"
    val inputRequestSent: String
        get() = if (en) "Request sent · Allow in Leftcar Host"
        else "요청 전송됨 · Mac의 Leftcar Host에서 허용"
    val inputChecking: String
        get() = if (en) "Checking remote input status" else "원격 입력 상태 확인 중"

    val keyBridgeUpdateRequired: String
        get() = if (en) "Update KeyBridge to use the Mac keyboard and mouse handoff."
        else "Mac 키보드·마우스 전환을 사용하려면 KeyBridge를 업데이트하세요."
    val keyBridgeUnavailable: String
        get() = if (en) "Could not hand the keyboard and mouse to the Mac."
        else "키보드와 마우스를 Mac으로 넘기지 못했습니다."

    val gestureHelpDescription: String
        get() = if (en) "Show touch gestures" else "터치 제스처 안내 보기"

    val keyboardToggleDescription: String
        get() = if (en) "Toggle the on-screen keyboard for typing on the computer"
        else "컴퓨터로 타이핑할 소프트키보드 켜기/끄기"

    val statsDescription: String
        get() = if (en) "Stream details" else "화면 공유 상세 정보"

    val closeWindowTitle: String get() = if (en) "Close this screen?" else "이 화면을 닫을까요?"
    val closeWindowMessage: String get() = if (en) "Only this screen's stream will end." else "이 창의 화면 공유만 종료됩니다."
    val continueViewing: String get() = if (en) "Keep viewing" else "계속 보기"
    val closeWindow: String get() = if (en) "Close screen" else "화면 닫기"

    /** Recoverable failures retain the window and use the reconnect indicator. */
    fun terminationMessage(reason: Int): String = when (reason) {
        1 -> if (en) "The connection to the computer was lost, so screen sharing ended."
        else "컴퓨터와의 연결이 끊어져 화면 공유를 종료했습니다."
        2, 3 -> if (en) "Screen sharing was stopped on the computer."
        else "컴퓨터에서 이 화면 공유를 종료했습니다."
        else -> if (en) "Reconnecting screen sharing."
        else "화면 공유를 다시 연결하고 있습니다."
    }

    fun fpsDescription(fpsText: String): String =
        if (en) "Actual render rate $fpsText" else "실제 렌더링 속도 $fpsText"

    val displayFallback: String get() = if (en) "Display" else "디스플레이"

    val splitDecoderMissing: String
        get() = if (en)
            "No verified concurrent hardware H.264 decoder; cannot open the 4K split stream."
        else "검증된 동시 하드웨어 H.264 디코더가 없어 4K 분할 스트림을 열 수 없습니다."
    val splitDecoderUnavailable: String
        get() = if (en)
            "Cannot use the two concurrent hardware H.264 decoders required for the 4K split stream."
        else "4K 분할 스트림에 필요한 동시 2개 하드웨어 H.264 디코더를 사용할 수 없습니다."
    val prepareFailed: String
        get() = if (en) "Could not prepare the media receive port."
        else "미디어 수신 포트를 준비하지 못했습니다."
    val streamNotActive: String
        get() = if (en) "Could not find the stream window." else "화면 공유 창을 찾을 수 없습니다."
    val prepareCancelFailed: String
        get() = if (en) "Failed to release the media receive port."
        else "미디어 수신 포트 정리에 실패했습니다."
}

/** 언어별 제스처 안내 행. 표시 문구는 [ViewerStrings.language]와 별개 인자로 받아 테스트한다. */
object GestureHintRows {
    fun rows(language: String): List<Pair<String, String>> = if (language == "en") {
        listOf(
            "Tap" to "Click",
            "Drag" to "Drag",
            "Two-finger swipe" to "Scroll",
            "Long press" to "Right-click",
            "Pinch" to "Zoom",
        )
    } else {
        listOf(
            "탭" to "클릭",
            "끌기" to "드래그",
            "두 손가락으로 밀기" to "스크롤",
            "길게 누르기" to "오른쪽 클릭",
            "두 손가락으로 벌리기·오므리기" to "확대·축소",
        )
    }
}
