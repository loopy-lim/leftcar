package dev.leftcar.viewer.stream

import android.util.DisplayMetrics
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PhysicalDisplayMetricsTest {
    private fun metrics(width: Int, height: Int, dpi: Int): DisplayMetrics =
        DisplayMetrics().apply {
            widthPixels = width
            heightPixels = height
            densityDpi = dpi
        }

    @Test
    fun panelMetricsAreReportedAsPhysicalPixels() {
        val payload = physicalDisplayMetricsFrom(metrics(2560, 1600, 278))

        assertEquals(PhysicalDisplayMetrics(2560, 1600, 278), payload)
    }

    @Test
    fun rotatedPanelIsReportedAsMeasured() {
        // real metrics는 회전을 따라 바뀐다 — 정규화는 호스트 매칭 몫이다.
        val payload = physicalDisplayMetricsFrom(metrics(1600, 2560, 278))

        assertEquals(PhysicalDisplayMetrics(1600, 2560, 278), payload)
    }

    @Test
    fun invalidMetricsAreDroppedSoTheViewerOmitsTheField() {
        assertNull(physicalDisplayMetricsFrom(metrics(0, 1600, 278)))
        assertNull(physicalDisplayMetricsFrom(metrics(2560, 0, 278)))
        assertNull(physicalDisplayMetricsFrom(metrics(2560, 1600, 0)))
    }
}
