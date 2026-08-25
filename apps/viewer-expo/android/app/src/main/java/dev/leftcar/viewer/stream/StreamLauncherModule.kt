package dev.leftcar.viewer.stream

import android.content.Intent
import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.Uri
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.Arguments
import dev.leftcar.viewer.shim.ViewerNative

/**
 * Opens one OS window (task) per unique stream: RN calls
 * prepareStream binds the media listener before Host reachability proof;
 * openStream then displays the already-authorized stream. Reopening the same
 * host/port reuses its existing document task instead of adding another entry
 * to Recents.
 */
class StreamLauncherModule(reactContext: ReactApplicationContext) :
    ReactContextBaseJavaModule(reactContext) {

    override fun getName() = "StreamLauncher"

    @ReactMethod
    fun getLocalIpv4Addresses(promise: Promise) {
        try {
            val result = Arguments.createArray()
            val seen = linkedSetOf<String>()
            val manager = reactApplicationContext.getSystemService(Context.CONNECTIVITY_SERVICE)
                as ConnectivityManager
            for (network in manager.allNetworks) {
                val capabilities = manager.getNetworkCapabilities(network) ?: continue
                val isPhysicalLan = capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) ||
                    capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)
                if (!isPhysicalLan) continue
                val properties = manager.getLinkProperties(network) ?: continue
                for (linkAddress in properties.linkAddresses) {
                    val address = linkAddress.address
                    if (address.address.size == 4 && address.isSiteLocalAddress) {
                        seen += address.hostAddress ?: continue
                    }
                }
            }
            seen.take(4).forEach(result::pushString)
            promise.resolve(result)
        } catch (t: Throwable) {
            promise.reject("ERR_LOCAL_ADDRESSES", t.message, t)
        }
    }

    @ReactMethod
    fun prepareStream(port: Int, host: String, mediaTransport: String, promise: Promise) {
        val result = ViewerNative.prepareStream(port, host, mediaTransport)
        if (result == 0) {
            promise.resolve(null)
        } else {
            promise.reject(
                "ERR_STREAM_PREPARE",
                "미디어 수신 포트를 준비하지 못했습니다. (code=$result)",
            )
        }
    }

    @ReactMethod
    fun cancelPreparedStream(port: Int, promise: Promise) {
        val result = ViewerNative.cancelPreparedStream(port)
        if (result == 0) {
            promise.resolve(null)
        } else {
            promise.reject(
                "ERR_STREAM_PREPARE_CANCEL",
                "미디어 수신 포트 정리에 실패했습니다. (code=$result)",
            )
        }
    }

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
                // A recovery reuses the existing document task and port, but
                // the old renderer was reclaimed before Host start. Force
                // that task to recreate its Surface/decoder on re-entry.
                putExtra("reconnect", true)
                addFlags(Intent.FLAG_ACTIVITY_NEW_DOCUMENT)
            }
            val ctx = getReactApplicationContext().getCurrentActivity()
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
