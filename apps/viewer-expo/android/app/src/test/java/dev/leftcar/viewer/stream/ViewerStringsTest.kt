package dev.leftcar.viewer.stream

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ViewerStringsTest {
    @Test
    fun `입력 잠김 배너는 두 언어 모두 승인 장소를 알려준다`() {
        ViewerStrings.applyLanguage("ko")
        assertTrue(ViewerStrings.inputLockedBanner.contains("Leftcar Host"))
        assertTrue(ViewerStrings.inputLockedBanner.contains("허용"))
        ViewerStrings.applyLanguage("en")
        assertTrue(ViewerStrings.inputLockedBanner.contains("Leftcar Host"))
        assertTrue(ViewerStrings.inputLockedBanner.contains("Allow"))
    }

    @Test
    fun `제스처 재열람 칩의 접근성 문구는 언어를 따른다`() {
        ViewerStrings.applyLanguage("ko")
        assertEquals("터치 제스처 안내 보기", ViewerStrings.gestureHelpDescription)
        ViewerStrings.applyLanguage("en")
        assertEquals("Show touch gestures", ViewerStrings.gestureHelpDescription)
    }
}
