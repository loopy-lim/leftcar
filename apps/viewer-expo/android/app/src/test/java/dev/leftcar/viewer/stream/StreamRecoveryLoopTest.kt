package dev.leftcar.viewer.stream

import android.content.Intent
import android.os.Looper
import android.view.SurfaceHolder
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.util.ReflectionHelpers
import org.robolectric.util.ReflectionHelpers.ClassParameter.from
import java.util.concurrent.TimeUnit

/** Drive the actual Activity retries and watchdog timers; only JNI/events are isolated. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE, shadows = [PresentationNativeShadow::class,
    ValidSurfaceShadow::class, PresentationSurfaceViewShadow::class, SplitRecoveryEventsShadow::class])
class StreamRecoveryLoopTest {
    private fun intent() = Intent().putExtra("instance", "recovery-loop")
        .putExtra("host", "192.168.0.2").putExtra("port", 5020).putExtra("showFps", false)

    @Before fun reset() {
        PresentationNativeShadow.attachResult = 0
        PresentationNativeShadow.rebindCalls = 0
        PresentationNativeShadow.surfaceAttachCalls = 0
        PresentationNativeShadow.renderedFrames = 0L
        SplitRecoveryEventsShadow.events.clear()
    }

    @Test fun `successful renderer spawns without frames exhaust the six attempt budget`() {
        val controller = Robolectric.buildActivity(StreamActivity::class.java, intent()).create()
        try {
            ReflectionHelpers.callInstanceMethod<Void>(controller.get(), "handleTermination",
                from(Int::class.javaPrimitiveType!!, 5))
            // Six 4s no-progress watchdogs plus 7.75s of retry delays.
            shadowOf(Looper.getMainLooper()).idleFor(60, TimeUnit.SECONDS)
            assertEquals("JNI success must not replenish a stalled renderer's budget", 6,
                PresentationNativeShadow.rebindCalls)
            assertEquals(listOf(5020 to 5), SplitRecoveryEventsShadow.events)
            assertFalse("The bounded handoff keeps the same window", controller.get().isFinishing)
        } finally {
            controller.destroy()
        }
    }

    @Test fun `verified frame progress replenishes the budget with diagnostics hidden`() {
        val controller = Robolectric.buildActivity(StreamActivity::class.java, intent()).create()
        try {
            ReflectionHelpers.callInstanceMethod<Void>(controller.get(), "handleTermination",
                from(Int::class.javaPrimitiveType!!, 5))
            shadowOf(Looper.getMainLooper()).idle()
            shadowOf(Looper.getMainLooper()).idleFor(4_250, TimeUnit.MILLISECONDS)
            assertEquals(2, PresentationNativeShadow.rebindCalls)
            PresentationNativeShadow.renderedFrames = 1L
            shadowOf(Looper.getMainLooper()).idleFor(250, TimeUnit.MILLISECONDS)
            shadowOf(Looper.getMainLooper()).idleFor(10, TimeUnit.SECONDS)
            assertEquals("Forward progress must cancel the incumbent watchdog", 2,
                PresentationNativeShadow.rebindCalls)
            ReflectionHelpers.callInstanceMethod<Void>(controller.get(), "handleTermination",
                from(Int::class.javaPrimitiveType!!, 5))
            shadowOf(Looper.getMainLooper()).idleFor(60, TimeUnit.SECONDS)
            assertEquals("A new failure after verified progress receives a fresh six attempts", 8,
                PresentationNativeShadow.rebindCalls)
            assertEquals(listOf(5020 to 5), SplitRecoveryEventsShadow.events)
        } finally {
            controller.destroy()
        }
    }

    @Test fun `Host stops terminate the window without recovering an incumbent watchdog`() {
        for (reason in listOf(2, 3)) {
            PresentationNativeShadow.rebindCalls = 0
            val controller = Robolectric.buildActivity(StreamActivity::class.java, intent()).create()
            try {
                ReflectionHelpers.callInstanceMethod<Void>(controller.get(), "handleTermination",
                    from(Int::class.javaPrimitiveType!!, 5))
                shadowOf(Looper.getMainLooper()).idle()
                ReflectionHelpers.callInstanceMethod<Void>(controller.get(), "handleTermination",
                    from(Int::class.javaPrimitiveType!!, reason))
                assertTrue(controller.get().isFinishing)
                shadowOf(Looper.getMainLooper()).idleFor(60, TimeUnit.SECONDS)
                assertEquals(1, PresentationNativeShadow.rebindCalls)
            } finally {
                controller.destroy()
            }
        }
        assertFalse(SplitRecoveryEventsShadow.events.any { it.second == 5 })
    }

    @Test fun `replacement generation cancels an incumbent delayed retry`() {
        val id = "recovery-retry-owner"
        val leases = ReflectionHelpers.getStaticField<MutableMap<String, StreamCloseLease>>(StreamLauncherModule::class.java, "closeLeases")
        leases[id] = StreamCloseLease("17")
        val controller = Robolectric.buildActivity(StreamActivity::class.java,
            intent().putExtra("instance", id).putExtra("ownershipGeneration", 17L)).create()
        try {
            PresentationNativeShadow.attachResult = -1
            ReflectionHelpers.callInstanceMethod<Void>(controller.get(), "handleTermination",
                from(Int::class.javaPrimitiveType!!, 5))
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals(1, PresentationNativeShadow.rebindCalls)
            leases[id] = StreamCloseLease("18")
            PresentationNativeShadow.attachResult = 0
            controller.newIntent(intent().putExtra("instance", id).putExtra("port", 5021)
                .putExtra("ownershipGeneration", 18L).putExtra("reconnect", true))
            assertEquals(2, PresentationNativeShadow.rebindCalls)
            shadowOf(Looper.getMainLooper()).idleFor(250, TimeUnit.MILLISECONDS)
            assertEquals("The old retry must not spawn another renderer on the successor", 2,
                PresentationNativeShadow.rebindCalls)
            assertTrue(SplitRecoveryEventsShadow.events.isEmpty())
        } finally {
            controller.destroy()
        }
    }

    @Test fun `replacement generation cancels an incumbent no progress watchdog`() {
        val id = "recovery-watchdog-owner"
        val leases = ReflectionHelpers.getStaticField<MutableMap<String, StreamCloseLease>>(StreamLauncherModule::class.java, "closeLeases")
        leases[id] = StreamCloseLease("17")
        val controller = Robolectric.buildActivity(StreamActivity::class.java,
            intent().putExtra("instance", id).putExtra("ownershipGeneration", 17L)).create()
        try {
            ReflectionHelpers.callInstanceMethod<Void>(controller.get(), "handleTermination",
                from(Int::class.javaPrimitiveType!!, 5))
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals(1, PresentationNativeShadow.rebindCalls)
            leases[id] = StreamCloseLease("18")
            controller.newIntent(intent().putExtra("instance", id).putExtra("port", 5021)
                .putExtra("ownershipGeneration", 18L).putExtra("reconnect", true))
            shadowOf(Looper.getMainLooper()).idleFor(4_000, TimeUnit.MILLISECONDS)
            assertEquals("The old watchdog must not restart the successor", 2,
                PresentationNativeShadow.rebindCalls)
            assertTrue(SplitRecoveryEventsShadow.events.isEmpty())
        } finally {
            controller.destroy()
        }
    }

    @Test fun `Surface loss cancels a pending retry instead of spending the background budget`() {
        val controller = Robolectric.buildActivity(StreamActivity::class.java, intent()).create()
        try {
            PresentationNativeShadow.attachResult = -1
            ReflectionHelpers.callInstanceMethod<Void>(controller.get(), "handleTermination",
                from(Int::class.javaPrimitiveType!!, 5))
            shadowOf(Looper.getMainLooper()).idle()
            val surfaces = ReflectionHelpers.getField<StreamSurfaces>(controller.get(), "streamSurfaces")
            controller.get().surfaceDestroyed(surfaces.left.holder)
            shadowOf(Looper.getMainLooper()).idleFor(60, TimeUnit.SECONDS)
            assertEquals("A hidden Surface cannot consume renderer retries", 1,
                PresentationNativeShadow.rebindCalls)
            assertTrue(SplitRecoveryEventsShadow.events.isEmpty())
        } finally {
            controller.destroy()
        }
    }

    @Test fun `Surface loss cancels the old watchdog and fresh Surface creation resumes normal attach`() {
        val controller = Robolectric.buildActivity(StreamActivity::class.java, intent()).create()
        val activity = controller.get()
        try {
            ReflectionHelpers.callInstanceMethod<Void>(activity, "handleTermination",
                from(Int::class.javaPrimitiveType!!, 5))
            shadowOf(Looper.getMainLooper()).idle()
            val surfaces = ReflectionHelpers.getField<StreamSurfaces>(activity, "streamSurfaces")
            val holder = surfaces.left.holder
            activity.surfaceDestroyed(holder)
            shadowOf(Looper.getMainLooper()).idleFor(60, TimeUnit.SECONDS)
            assertEquals("A retired Surface cannot trigger a recovery watchdog", 1,
                PresentationNativeShadow.rebindCalls)
            assertTrue(SplitRecoveryEventsShadow.events.isEmpty())

            activity.surfaceCreated(holder)
            val gate = ReflectionHelpers.getField<StreamSurfaceLifecycleGate<SurfaceHolder>>(activity, "surfaceLifecycle")
            ReflectionHelpers.callInstanceMethod<Void>(activity, "attachStableSurfaces",
                from(Int::class.javaPrimitiveType!!, gate.currentGeneration))
            assertEquals(1, PresentationNativeShadow.surfaceAttachCalls)
            assertTrue(gate.isAttached)
            PresentationNativeShadow.renderedFrames = 1L
            shadowOf(Looper.getMainLooper()).idleFor(5, TimeUnit.SECONDS)
            assertEquals(1, PresentationNativeShadow.rebindCalls)
            assertTrue(SplitRecoveryEventsShadow.events.isEmpty())
            assertFalse(activity.isFinishing)
        } finally {
            controller.destroy()
        }
    }
}
