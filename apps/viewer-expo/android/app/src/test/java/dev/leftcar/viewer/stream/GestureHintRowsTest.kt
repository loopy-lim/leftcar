package dev.leftcar.viewer.stream

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class GestureHintRowsTest {
    @Test
    fun `안내는 네 가지 핵심 제스처를 모두 다룬다`() {
        assertEquals(4, GestureHintRows.rows("ko").size)
        assertEquals(4, GestureHintRows.rows("en").size)
    }

    @Test
    fun `각 행은 제스처와 동작을 짝지은 안내 문구다`() {
        val actions = GestureHintRows.rows("ko").map { it.second }
        assertTrue("클릭" in actions)
        assertTrue("드래그" in actions)
        assertTrue("스크롤" in actions)
        assertTrue("오른쪽 클릭" in actions)
        GestureHintRows.rows("ko").forEach { (gesture, action) ->
            assertTrue(gesture.isNotBlank())
            assertTrue(action.isNotBlank())
        }
    }

    @Test
    fun `길게 누르기 우클릭 안내가 발견 가능성 문제의 핵심을 담는다`() {
        val longPress = GestureHintRows.rows("ko").firstOrNull { it.first == "길게 누르기" }
        assertEquals("오른쪽 클릭", longPress?.second)
    }

    @Test
    fun `영어 행도 같은 네 동작을 짝지은 안내 문구다`() {
        val actions = GestureHintRows.rows("en").map { it.second }
        assertEquals(listOf("Click", "Drag", "Scroll", "Right-click"), actions)
        val longPress = GestureHintRows.rows("en").firstOrNull { it.first == "Long press" }
        assertEquals("Right-click", longPress?.second)
    }

    @Test
    fun `알 수 없는 언어는 한국어로 귀결된다`() {
        assertEquals(GestureHintRows.rows("ko"), GestureHintRows.rows("fr"))
    }
}
