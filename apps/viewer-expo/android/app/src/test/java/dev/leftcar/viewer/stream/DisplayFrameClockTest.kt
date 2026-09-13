package dev.leftcar.viewer.stream

import android.view.Choreographer
import org.junit.Assert.*
import org.junit.Test

class DisplayFrameClockTest {
    @Test fun `actual callback timestamps preserve 60 90 120 display epochs and retired generations never publish`() {
        for (rate in listOf(60f,90f,120f)) {
            val posted = mutableListOf<Choreographer.FrameCallback>()
            val removed = mutableListOf<Choreographer.FrameCallback>()
            val samples = mutableListOf<Triple<Int,Long,Long>>()
            var screen = 3 to rate
            val clock = DisplayFrameClock({posted += it}, {removed += it}, {screen}, {d,f,p -> samples += Triple(d,f,p)})
            clock.start()
            val old = posted.last()
            old.doFrame(9_876_543_210L)
            assertEquals(Triple(3,9_876_543_210L,(1e9/rate).toLong()),samples.single())
            clock.stop()
            val count = posted.size
            old.doFrame(9_900_000_000L)
            assertEquals(1,samples.size)
            assertEquals(count,posted.size)
            assertSame(old,removed.last())
            screen = 9 to 120f
            clock.start()
            old.doFrame(10_000_000_000L)
            posted.last().doFrame(1_000_000_001L)
            assertEquals(Triple(9,1_000_000_001L,8_333_333L),samples.last())
            assertEquals(2,samples.size)
        }
    }
    @Test fun `invalid display resets clock instead of inventing a 60Hz epoch`() {
        lateinit var callback: Choreographer.FrameCallback
        val samples=mutableListOf<Long>()
        val clock=DisplayFrameClock({callback=it},{},{null},{_,frame,_ -> samples += frame})
        clock.start(); callback.doFrame(999L)
        assertEquals(listOf(0L),samples)
    }
    @Test fun `advertised limits are bounded and malformed values are unavailable`() {
        assertEquals(1, conservativeDecoderHint("codec",null,null).maxInstances)
        assertEquals(4, conservativeDecoderHint("codec",99,Double.NaN).maxInstances)
        assertNull(conservativeDecoderHint("codec",0,-1.0).maxInstancePixelRate)
        assertEquals(2,conservativeDecoderHint("codec",2,248_832_000.0).maxInstances)
    }
}
