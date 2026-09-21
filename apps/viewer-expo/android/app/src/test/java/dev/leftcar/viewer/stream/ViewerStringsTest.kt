package dev.leftcar.viewer.stream

import org.junit.Assert.assertTrue
import org.junit.Test

class ViewerStringsTest {
    @Test
    fun `입력 잠김 배너는 두 언어 모두 요청 방법과 승인 장소를 알려준다`() {
        ViewerStrings.applyLanguage("ko")
        assertTrue(ViewerStrings.inputLockedBanner.contains("허용 요청"))
        assertTrue(ViewerStrings.inputRequestSent.contains("Leftcar Host"))
        assertTrue(ViewerStrings.inputRequestSent.contains("허용"))
        ViewerStrings.applyLanguage("en")
        assertTrue(ViewerStrings.inputLockedBanner.contains("request"))
        assertTrue(ViewerStrings.inputRequestSent.contains("Leftcar Host"))
        assertTrue(ViewerStrings.inputRequestSent.contains("Allow"))
        ViewerStrings.applyLanguage("ko")
    }

}
