package dev.leftcar.viewer.stream

import android.content.Intent
import com.facebook.react.bridge.BridgeReactContext
import com.facebook.react.bridge.Promise
import java.lang.reflect.Proxy
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.util.ReflectionHelpers

/** Exercises the actual React methods with Android activity launch failure. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE)
class StreamLauncherSettingsFailureTest {
    @Test fun `all live setting launch failures reject the JS promise for local retry`() {
        val context = object : BridgeReactContext(RuntimeEnvironment.getApplication()) {
            override fun startActivity(intent: Intent) {
                throw SecurityException("Device setting update failed")
            }
        }
        val module = StreamLauncherModule(context)
        val targets = ReflectionHelpers.getStaticField<MutableMap<String, Any>>(StreamLauncherModule::class.java, "liveStreams")
        val ownershipClass = StreamLauncherModule::class.java.declaredClasses.first { it.simpleName == "StreamOwnership" }
        val constructor = ownershipClass.declaredConstructors.first().apply { isAccessible = true }
        val id = "native-setting-failure-test"
        targets[id] = constructor.newInstance("192.168.0.2", 5001, 41L)
        val errors = mutableListOf<Pair<String, String>>()
        var resolved = 0
        val promise = Proxy.newProxyInstance(Promise::class.java.classLoader, arrayOf(Promise::class.java)) { _, method, arguments ->
            when (method.name) {
                "reject" -> errors += Pair(arguments!![0] as String, arguments[1] as String)
                "resolve" -> resolved += 1
            }
            null
        } as Promise
        try {
            module.setCursorStream(id, false, promise)
            module.setAudioStream(id, false, promise)
            module.setOpusAudio(id, false, promise)
            module.setBalancedPresentation(id, false, promise)
            module.setPresentationSmooth(id, false, promise)
            assertEquals(0, resolved)
            assertEquals(listOf(
                "ERR_CURSOR_TOGGLE", "ERR_AUDIO_TOGGLE", "ERR_AUDIO_CODEC",
                "ERR_PRESENTATION_TOGGLE", "ERR_SMOOTH_TOGGLE",
            ), errors.map { it.first })
            assertEquals(List(5) { "Device setting update failed" }, errors.map { it.second })
        } finally {
            targets.remove(id)
            module.invalidate()
        }
    }
}
