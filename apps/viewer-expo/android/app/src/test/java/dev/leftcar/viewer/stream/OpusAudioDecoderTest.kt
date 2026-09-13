package dev.leftcar.viewer.stream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import org.junit.Assert.*
import org.junit.Test
class OpusAudioDecoderTest {
    @Test fun `Android CSD is OpusHead with negotiated delay not Apple magic cookie`() {
        val csd = opusInitializationData(2, 312)
        assertEquals("OpusHead", String(csd[0].copyOfRange(0, 8), Charsets.US_ASCII))
        assertEquals(19, csd[0].size)
        assertEquals(2, csd[0][9].toInt())
        assertEquals(312, ByteBuffer.wrap(csd[0], 10, 2).order(ByteOrder.LITTLE_ENDIAN).short.toInt())
        assertEquals(6_500_000L, ByteBuffer.wrap(csd[1]).order(ByteOrder.nativeOrder()).long)
        assertEquals(80_000_000L, ByteBuffer.wrap(csd[2]).order(ByteOrder.nativeOrder()).long)
    }
}
