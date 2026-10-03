package dev.leftcar.viewer.stream

import android.content.Intent
import android.view.SurfaceHolder
import android.content.DialogInterface
import org.robolectric.shadows.ShadowAlertDialog
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.util.ReflectionHelpers
import org.robolectric.util.ReflectionHelpers.ClassParameter.from

/** Actual Activity Surface callbacks; JNI and the React event boundary are isolated. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE, shadows = [PresentationNativeShadow::class,
    ValidSurfaceShadow::class, PresentationSurfaceViewShadow::class, SplitRecoveryEventsShadow::class])
class StreamSplitResumeTest {
    private fun intent() = Intent().putExtra("instance", "split-resume")
        .putExtra("host", "100.80.133.120").putExtra("port", 5010)
        .putExtra("width", 3840).putExtra("height", 2160)
        .putExtra("splitVertical", true).putExtra("splitDecoderName", "c2.qti.avc.decoder")

    @Before fun reset() {
        PresentationNativeShadow.attachResult = 0
        PresentationNativeShadow.splitAttachCalls = 0
        SplitRecoveryEventsShadow.events.clear()
    }

    private fun holders(activity: StreamActivity) =
        ReflectionHelpers.getField<StreamSurfaces>(activity, "streamSurfaces").holders

    private fun attach(activity: StreamActivity) {
        val gate = ReflectionHelpers.getField<StreamSurfaceLifecycleGate<SurfaceHolder>>(activity, "surfaceLifecycle")
        ReflectionHelpers.callInstanceMethod<Void>(activity, "attachStableSurfaces",
            from(Int::class.javaPrimitiveType, gate.currentGeneration))
    }

    @Test fun `explicit close retires the logical stream before finishing and notifies only once`() {
        val controller = Robolectric.buildActivity(StreamActivity::class.java, intent()).create().start().resume()
        val activity = controller.get()
        activity.onBackPressedDispatcher.onBackPressed()
        ShadowAlertDialog.getLatestAlertDialog().getButton(DialogInterface.BUTTON_POSITIVE).performClick()
        org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
        assertEquals(listOf(5010 to 0), SplitRecoveryEventsShadow.events)
        assertTrue(activity.isFinishing)
        controller.pause().stop().destroy()
        assertEquals(listOf(5010 to 0), SplitRecoveryEventsShadow.events)
    }

    @Test fun `task removal retires its logical stream without a Back dialog`() {
        val controller = Robolectric.buildActivity(StreamActivity::class.java, intent()).create()
        controller.get().finish()
        controller.destroy()
        assertEquals(listOf(5010 to 0), SplitRecoveryEventsShadow.events)
    }

    @Test fun `non final Activity recreation does not retire its logical stream`() {
        val controller = Robolectric.buildActivity(StreamActivity::class.java, intent()).create()
        controller.destroy()
        assertTrue(SplitRecoveryEventsShadow.events.isEmpty())
    }

    @Test fun `returning split surfaces request fresh transport once and keep the same window`() {
        val controller = Robolectric.buildActivity(StreamActivity::class.java, intent()).create()
        val activity = controller.get()
        val previous = holders(activity)
        previous.forEach(activity::surfaceCreated)
        attach(activity)
        assertEquals(1, PresentationNativeShadow.splitAttachCalls)
        previous.forEach(activity::surfaceDestroyed)
        previous.forEach(activity::surfaceCreated)
        attach(activity)
        attach(activity)
        assertEquals("detached receivers and keys cannot be reused", 1, PresentationNativeShadow.splitAttachCalls)
        assertEquals(listOf(5010 to 5), SplitRecoveryEventsShadow.events)
        assertFalse(activity.isFinishing)

        // Only a fresh Host preparation (reconnect intent) permits another attach.
        controller.newIntent(intent().putExtra("reconnect", true))
        previous.forEach(activity::surfaceDestroyed) // late retired callbacks
        holders(activity).forEach(activity::surfaceCreated)
        attach(activity)
        assertEquals(2, PresentationNativeShadow.splitAttachCalls)
        assertTrue(ReflectionHelpers.getField<StreamSurfaceLifecycleGate<SurfaceHolder>>(activity, "surfaceLifecycle").isAttached)
        assertEquals(listOf(5010 to 5), SplitRecoveryEventsShadow.events)
        assertSame(activity, controller.get())
        controller.destroy()
    }

    @Test fun `failed split attach requests fresh preparation instead of leaving a black window`() {
        val controller = Robolectric.buildActivity(StreamActivity::class.java, intent()).create()
        val activity = controller.get()
        holders(activity).forEach(activity::surfaceCreated)
        PresentationNativeShadow.attachResult = -2 // prepared receiver absent/consumed
        attach(activity)
        attach(activity)
        assertEquals(1, PresentationNativeShadow.splitAttachCalls)
        assertEquals(listOf(5010 to 5), SplitRecoveryEventsShadow.events)
        assertFalse(activity.isFinishing)

        controller.newIntent(intent().putExtra("reconnect", true))
        holders(activity).forEach(activity::surfaceCreated)
        attach(activity) // fresh preparation also failed: allow the next bounded retry
        assertEquals(listOf(5010 to 5, 5010 to 5), SplitRecoveryEventsShadow.events)
        controller.destroy()
    }
}

@Implements(value = StreamLauncherModule.Companion::class, isInAndroidSdk = false)
class SplitRecoveryEventsShadow {
    companion object { val events = mutableListOf<Pair<Int, Int>>() }
    @Implementation fun emitTermination(port: Int, reason: Int) { events += port to reason }
    @Implementation fun emitTermination(port: Int, reason: Int, generation: Long) { events += port to reason }
    @Implementation fun emitWindowClosed(instanceId: String, generation: Long, port: Int) { events += port to 0 }
}
