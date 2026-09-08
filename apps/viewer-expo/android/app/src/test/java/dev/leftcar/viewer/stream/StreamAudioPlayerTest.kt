package dev.leftcar.viewer.stream

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class StreamAudioPlayerTest {
    @Test
    fun `drain blob header parses rate and channels`() {
        assertEquals(48_000 to 2, parseAudioBlobHeader(byteArrayOf(0xbb.toByte(), 0x80.toByte(), 2, 0, 0, 4)))
        assertEquals(44_100 to 1, parseAudioBlobHeader(byteArrayOf(0xac.toByte(), 0x44, 1, 0, 0, 1)))
    }

    @Test
    fun `malformed headers are rejected`() {
        assertNull(parseAudioBlobHeader(byteArrayOf(1, 2, 3)))
        assertNull(parseAudioBlobHeader(byteArrayOf(0, 0, 2, 0, 0, 4)))
        assertNull(parseAudioBlobHeader(byteArrayOf(0x30, 0x39, 0, 0, 0, 4)))
        assertNull(parseAudioBlobHeader(byteArrayOf(0x30, 0x39, 3, 0, 0, 4)))
    }

}
