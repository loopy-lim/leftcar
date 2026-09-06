package dev.leftcar.viewer.stream

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class StreamWindowGeometryTest {
    @Test fun landscapeSourceFitsAvailableSpaceAndInsets() {
        val bounds = initialStreamWindowBounds(1920, 1080, 2400, 1600, 0, 80, 0, 40)
        assertNotNull(bounds)
        assertEquals(16.0 / 9.0, bounds!!.width.toDouble() / bounds.height, 0.01)
        assertEquals(2160, bounds.width)
        assertEquals(1215, bounds.height)
        assertEquals(120, bounds.left)
        assertEquals(212, bounds.top)
    }

    @Test fun portraitSourceKeepsPortraitRatio() {
        val bounds = initialStreamWindowBounds(1080, 1920, 1200, 2200)
        assertNotNull(bounds)
        assertEquals(1080.0 / 1920.0, bounds!!.width.toDouble() / bounds.height, 0.01)
        assertEquals(1080, bounds.width)
        assertEquals(1920, bounds.height)
    }

    @Test fun invalidOrExtremeInputsRemainBounded() {
        val bounds = initialStreamWindowBounds(Int.MAX_VALUE, 1, 1000, 800)
        assertNotNull(bounds)
        assertEquals(900, bounds!!.width)
        assertEquals(1, bounds.height)
        assertNull(initialStreamWindowBounds(1920, 1080, 0, 100))
    }

    @Test fun aspectFitMappingClampsLetterboxInsets() {
        assertEquals(0f, mapAspectFitPoint(0f, 0f, 1600, 1200, 16, 9).first, 0.001f)
        assertEquals(0.5f, mapAspectFitPoint(800f, 600f, 1600, 1200, 16, 9).first, 0.001f)
        assertEquals(0f, mapAspectFitPoint(800f, 0f, 1600, 1200, 16, 9).second, 0.001f)
    }
}
