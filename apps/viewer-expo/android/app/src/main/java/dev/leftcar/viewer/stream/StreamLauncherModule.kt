package dev.leftcar.viewer.stream

import android.content.Intent
import android.net.Uri
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.bridge.Promise

/**
 * Opens one OS window (task) per unique stream: RN calls
 * openStream(port, host, width, height, fps) and gets back the instanceId the
 * Rust core uses for that window. Reopening the same host/port reuses its
 * existing document task instead of adding another entry to Recents.
 */
class StreamLauncherModule(reactContext: ReactApplicationContext) :
    ReactContextBaseJavaModule(reactContext) {

    override fun getName() = "StreamLauncher"

    @ReactMethod
    fun openStream(port: Int, host: String, width: Int, height: Int, fps: Int, promise: Promise) {
        try {
            val instanceId = "src-$port"
            val intent = Intent(reactApplicationContext, StreamActivity::class.java).apply {
                data = Uri.Builder()
                    .scheme("leftcar-stream")
                    .authority("session")
                    .appendPath(host)
                    .appendPath(port.toString())
                    .build()
                putExtra("instance", instanceId)
                putExtra("port", port)
                putExtra("host", host)
                putExtra("width", width)
                putExtra("height", height)
                putExtra("fps", fps.coerceIn(1, 90))
                addFlags(Intent.FLAG_ACTIVITY_NEW_DOCUMENT)
            }
            val ctx = getCurrentActivity()
            if (ctx != null) {
                ctx.startActivity(intent)
            } else {
                intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                reactApplicationContext.startActivity(intent)
            }
            promise.resolve(instanceId)
        } catch (t: Throwable) {
            promise.reject("ERR_STREAM_LAUNCH", t.message, t)
        }
    }
}
