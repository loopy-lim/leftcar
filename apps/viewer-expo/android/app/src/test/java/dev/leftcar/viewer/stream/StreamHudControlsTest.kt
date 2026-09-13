package dev.leftcar.viewer.stream

import android.app.Activity
import android.view.View
import android.widget.LinearLayout
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE)
class StreamHudControlsTest {
    @Test fun `measured controls have distinct minimum touch targets and spacing at every panel scale`() {
        val activity = Robolectric.buildActivity(Activity::class.java).setup().get()
        for (scale in listOf(1f, 1.25f)) {
            val taps = mutableListOf<String>()
            val controls = StreamHudControls(activity, scale)
            controls.addChip("?", "Help") { taps += "help" }
            controls.addChip("ABC", "Keyboard") { taps += "keyboard" }
            controls.addChip("×", "Close stream") { taps += "close" }
            controls.view.measure(View.MeasureSpec.makeMeasureSpec(400, View.MeasureSpec.AT_MOST), View.MeasureSpec.makeMeasureSpec(1200, View.MeasureSpec.AT_MOST))
            controls.view.layout(0, 0, controls.view.measuredWidth, controls.view.measuredHeight)
            val minimum = (48 * activity.resources.displayMetrics.density).toInt()
            var bottom = 0
            for (index in 0 until controls.view.childCount) {
                val chip = controls.view.getChildAt(index)
                assertTrue(chip.width >= minimum)
                assertTrue(chip.height >= minimum)
                if (index > 0) assertTrue(chip.top > bottom)
                bottom = chip.bottom
                assertTrue(chip.performClick())
            }
            assertEquals(listOf("help", "keyboard", "close"), taps)
        }
    }
}
