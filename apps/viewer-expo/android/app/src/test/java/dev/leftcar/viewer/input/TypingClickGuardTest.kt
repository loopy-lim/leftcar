package dev.leftcar.viewer.input

import android.view.MotionEvent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TypingClickGuardTest {
    private var now = 1_000L
    private val guard = TypingClickGuard(clock = { now })

    @Test fun `키 활동이 없으면 press와 release는 항상 전달된다`() {
        val bit = MotionEvent.BUTTON_PRIMARY
        assertTrue(guard.shouldForwardPress(bit))
        assertTrue(guard.shouldForwardRelease(bit))
        assertFalse(guard.typingRecently())
    }

    @Test fun `키 활동 창 안의 press는 억제되고 그 쌍 release도 억제된다`() {
        guard.onTyping()
        val bit = MotionEvent.BUTTON_PRIMARY
        assertTrue(guard.typingRecently())
        assertFalse(guard.shouldForwardPress(bit))
        now += 100
        assertFalse(guard.shouldForwardRelease(bit))
        // 쌍이 소비된 뒤의 같은 버튼 press는 창 안이 아니면 다시 전달된다.
        now += TypingClickGuard.DEFAULT_WINDOW_MS
        assertFalse(guard.typingRecently())
        assertTrue(guard.shouldForwardPress(bit))
    }

    @Test fun `창이 지나면 press는 다시 전달된다`() {
        guard.onTyping()
        val bit = MotionEvent.BUTTON_SECONDARY
        now += TypingClickGuard.DEFAULT_WINDOW_MS - 1
        assertTrue(guard.typingRecently())
        assertFalse(guard.shouldForwardPress(bit))
    }

    @Test fun `경계값인 windowMs 뒤에는 억제가 끝난다`() {
        guard.onTyping()
        now += TypingClickGuard.DEFAULT_WINDOW_MS
        assertFalse(guard.typingRecently())
        assertTrue(guard.shouldForwardPress(MotionEvent.BUTTON_PRIMARY))
    }

    @Test fun `실제로 전송된 press의 release는 타이핑 창 안이어도 억제되지 않는다`() {
        val bit = MotionEvent.BUTTON_PRIMARY
        assertTrue(guard.shouldForwardPress(bit))
        guard.onTyping()
        now += 50
        assertTrue(guard.shouldForwardRelease(bit))
    }

    @Test fun `sanitizeButtons는 억제 중인 버튼 비트만 뺀다`() {
        val primary = MotionEvent.BUTTON_PRIMARY
        val secondary = MotionEvent.BUTTON_SECONDARY
        guard.onTyping()
        assertFalse(guard.shouldForwardPress(primary))
        assertEquals(secondary, guard.sanitizeButtons(primary or secondary))
        assertEquals(0, guard.sanitizeButtons(primary))
        now += TypingClickGuard.DEFAULT_WINDOW_MS + 1
        // 쌍 억제는 창 만료 뒤에도 유지된다 — press 가 호스트에 없었으므로.
        assertFalse(guard.shouldForwardRelease(primary))
        assertEquals(primary or secondary, guard.sanitizeButtons(primary or secondary))
    }

    @Test fun `세션 경계로 짝 release를 잃은 억제 쌍은 다음 전송 press가 무효화한다`() {
        val bit = MotionEvent.BUTTON_PRIMARY
        guard.onTyping()
        assertFalse(guard.shouldForwardPress(bit))
        now += TypingClickGuard.DEFAULT_WINDOW_MS + 1
        // 유령 쌍이 남은 채 창 밖의 실제 클릭이 오면 press 도 release 도
        // 전송돼야 한다 — 그렇지 않으면 호스트에 눌린 버튼이 남는다.
        assertTrue(guard.shouldForwardPress(bit))
        now += 50
        assertTrue(guard.shouldForwardRelease(bit))
    }

    @Test fun `서로 다른 버튼의 억제는 독립적으로 소비된다`() {
        guard.onTyping()
        val primary = MotionEvent.BUTTON_PRIMARY
        val secondary = MotionEvent.BUTTON_SECONDARY
        // 창 안의 모든 버튼 press 가 억제 대상이다.
        assertFalse(guard.shouldForwardPress(primary))
        assertFalse(guard.shouldForwardPress(secondary))
        // secondary 의 쌍을 먼저 소비해도 primary 의 쌍은 그대로 억제된다.
        assertFalse(guard.shouldForwardRelease(secondary))
        assertTrue(guard.shouldForwardRelease(secondary))
        // 아직 primary 쌍이 억제 중이므로 buttonState 에서 primary 만 빠진다.
        assertEquals(secondary, guard.sanitizeButtons(primary or secondary))
        assertFalse(guard.shouldForwardRelease(primary))
    }
}
