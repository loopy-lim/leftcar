package dev.leftcar.viewer.stream

import android.os.Build
import android.content.Context
import android.view.KeyEvent
import android.view.WindowInsets
import android.view.View
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection

/**
 * 스트림 창 구석에 숨어 있는 1×1 텍스트 렌즈. 소프트키보드(IME)가 붙을
 * 유일한 포커스 지점이다. 스트림 SurfaceView가 setZOrderOnTop으로 합성돼
 * 창 안 어떤 뷰도 비디오 위에 그려지지 않으므로, 렌즈도 보이지 않는 1px로
 * 존재한다 — 시각 UI는 HUD 토글 칩이 대신한다.
 *
 * 조합 중 텍스트 전송 정책 등 IME 의미 처리는 [TextInputRelay]가 담당하고
 * 이 뷰는 포워딩만 한다. 더미 모드(fullEditor=false) BaseInputConnection은
 * 실제 편집 버퍼가 없는 커스텀 뷰용 표준 경로다.
 */
internal class TextInputLensView(
    context: Context,
    private val relay: TextInputRelay,
) : View(context) {
    /** IME 표시 상태 변화(API 30+). HUD 칩 강조에 쓰인다. */
    var onImeVisibilityChanged: ((Boolean) -> Unit)? = null

    init {
        isFocusable = true
        isFocusableInTouchMode = true
        alpha = 0f
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
        // 1×1로 유지 — 클릭 가능 영역이 생기면 스트림 터치를 가로챈다.
        translationX = -1f
        translationY = -1f
    }

    override fun onCheckIsTextEditor(): Boolean = true

    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection? {
        outAttrs.inputType = EditorInfo.TYPE_CLASS_TEXT
        outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN or
            EditorInfo.IME_FLAG_NO_EXTRACT_UI or
            EditorInfo.IME_ACTION_DONE
        return object : BaseInputConnection(this, false) {
            override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
                if (text != null) relay.commitText(text)
                return true
            }

            override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
                relay.setComposingText(text)
                return true
            }

            override fun finishComposingText(): Boolean {
                relay.finishComposingText()
                return true
            }

            override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
                relay.deleteSurroundingText(beforeLength, afterLength)
                return true
            }

            override fun performEditorAction(actionCode: Int): Boolean {
                relay.performEditorAction(actionCode)
                return true
            }

            override fun sendKeyEvent(event: KeyEvent?): Boolean {
                if (event != null) relay.handleKeyCode(event.keyCode, event.action)
                return true
            }
        }
    }

    override fun onApplyWindowInsets(insets: WindowInsets): WindowInsets {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            onImeVisibilityChanged?.invoke(insets.isVisible(WindowInsets.Type.ime()))
        }
        return super.onApplyWindowInsets(insets)
    }
}
