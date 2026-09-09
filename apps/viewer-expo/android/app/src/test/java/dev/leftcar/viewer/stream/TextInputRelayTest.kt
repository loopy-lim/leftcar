package dev.leftcar.viewer.stream

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class TextInputRelayTest {
    private class Recorder {
        val texts = mutableListOf<String>()
        val backspaces = mutableListOf<Int>()
        val forwardDeletes = mutableListOf<Int>()
        var enters = 0
    }

    private fun relayWith(recorder: Recorder) = TextInputRelay(
        sendText = { recorder.texts += it },
        sendBackspace = { recorder.backspaces += it },
        sendForwardDelete = { recorder.forwardDeletes += it },
        sendEnter = { recorder.enters += 1 },
    )

    @Test
    fun `커밋 텍스트는 그대로 한 번 전송된다`() {
        val recorder = Recorder()
        val relay = relayWith(recorder)
        relay.commitText("안녕하세요 hello")
        assertEquals(listOf("안녕하세요 hello"), recorder.texts)
        assertEquals(0, recorder.enters)
    }

    @Test
    fun `커밋 내 개행은 Enter로 분리된다`() {
        val recorder = Recorder()
        val relay = relayWith(recorder)
        relay.commitText("a\nb\n\nc")
        assertEquals(listOf("a", "b", "c"), recorder.texts)
        assertEquals(3, recorder.enters)
    }

    @Test
    fun `commitText의 개행만 있는 입력은 Enter가 된다`() {
        val recorder = Recorder()
        val relay = relayWith(recorder)
        relay.commitText("\n")
        assertEquals(0, recorder.texts.size)
        assertEquals(1, recorder.enters)
    }

    @Test
    fun `긴 커밋은 코드포인트 경계에서 200바이트로 나뉜다`() {
        val recorder = Recorder()
        val relay = relayWith(recorder)
        val hangul = "가".repeat(140) // 3바이트 × 140 = 420바이트
        relay.commitText(hangul)
        assertEquals(3, recorder.texts.size)
        // 경계가 코드포인트를 가르지 않는다 — 모든 청크는 온전한 Hangul 음절
        assertEquals(hangul, recorder.texts.joinToString(""))
        assertTrue(recorder.texts.all { chunk -> chunk.toByteArray(Charsets.UTF_8).size <= 200 })
        // 서로게이트 페어(이모지)도 중간에서 깨지지 않는다
        recorder.texts.clear()
        relay.commitText("😀".repeat(80)) // 4바이트 × 80 = 320바이트
        assertEquals("😀".repeat(80), recorder.texts.joinToString(""))
    }

    @Test
    fun `조합 중 텍스트는 전송되지 않고 커밋에서만 전송된다`() {
        val recorder = Recorder()
        val relay = relayWith(recorder)
        relay.setComposingText("ㄱ")
        relay.setComposingText("가")
        relay.setComposingText("간")
        assertEquals(0, recorder.texts.size)
        relay.commitText("간 ")
        assertEquals(listOf("간 "), recorder.texts)
    }

    @Test
    fun `finishComposingText는 버퍼를 확정 전송한다`() {
        val recorder = Recorder()
        val relay = relayWith(recorder)
        relay.setComposingText("안녕")
        relay.finishComposingText()
        assertEquals(listOf("안녕"), recorder.texts)
        // 두 번 호출하면 이중 전송되지 않는다
        relay.finishComposingText()
        assertEquals(1, recorder.texts.size)
    }

    @Test
    fun `deleteSurroundingText는 백스페이스와 forward delete로 간다`() {
        val recorder = Recorder()
        val relay = relayWith(recorder)
        relay.deleteSurroundingText(2, 0)
        assertEquals(listOf(2), recorder.backspaces)
        relay.deleteSurroundingText(0, 1)
        assertEquals(listOf(1), recorder.forwardDeletes)
        relay.deleteSurroundingText(0, 0)
        assertEquals(1, recorder.backspaces.size)
    }

    @Test
    fun `과도한 삭제 요청은 상한으로 잘린다`() {
        val recorder = Recorder()
        val relay = relayWith(recorder)
        relay.deleteSurroundingText(10_000, 0)
        assertEquals(listOf(64), recorder.backspaces)
    }

    @Test
    fun `키 이벤트는 다운만 처리해 중복을 막는다`() {
        val recorder = Recorder()
        val relay = relayWith(recorder)
        relay.handleKeyCode(TextInputRelay.KEYCODE_DEL, TextInputRelay.ACTION_DOWN)
        relay.handleKeyCode(TextInputRelay.KEYCODE_DEL, 1) // ACTION_UP
        assertEquals(listOf(1), recorder.backspaces)
        relay.handleKeyCode(TextInputRelay.KEYCODE_ENTER, TextInputRelay.ACTION_DOWN)
        assertEquals(1, recorder.enters)
        relay.handleKeyCode(999, TextInputRelay.ACTION_DOWN)
        assertEquals(1, recorder.enters)
    }

    @Test
    fun `편집기 액션은 액션 코드와 무관하게 Enter가 된다`() {
        val recorder = Recorder()
        val relay = relayWith(recorder)
        relay.performEditorAction(0)
        relay.performEditorAction(2)
        relay.performEditorAction(3)
        assertEquals(3, recorder.enters)
    }
}
