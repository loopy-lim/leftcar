package dev.leftcar.viewer.stream

import android.content.DialogInterface
import android.content.Intent
import android.view.InputDevice
import android.view.KeyEvent
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.Shadows.shadowOf
import android.os.Looper
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowAlertDialog
import org.robolectric.util.ReflectionHelpers
import org.robolectric.util.ReflectionHelpers.ClassParameter.from

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE, shadows = [PresentationNativeShadow::class, ValidSurfaceShadow::class, PresentationSurfaceViewShadow::class])
class StreamWindowSafetyTest {
    @Test fun `mouse back events cannot close the stream or open an exit dialog`() {
        val controller = Robolectric.buildActivity(StreamActivity::class.java,
            Intent().putExtra("instance", "mouse-back-safety").putExtra("host", "192.168.0.2")).create().start().resume()
        val activity = controller.get()
        for (action in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP)) {
            val event = KeyEvent(1L, 2L, action, KeyEvent.KEYCODE_BACK, 0, 0, 3, 0, 0, InputDevice.SOURCE_MOUSE)
            assertTrue(activity.dispatchKeyEvent(event))
        }
        assertFalse(activity.isFinishing)
        assertNull(ShadowAlertDialog.getLatestAlertDialog())
        controller.pause().stop().destroy()
    }

    @Test fun `receiver feedback timeout retains the stream window for reconnection`() {
        val controller = Robolectric.buildActivity(StreamActivity::class.java,
            Intent().putExtra("instance", "feedback-safety").putExtra("host", "192.168.0.2")).create()
        ReflectionHelpers.callInstanceMethod<Void>(controller.get(), "handleTermination", from(Int::class.javaPrimitiveType!!, 1))
        assertFalse(controller.get().isFinishing)
        controller.destroy()
    }

    @Test fun `back keeps the actual stream window until close is explicitly selected`() {
        val controller = Robolectric.buildActivity(StreamActivity::class.java,
            Intent().putExtra("instance", "back-safety").putExtra("host", "192.168.0.2")).create().start().resume()
        val activity = controller.get()
        activity.onBackPressedDispatcher.onBackPressed()
        assertFalse("An incidental Back must not finish the stream Activity", activity.isFinishing)
        val dialog = ShadowAlertDialog.getLatestAlertDialog()
        assertNotNull("Back must leave an explicit way to close", dialog)
        dialog.getButton(DialogInterface.BUTTON_NEGATIVE).performClick()
        shadowOf(Looper.getMainLooper()).idle()
        assertFalse(activity.isFinishing)
        activity.onBackPressedDispatcher.onBackPressed()
        ShadowAlertDialog.getLatestAlertDialog().getButton(DialogInterface.BUTTON_POSITIVE).performClick()
        shadowOf(Looper.getMainLooper()).idle()
        assertTrue(activity.isFinishing)
        controller.pause().stop().destroy()
    }
}
