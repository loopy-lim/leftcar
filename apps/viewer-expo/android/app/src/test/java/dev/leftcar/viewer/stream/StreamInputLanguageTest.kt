package dev.leftcar.viewer.stream

import org.junit.Assert.*
import org.junit.Test

class StreamInputLanguageTest {
    @Test fun `Korean subtype takes priority over its English physical keyboard layout hint`() {
        assertEquals(2, streamInputLanguage("ko-KR", "ko_KR", "en-Latn-US"))
        assertEquals(1, streamInputLanguage("", "", "en-Latn-US"))
        assertEquals(2, streamInputLanguage("", "ko_KR", "en"))
        assertNull(streamInputLanguage("ja-JP", "", "en"))
        assertNull(streamInputLanguage("", "", ""))
    }

    @Test fun `only active streams send language and retries do not toggle twice`() {
        val sync = StreamInputLanguageSync()
        val sent = mutableListOf<Int>()
        val send: (Int) -> Boolean = { sent.add(it); true }
        assertFalse(sync.update(2, false, send))
        assertTrue(sent.isEmpty())
        assertTrue(sync.update(2, true, send))
        assertTrue(sync.update(2, true, send))
        assertTrue(sync.update(1, true, send))
        assertEquals(listOf(2, 1), sent)
        sync.update(1, false, send)
        assertTrue(sync.update(1, true, send))
        assertEquals(listOf(2, 1, 1), sent)
    }

    @Test fun `capability not ready is retried before the next hardware key`() {
        val sync = StreamInputLanguageSync()
        assertFalse(sync.update(2, true) { false })
        val order = mutableListOf<String>()
        assertTrue(sync.update(2, true) { order.add("language:$it"); true })
        order.add("key")
        assertEquals(listOf("language:2", "key"), order)
        sync.reset()
        assertFalse(sync.update(null, true) { fail("unknown language must not be sent"); true })
    }
}
