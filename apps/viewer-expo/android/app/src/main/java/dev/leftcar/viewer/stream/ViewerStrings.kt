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
    val gestureHintConfirm: String get() = if (en) "Got it" else "확인"

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
    val inputChecking: String
        get() = if (en) "Checking remote input status" else "원격 입력 상태 확인 중"

    val statsDescription: String
        get() = if (en) "Stream details" else "화면 공유 상세 정보"

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
    val invalidRatio: String
        get() = if (en) "Invalid aspect ratio value." else "비율 값이 올바르지 않습니다."
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
        )
    } else {
        listOf(
            "탭" to "클릭",
            "끌기" to "드래그",
            "두 손가락으로 밀기" to "스크롤",
            "길게 누르기" to "오른쪽 클릭",
        )
    }
}
