package dev.leftcar.viewer.stream

import android.content.Intent
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.bridge.Promise

/**
 * Opens one OS window (task) per stream: RN calls openStream(port, host, width, height, fps)
 * and gets back the instanceId the Rust core uses for that window.
 */
class StreamLauncherModule(reactContext: ReactApplicationContext) :
    ReactContextBaseJavaModule(reactContext) {

    override fun getName() = "StreamLauncher"

    @ReactMethod
    fun openStream(port: Int, host: String, width: Int, height: Int, fps: Int, promise: Promise) {
        try {
            val instanceId = "src-$port"
            val intent = Intent(reactApplicationContext, StreamActivity::class.java).apply {
                putExtra("instance", instanceId)
                putExtra("port", port)
                putExtra("host", host)
                putExtra("width", width)
                putExtra("height", height)
                putExtra("fps", fps.coerceIn(1, 60))
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            val ctx = getCurrentActivity()
            if (ctx != null) {
                ctx.startActivity(intent)
            } else {
                reactApplicationContext.startActivity(intent)
            }
            promise.resolve(instanceId)
        } catch (t: Throwable) {
            promise.reject("ERR_STREAM_LAUNCH", t.message, t)
        }
    }
}
