package dev.leftcar.viewer.stream

import android.content.Intent
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.bridge.Promise

/**
 * Opens one OS window (task) per stream: RN calls openStream(port, title)
 * and gets back the instanceId the Rust core uses for that window.
 */
class StreamLauncherModule(reactContext: ReactApplicationContext) :
    ReactContextBaseJavaModule(reactContext) {

    override fun getName() = "StreamLauncher"

    @ReactMethod
    fun openStream(port: Int, title: String, promise: Promise) {
        try {
            val instanceId = "src-$port"
            val intent = Intent(reactApplicationContext, StreamActivity::class.java).apply {
                putExtra("instance", instanceId)
                putExtra("port", port)
                putExtra("title", title)
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
