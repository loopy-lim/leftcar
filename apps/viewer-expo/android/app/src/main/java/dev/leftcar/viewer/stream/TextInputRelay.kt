package dev.leftcar.viewer.stream

/**
 * IME InputConnection 의미를 원격 전송 명령으로 변환하는 JVM 순수 변환기.
 * android.view.InputConnection의 실제 구현은 [TextInputLensView]가 담당하고,
 * 이 클래스는 문자열·정수만으로 구성돼 JVM 단위 테스트로 전수 검증한다.
 *
 * 조합 중인 텍스트는 전송하지 않는다. 한국어 조합("ㄱ"→"가"→"간")의 중간
 * 값이 Mac에 타이핑되는 것을 막기 위해서다. 조합 버퍼는 커밋 시점에만
 * 확정 전송되고, 일부 IME가 commitText 없이 finishComposingText로 조합을
 * 끝내는 경로도 같은 전송으로 수렴시킨다.
 */
internal class TextInputRelay(
    private val sendText: (String) -> Unit,
    private val sendBackspace: (Int) -> Unit,
    private val sendForwardDelete: (Int) -> Unit,
    private val sendEnter: () -> Unit,
) {
    companion object {
        /** android.view.KeyEvent 키코드 미러 (JVM 순수 유지를 위한 복사본). */
        const val KEYCODE_ENTER = 66
        const val KEYCODE_DEL = 67
        const val KEYCODE_FORWARD_DEL = 112
        const val ACTION_DOWN = 0

        /**
         * UDP 컨트롤 버퍼는 512바이트다. 헤더(10B)·토큰·연속 패킷 간격을
         * 고려한 1패킷 UTF-8 청크 상한.
         */
        const val MAX_CHUNK_BYTES = 200
        private const val MAX_DELETE_COUNT = 64
    }

    private var composing: String = ""

    /** 조합 중 텍스트 갱신. 전송하지 않고 버퍼만 대체한다. */
    fun setComposingText(text: CharSequence?) {
        composing = text?.toString().orEmpty()
    }

    /** 조합을 확정 전송으로 마무리한다. 버퍼가 비었으면 no-op. */
    fun finishComposingText() {
        val text = composing
        composing = ""
        if (text.isNotEmpty()) dispatch(text)
    }

    /**
     * 커밋된 최종 텍스트 전송. 개행은 Enter 키로 분리하고, 나머지는
     * [MAX_CHUNK_BYTES] 이하의 UTF-8 청크로 나눠 보낸다.
     */
    fun commitText(text: CharSequence) {
        composing = ""
        dispatch(text.toString())
    }

    fun deleteSurroundingText(beforeLength: Int, afterLength: Int) {
        if (beforeLength > 0) sendBackspace(beforeLength.coerceAtMost(MAX_DELETE_COUNT))
        if (afterLength > 0) sendForwardDelete(afterLength.coerceAtMost(MAX_DELETE_COUNT))
    }

    /**
     * IME 액션 버튼(완료·검색·다음)과 소프트 Enter는 모두 Mac의 Enter로
     * 수렴시킨다. performEditorAction은 사용자가 액션 키를 눌렀을 때만
     * 호출되므로 액션 코드 분기 없이 전부 Enter가 사용자 의도다.
     */
    fun performEditorAction(@Suppress("UNUSED_PARAMETER") actionCode: Int) {
        sendEnter()
    }

    /** sendKeyEvent로 들어오는 하드웨어식 키 이벤트(백스페이스·Enter). */
    fun handleKeyCode(keyCode: Int, action: Int) {
        if (action != ACTION_DOWN) return
        when (keyCode) {
            KEYCODE_DEL -> sendBackspace(1)
            KEYCODE_FORWARD_DEL -> sendForwardDelete(1)
            KEYCODE_ENTER -> sendEnter()
        }
    }

    private fun dispatch(text: String) {
        var segment = StringBuilder()
        var index = 0
        while (index < text.length) {
            val codePoint = text.codePointAt(index)
            val charCount = Character.charCount(codePoint)
            when (codePoint) {
                '\n'.code -> {
                    flushSegment(segment)
                    segment = StringBuilder()
                    sendEnter()
                }
                '\r'.code -> {}
                else -> {
                    val byteLength = utf8Length(codePoint)
                    if (segment.utf8Bytes() + byteLength > MAX_CHUNK_BYTES) {
                        flushSegment(segment)
                        segment = StringBuilder()
                    }
                    segment.appendCodePoint(codePoint)
                }
            }
            index += charCount
        }
        flushSegment(segment)
    }

    private fun flushSegment(segment: StringBuilder) {
        if (segment.isNotEmpty()) sendText(segment.toString())
    }

    private fun StringBuilder.utf8Bytes(): Int = sumOf { utf8Length(it.code) }

    private fun utf8Length(codePoint: Int): Int = when {
        codePoint < 0x80 -> 1
        codePoint < 0x800 -> 2
        codePoint < 0x10000 -> 3
        else -> 4
    }
}
