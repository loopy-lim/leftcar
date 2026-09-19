package dev.leftcar.viewer.stream

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

}
