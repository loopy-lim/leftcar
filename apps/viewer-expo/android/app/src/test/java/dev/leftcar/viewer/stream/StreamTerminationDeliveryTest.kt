@file:Suppress("DEPRECATION")

package dev.leftcar.viewer.stream

import android.content.Context
import android.content.Intent
import android.os.Looper
import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.BridgeReactContext
import com.facebook.react.bridge.JavaOnlyMap
import com.facebook.react.bridge.JavaScriptModule
import com.facebook.react.bridge.ReadableMap
import com.facebook.react.bridge.WritableMap
import com.facebook.react.modules.core.DeviceEventManagerModule
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.util.ReflectionHelpers
import org.robolectric.util.ReflectionHelpers.ClassParameter.from
import java.util.concurrent.TimeUnit

/** Actual Launcher/Activity ownership; only RN's queue and native map allocation are isolated. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE, shadows = [PresentationNativeShadow::class,
    ValidSurfaceShadow::class, PresentationSurfaceViewShadow::class, TerminationArgumentsShadow::class])
class StreamTerminationDeliveryTest {
    private val port = 5022
    private val id = "src-$port"

    private fun own(generation: Long) {
        val streams = ReflectionHelpers.getStaticField<MutableMap<String, Any>>(StreamLauncherModule::class.java, "liveStreams")
        val type = Class.forName("dev.leftcar.viewer.stream.StreamLauncherModule\$StreamOwnership")
        val constructor = type.getDeclaredConstructor(String::class.java, Int::class.javaPrimitiveType, Long::class.javaPrimitiveType)
        constructor.isAccessible = true
        streams[id] = constructor.newInstance("192.168.0.2", port, generation)
        val leases = ReflectionHelpers.getStaticField<MutableMap<String, StreamCloseLease>>(StreamLauncherModule::class.java, "closeLeases")
        leases[id] = StreamCloseLease(generation.toString())
    }

    private fun streamIntent() = Intent().putExtra("instance", id).putExtra("host", "192.168.0.2")
        .putExtra("port", port).putExtra("ownershipGeneration", 17L).putExtra("splitVertical", true)
        .putExtra("splitDecoderName", "synthetic.decoder")

    @Test fun `split recovery waits for a returning React listener and delivers one exact generation event`() {
        val context = TerminationReactContext(RuntimeEnvironment.getApplication())
        val module = StreamLauncherModule(context)
        own(17)
        val controller = Robolectric.buildActivity(StreamActivity::class.java, streamIntent()).create()
        try {
            ReflectionHelpers.callInstanceMethod<Void>(controller.get(), "handleTermination",
                from(Int::class.javaPrimitiveType!!, 5))
            StreamLauncherModule.emitTermination(port, 5)
            assertTrue(context.events.isEmpty())
            context.active = true
            module.addListener("leftcarStreamTerminated")
            context.drain()
            assertEquals(1, context.events.size)
            val event = context.events.single()
            assertEquals("leftcarStreamTerminated", event.first)
            assertEquals(port, event.second.getInt("port"))
            assertEquals(5, event.second.getInt("reason"))
            assertEquals("17", event.second.getString("generation"))
            module.addListener("leftcarStreamTerminated")
            context.drain()
            assertEquals("Listener registration must not replay a delivered request", 1, context.events.size)
        } finally {
            controller.destroy()
            module.invalidate()
        }
    }

    @Test fun `a queued recovery cannot be delivered for a successor generation`() {
        val context = TerminationReactContext(RuntimeEnvironment.getApplication())
        val module = StreamLauncherModule(context)
        own(17)
        try {
            StreamLauncherModule.emitTermination(port, 5)
            own(18)
            context.active = true
            module.addListener("leftcarStreamTerminated")
            context.drain()
            assertTrue(context.events.isEmpty())
        } finally {
            StreamLauncherModule.forgetStream(id, 18)
            module.invalidate()
        }
    }

    @Test fun `an incumbent Activity cannot request recovery for a published successor`() {
        val context = TerminationReactContext(RuntimeEnvironment.getApplication()).apply { active = true }
        val module = StreamLauncherModule(context)
        own(17)
        val controller = Robolectric.buildActivity(StreamActivity::class.java, streamIntent()).create()
        try {
            module.addListener("leftcarStreamTerminated")
            own(18) // Host/React published its next lease; onNewIntent is not delivered yet.
            ReflectionHelpers.callInstanceMethod<Void>(controller.get(), "handleTermination",
                from(Int::class.javaPrimitiveType!!, 5))
            context.drain()
            assertTrue("The old Activity must not tag its failure with the new generation", context.events.isEmpty())
        } finally {
            controller.destroy()
            StreamLauncherModule.forgetStream(id, 18)
            module.invalidate()
        }
    }

    @Test fun `a closed window cancels its pending recovery before React returns`() {
        val context = TerminationReactContext(RuntimeEnvironment.getApplication())
        val module = StreamLauncherModule(context)
        own(17)
        val controller = Robolectric.buildActivity(StreamActivity::class.java, streamIntent()).create()
        try {
            ReflectionHelpers.callInstanceMethod<Void>(controller.get(), "handleTermination",
                from(Int::class.javaPrimitiveType!!, 5))
            controller.get().finish()
            controller.destroy()
            shadowOf(Looper.getMainLooper()).idle()
            context.active = true
            module.addListener("leftcarStreamTerminated")
            context.drain()
            assertTrue(context.events.isEmpty())
        } finally {
            StreamLauncherModule.forgetStream(id, 17)
            module.invalidate()
        }
    }

    @Test fun `rendered progress cancels a pending handoff before React returns`() {
        val context = TerminationReactContext(RuntimeEnvironment.getApplication())
        val module = StreamLauncherModule(context)
        own(17)
        PresentationNativeShadow.renderedFrames = 0L
        val controller = Robolectric.buildActivity(StreamActivity::class.java, streamIntent()).create().start().resume().visible()
        try {
            ReflectionHelpers.callInstanceMethod<Void>(controller.get(), "handleTermination",
                from(Int::class.javaPrimitiveType!!, 5))
            shadowOf(Looper.getMainLooper()).idle()
            PresentationNativeShadow.renderedFrames = 1L
            shadowOf(Looper.getMainLooper()).idleFor(250, TimeUnit.MILLISECONDS)
            context.active = true
            module.addListener("leftcarStreamTerminated")
            context.drain()
            assertTrue("A recovered renderer must not receive a delayed reconnect request", context.events.isEmpty())
        } finally {
            PresentationNativeShadow.renderedFrames = 0L
            controller.destroy()
            module.invalidate()
        }
    }

    @Test fun `listener removal defers a queued handoff until a fresh subscription`() {
        val context = TerminationReactContext(RuntimeEnvironment.getApplication()).apply { active = true }
        val module = StreamLauncherModule(context)
        own(17)
        try {
            module.addListener("leftcarStreamTerminated")
            StreamLauncherModule.emitTermination(port, 5)
            module.removeListeners(1)
            context.drain()
            assertTrue(context.events.isEmpty())
            module.addListener("leftcarStreamTerminated")
            context.drain()
            assertEquals(1, context.events.size)
        } finally {
            StreamLauncherModule.forgetStream(id, 17)
            module.invalidate()
        }
    }

    @Test fun `React replacement retains an undelivered request without emitting on the retired queue`() {
        val first = TerminationReactContext(RuntimeEnvironment.getApplication()).apply { active = true }
        val firstModule = StreamLauncherModule(first)
        own(17)
        firstModule.addListener("leftcarStreamTerminated")
        StreamLauncherModule.emitTermination(port, 5)
        firstModule.invalidate()
        val second = TerminationReactContext(RuntimeEnvironment.getApplication()).apply { active = true }
        val secondModule = StreamLauncherModule(second)
        try {
            secondModule.addListener("leftcarStreamTerminated")
            first.drain()
            second.drain()
            assertTrue(first.events.isEmpty())
            assertEquals(1, second.events.size)
            assertEquals("17", second.events.single().second.getString("generation"))
        } finally {
            StreamLauncherModule.forgetStream(id, 17)
            secondModule.invalidate()
        }
    }
}

@Suppress("DEPRECATION")
private class TerminationReactContext(context: Context) : BridgeReactContext(context) {
    var active = false
    val events = mutableListOf<Pair<String, ReadableMap>>()
    private val queue = ArrayDeque<Runnable>()
    override fun hasActiveReactInstance() = active
    override fun runOnNativeModulesQueueThread(runnable: Runnable) { queue.addLast(runnable) }
    override fun <T : JavaScriptModule> getJSModule(type: Class<T>): T = requireNotNull(type.cast(
        DeviceEventManagerModule.RCTDeviceEventEmitter { name, data -> events += name to (data as ReadableMap) },
    ))
    fun drain() { while (queue.isNotEmpty()) queue.removeFirst().run() }
}

@Implements(value = Arguments::class, isInAndroidSdk = false)
class TerminationArgumentsShadow {
    companion object {
        @JvmStatic @Implementation fun createMap(): WritableMap = JavaOnlyMap()
    }
}
