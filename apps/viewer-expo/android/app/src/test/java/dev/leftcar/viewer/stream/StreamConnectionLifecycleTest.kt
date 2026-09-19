package dev.leftcar.viewer.stream

import android.content.Intent
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

/** Exercises Activity callers, including two independently owned windows. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE, shadows = [PresentationNativeShadow::class,
    ValidSurfaceShadow::class, PresentationSurfaceViewShadow::class])
class StreamConnectionLifecycleTest {
    private fun window(id: String) = Robolectric.buildActivity(StreamActivity::class.java,
        Intent().putExtra("instance", id).putExtra("host", "100.80.133.120")
            .putExtra("port", 5000)).create().start()

    @Test fun `hidden window retains its network service until destruction`() {
        val app = shadowOf(RuntimeEnvironment.getApplication())
        val window = window("background-window")
        try {
            assertEquals("dev.leftcar.viewer.stream.StreamConnectionService",
                app.nextStartedService?.component?.className)
            window.stop()
            assertNull("Surface hiding is not a connection close", app.nextStoppedService)
        } finally { window.destroy() }
        assertEquals("dev.leftcar.viewer.stream.StreamConnectionService",
            app.nextStoppedService?.component?.className)
    }

    @Test fun `closing one of two windows does not stop the other connection`() {
        val app = shadowOf(RuntimeEnvironment.getApplication())
        val first = window("first-window")
        val second = window("second-window")
        first.stop().destroy()
        assertNull("another window still owns the service", app.nextStoppedService)
        second.stop().destroy()
        assertEquals("dev.leftcar.viewer.stream.StreamConnectionService",
            app.nextStoppedService?.component?.className)
    }

    @Test fun `repeated foreground visits do not keep a closed window alive`() {
        val app = shadowOf(RuntimeEnvironment.getApplication())
        val window = window("revisited-window")
        window.stop().start().stop().start().stop().destroy()
        assertEquals("dev.leftcar.viewer.stream.StreamConnectionService",
            app.nextStoppedService?.component?.className)
        assertTrue(StreamConnectionOwners.isEmpty)
    }

}
