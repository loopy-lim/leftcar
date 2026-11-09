package dev.leftcar.viewer.km

import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE)
class KmDuplexTest {
    @Test fun `wire rejects unknown fields and numeric string coercion`() {
        fun frame(payload: String) =
            """{"version":1,"sessionId":"s","epoch":1,"seq":1,"origin":"mac","payload":$payload}""".toByteArray()
        assertThrows(IllegalArgumentException::class.java) {
            KmWire.decode(frame("""{"kind":"key","code":"4","down":true}"""), "s")
        }
        assertThrows(IllegalArgumentException::class.java) {
            KmWire.decode(frame("""{"kind":"heartbeat","ignored":true}"""), "s")
        }
        assertThrows(IllegalArgumentException::class.java) {
            KmWire.decode(frame("""{"kind":"move","dx":1e100,"dy":0}"""), "s")
        }
    }
}
