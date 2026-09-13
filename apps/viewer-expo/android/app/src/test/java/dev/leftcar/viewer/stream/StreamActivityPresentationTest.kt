package dev.leftcar.viewer.stream

import android.content.Intent
import android.os.Bundle
import android.view.Choreographer
import android.view.Surface
import android.view.SurfaceView
import org.robolectric.shadows.ShadowSurfaceView
import android.view.SurfaceHolder
import dev.leftcar.viewer.shim.ViewerNative
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.util.ReflectionHelpers
import org.robolectric.util.ReflectionHelpers.ClassParameter.from

/** Runs the actual Activity callers; only JNI and the frame scheduler are controlled. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE, shadows = [PresentationNativeShadow::class, ValidSurfaceShadow::class, PresentationSurfaceViewShadow::class])
class StreamActivityPresentationTest {
    private fun intent(balanced: Boolean) = Intent().putExtra("instance", "presentation-test")
        .putExtra("host", "192.168.0.2").putExtra("balancedPresentation", balanced)

    private fun controlIntent() = Intent().putExtra("instance", "presentation-test").putExtra("host", "192.168.0.2").putExtra("port", 5000)

    @Test fun `saved state recreation releases every actual native state with no launcher lease`() {
        PresentationNativeShadow.releases.clear()
        val original = Robolectric.buildActivity(StreamActivity::class.java, intent(false)).create()
        val saved = Bundle()
        original.saveInstanceState(saved)
        val firstState = ReflectionHelpers.getField<Long>(original.get(), "nativeState")
        original.destroy()
        val recreated = Robolectric.buildActivity(StreamActivity::class.java, intent(false)).create(saved)
        val secondState = ReflectionHelpers.getField<Long>(recreated.get(), "nativeState")
        recreated.destroy()
        assertNotEquals(firstState, secondState)
        val deadline = System.nanoTime() + 2_000_000_000L
        while (PresentationNativeShadow.releases.size < 2 && System.nanoTime() < deadline) Thread.sleep(1)
        assertEquals(listOf(firstState, secondState), PresentationNativeShadow.releases.toList())
    }

    @Test fun `actual registered Activity recreation owns distinct native states and transfers onNewIntent`() {
        val id = "actual-owner-recreation"
        val leases = ReflectionHelpers.getStaticField<MutableMap<String, StreamCloseLease>>(StreamLauncherModule::class.java, "closeLeases")
        val registry = ReflectionHelpers.getStaticField<StreamActivityRegistry<StreamActivity>>(StreamLauncherModule::class.java, "activityRegistry")
        val releases = mutableListOf<Long>()
        leases[id] = StreamCloseLease("17")
        PresentationNativeShadow.releases.clear()
        val controller = Robolectric.buildActivity(StreamActivity::class.java, intent(false).putExtra("instance", id).putExtra("ownershipGeneration", 17L)).create()
        val saved = Bundle()
        controller.saveInstanceState(saved)
        val oldState = ReflectionHelpers.getField<Long>(controller.get(), "nativeState")
        releases += oldState
        controller.destroy()
        assertEquals(releases, PresentationNativeShadow.releases.toList())
        val recreated = Robolectric.buildActivity(StreamActivity::class.java, intent(false)).create(saved)
        val activity = recreated.get()
        assertSame(activity, registry.current(id))
        assertFalse(activity.isFinishing)
        val newState = ReflectionHelpers.getField<Long>(activity, "nativeState")
        assertNotEquals(oldState, newState)
        leases[id] = StreamCloseLease("18")
        recreated.newIntent(controlIntent().putExtra("ownershipGeneration", 18L))
        assertEquals(18L, ReflectionHelpers.getField<Long>(activity, "ownershipGeneration"))
        assertEquals(newState, ReflectionHelpers.getField<Long>(activity, "nativeState"))
        val results = mutableListOf<Boolean>()
        registry.close(id, "18") { results += it }
        assertTrue(results.isEmpty())
        recreated.destroy()
        releases += newState
        assertEquals(releases, PresentationNativeShadow.releases.toList())
        assertEquals(listOf(true), results)
    }

    @Test fun `close before actual onNewIntent delivery still releases the transferred native owner`() {
        val id = "actual-pending-intent-close"
        val leases = ReflectionHelpers.getStaticField<MutableMap<String, StreamCloseLease>>(StreamLauncherModule::class.java, "closeLeases")
        val registry = ReflectionHelpers.getStaticField<StreamActivityRegistry<StreamActivity>>(StreamLauncherModule::class.java, "activityRegistry")
        leases[id] = StreamCloseLease("17")
        PresentationNativeShadow.releases.clear()
        val controller = Robolectric.buildActivity(StreamActivity::class.java, intent(false).putExtra("instance", id).putExtra("ownershipGeneration", 17L)).create()
        val state = ReflectionHelpers.getField<Long>(controller.get(), "nativeState")
        leases[id] = StreamCloseLease("18")
        val results = mutableListOf<Boolean>()
        registry.close(id, "18") { results += it }
        assertFalse(controller.get().isFinishing)
        controller.newIntent(controlIntent().putExtra("ownershipGeneration", 18L))
        assertEquals(18L, ReflectionHelpers.getField<Long>(controller.get(), "ownershipGeneration"))
        assertTrue(controller.get().isFinishing)
        assertTrue(results.isEmpty())
        controller.destroy()
        assertEquals(listOf(state), PresentationNativeShadow.releases.toList())
        assertEquals(listOf(true), results)
    }

    @Test fun `restoration after acknowledged close cannot allocate another native state`() {
        val id = "actual-closed-restoration"
        val leases = ReflectionHelpers.getStaticField<MutableMap<String, StreamCloseLease>>(StreamLauncherModule::class.java, "closeLeases")
        val registry = ReflectionHelpers.getStaticField<StreamActivityRegistry<StreamActivity>>(StreamLauncherModule::class.java, "activityRegistry")
        leases[id] = StreamCloseLease("17")
        val controller = Robolectric.buildActivity(StreamActivity::class.java, intent(false).putExtra("instance", id).putExtra("ownershipGeneration", 17L)).create()
        val saved = Bundle()
        controller.saveInstanceState(saved)
        val results = mutableListOf<Boolean>()
        registry.close(id, "17") { results += it }
        controller.destroy()
        assertEquals(listOf(true), results)
        val startsBefore = PresentationNativeShadow.nextState
        val recreated = Robolectric.buildActivity(StreamActivity::class.java, intent(false)).create(saved)
        assertTrue(recreated.get().isFinishing)
        assertEquals(startsBefore, PresentationNativeShadow.nextState)
        recreated.destroy()
    }

    @Test fun `partial controls preserve both effective modes across save and recreation`() {
        for (balanced in listOf(true, false)) {
            val controller = Robolectric.buildActivity(StreamActivity::class.java, intent(!balanced)).create()
            val activity = controller.get()
            controller.newIntent(controlIntent().putExtra("balancedPresentation", balanced))
            controller.newIntent(controlIntent().putExtra("localCursor", false))
            controller.newIntent(controlIntent().putExtra("localAudio", false))
            controller.newIntent(controlIntent().putExtra("xrWindowRatio", 1.5f))
            val saved = Bundle()
            controller.saveInstanceState(saved)
            assertTrue(saved.containsKey("balancedPresentation"))
            assertEquals(balanced, saved.getBoolean("balancedPresentation"))
            val partialIntent = activity.intent
            controller.destroy()
            val recreated = Robolectric.buildActivity(StreamActivity::class.java, partialIntent).create(saved)
            assertEquals(balanced, ReflectionHelpers.getField<Boolean>(recreated.get(), "balancedPresentation"))
            recreated.destroy()
        }
    }

    @Test fun `complete effective configuration survives partial toggles and recreation`() {
        val start = intent(true).putExtra("width", 3200).putExtra("height", 2000)
            .putExtra("fps", 90).putExtra("localAudio", false).putExtra("localCursor", false).putExtra("opusAudio", true)
            .putExtra("splitVertical", true).putExtra("splitDecoderName", "synthetic.decoder")
            .putExtra("displayName", "Synthetic screen").putExtra("showFps", true)
            .putExtra("ownershipGeneration", 17L)
        val controller = Robolectric.buildActivity(StreamActivity::class.java, start).create()
        controller.newIntent(controlIntent().putExtra("balancedPresentation", false))
        controller.newIntent(controlIntent().putExtra("xrWindowRatio", 1.5f))
        val saved = Bundle()
        controller.saveInstanceState(saved)
        val recreatedIntent = controller.get().intent
        controller.destroy()
        val recreated = Robolectric.buildActivity(StreamActivity::class.java, recreatedIntent).create(saved)
        val activity = recreated.get()
        assertFalse(ReflectionHelpers.getField<Boolean>(activity, "localAudioEnabled"))
        assertFalse(ReflectionHelpers.getField<Boolean>(activity, "localCursorEnabled"))
        assertFalse(ReflectionHelpers.getField<Boolean>(activity, "balancedPresentation"))
        assertTrue(ReflectionHelpers.getField<Boolean>(activity, "opusAudioRequested"))
        assertEquals("opus128k", activity.audioStats()["requestedCodec"])
        assertNull(activity.audioStats()["effectiveCodec"])
        assertEquals(3200, ReflectionHelpers.getField<Int>(activity, "sourceWidth"))
        assertEquals(2000, ReflectionHelpers.getField<Int>(activity, "sourceHeight"))
        assertEquals(90, ReflectionHelpers.getField<Int>(activity, "fps"))
        assertTrue(ReflectionHelpers.getField<Boolean>(activity, "splitVertical"))
        assertEquals("synthetic.decoder", ReflectionHelpers.getField<String>(activity, "splitDecoderName"))
        assertEquals(17L, ReflectionHelpers.getField<Long>(activity, "ownershipGeneration"))
        assertEquals("Synthetic screen", activity.title)
        recreated.destroy()
    }

    @Test fun `audio off reports retired native output through actual Activity snapshot`() {
        val controller = Robolectric.buildActivity(StreamActivity::class.java, intent(false).putExtra("localAudio", false)).create()
        val player = StreamAudioPlayer("synthetic", { bytes ->
            byteArrayOf(0xbb.toByte(), 0x80.toByte(), 2, 0, 0, 1, 1, 2, 3, 4).also { it.copyInto(bytes) }.size
        }, { _, _ -> object : AudioOutput {
            override val bufferFrames = 2880
            override val capacityFrames = 5760
            override val underruns = 0
            override val playbackFrames = 1L
            override fun write(bytes: ByteArray, offset: Int, length: Int) = length
            override fun resize(frames: Int) = frames
            override fun close() {}
        } })
        ReflectionHelpers.setField(controller.get(), "audioPlayer", player)
        controller.newIntent(controlIntent().putExtra("localAudio", true))
        val deadline = System.nanoTime() + 2_000_000_000L
        while (player.metrics.effectiveCodec == null && System.nanoTime() < deadline) Thread.sleep(1)
        assertEquals("pcm", controller.get().audioStats()["effectiveCodec"])
        controller.newIntent(controlIntent().putExtra("localAudio", false))
        val retired = controller.get().audioStats()
        assertEquals(false, retired["enabled"])
        for (key in listOf("effectiveCodec", "requestedBufferFrames", "actualBufferFrames", "capacityFrames", "underruns", "playbackFrames", "decodeNanoseconds")) assertNull(key, retired[key])
        assertTrue((retired["writtenFrames"] as Double) > 0)
        controller.destroy()
    }

    @Test fun `hierarchy replacement retires clock through delayed failed and successful attach`() {
        val controller = Robolectric.buildActivity(StreamActivity::class.java, intent(true)).create()
        val activity = controller.get()
        val posted = mutableListOf<Choreographer.FrameCallback>()
        val removed = mutableListOf<Choreographer.FrameCallback>()
        val frames = mutableListOf<Long>()
        val clock = DisplayFrameClock({ posted += it }, { removed += it }, { 4 to 90f }, { _, frame, _ -> frames += frame })
        ReflectionHelpers.setField(activity, "displayClock\$delegate", lazyOf(clock))
        val gate = ReflectionHelpers.getField<StreamSurfaceLifecycleGate<SurfaceHolder>>(activity, "surfaceLifecycle")
        val old = ReflectionHelpers.getField<StreamSurfaces>(activity, "streamSurfaces").holders.single()
        gate.confirmAttached(true)
        controller.start()
        val retiredCallback = posted.last()
        retiredCallback.doFrame(8_000_000_000L)
        PresentationNativeShadow.samples.clear()
        controller.newIntent(controlIntent().putExtra("splitVertical", true).putExtra("splitDecoderName", "c2.qti.avc.decoder"))
        assertTrue("hierarchy replacement must remove the previous callback", removed.contains(retiredCallback))
        assertEquals(listOf(Triple(-1, 0L, 0L)), PresentationNativeShadow.samples)
        val postsAfterSwap = posted.size
        retiredCallback.doFrame(8_010_000_000L)
        activity.surfaceDestroyed(old)
        assertEquals(postsAfterSwap, posted.size)
        assertEquals(listOf(8_000_000_000L), frames)
        assertFalse(gate.isAttached)
        val next = ReflectionHelpers.getField<StreamSurfaces>(activity, "streamSurfaces")
        next.holders.forEach(activity::surfaceCreated)
        PresentationNativeShadow.attachResult = -1
        ReflectionHelpers.callInstanceMethod<Void>(activity, "attachStableSurfaces", from(Int::class.javaPrimitiveType, gate.currentGeneration))
        assertEquals(postsAfterSwap, posted.size)
        assertFalse(gate.isAttached)
        PresentationNativeShadow.attachResult = 0
        ReflectionHelpers.callInstanceMethod<Void>(activity, "attachStableSurfaces", from(Int::class.javaPrimitiveType, gate.currentGeneration))
        assertTrue(gate.isAttached)
        assertEquals(postsAfterSwap + 1, posted.size)
        val currentCallback = posted.last()
        activity.surfaceDestroyed(old)
        retiredCallback.doFrame(8_020_000_000L)
        assertEquals(postsAfterSwap + 1, posted.size)
        currentCallback.doFrame(8_030_000_000L)
        assertEquals(listOf(8_000_000_000L, 8_030_000_000L), frames)
        controller.stop().destroy()
    }
}

@Implements(value = SurfaceView::class)
class PresentationSurfaceViewShadow : ShadowSurfaceView() {
    private val syntheticSurface = Surface(android.graphics.SurfaceTexture(0))
    private val holder = object : FakeSurfaceHolder() {
        override fun getSurface(): Surface = syntheticSurface
    }
    @Implementation override fun getHolder(): SurfaceHolder = holder
}

@Implements(value = Surface::class)
class ValidSurfaceShadow {
    @Implementation fun isValid(): Boolean = true
    @Implementation fun setFrameRate(rate: Float, compatibility: Int, strategy: Int) {}
}

@Implements(value = ViewerNative::class, isInAndroidSdk = false, callThroughByDefault = false)
class PresentationNativeShadow {
    companion object {
        var nextState = 0L
        val releases = java.util.concurrent.CopyOnWriteArrayList<Long>()
        var attachResult = 0
        val samples = mutableListOf<Triple<Int, Long, Long>>()
        @JvmStatic @Implementation fun __staticInitializer__() {
            ReflectionHelpers.setStaticField(ViewerNative::class.java, "INSTANCE", ReflectionHelpers.callConstructor(ViewerNative::class.java))
        }
    }
    @Implementation fun start(): Long = ++nextState
    @Implementation fun release(state: Long, instance: String): Int { releases += state; return 0 }
    @Implementation fun pollAudio(instance: String, bytes: ByteArray): Int = 0
    @Implementation fun displayFrame(state: Long, instance: String, balanced: Boolean, display: Int, frame: Long, period: Long): Int {
        samples += Triple(display, frame, period)
        return 0
    }
    @Implementation fun attachSplitSurfacesWithPresentation(state: Long, instance: String, left: Surface, right: Surface, port: Int, host: String, width: Int, height: Int, fps: Int, decoder: String, balanced: Boolean): Int = attachResult
}
