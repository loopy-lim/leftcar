package dev.leftcar.viewer.stream

import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.bridge.Promise
import java.net.Inet4Address
import java.net.NetworkInterface

/**
 * Opens one OS window (task) per stream: RN calls openStream(port, width, height, fps)
 * and gets back the instanceId the Rust core uses for that window.
 */
class StreamLauncherModule(reactContext: ReactApplicationContext) :
    ReactContextBaseJavaModule(reactContext) {

    override fun getName() = "StreamLauncher"

    @ReactMethod
    fun openStream(port: Int, width: Int, height: Int, fps: Int, promise: Promise) {
        try {
            val instanceId = "src-$port"
            val intent = Intent(reactApplicationContext, StreamActivity::class.java).apply {
                putExtra("instance", instanceId)
                putExtra("port", port)
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

    @ReactMethod
    fun getLocalAddresses(promise: Promise) {
        try {
            val connectivity = reactApplicationContext
                .getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
            val linkAddresses = connectivity.allNetworks
                .flatMap { network ->
                    connectivity.getLinkProperties(network)?.linkAddresses.orEmpty()
                }
                .map { it.address }

            // LinkProperties sees both wlan0 and VPN interfaces on Android,
            // while NetworkInterface is kept as a fallback for older/vendor
            // builds. Prefer RFC1918 Wi-Fi addresses: the host must connect
            // back to the viewer, and a VPN address may route through NAT.
            val interfaceAddresses = NetworkInterface.getNetworkInterfaces().toList()
                .filter { it.isUp && !it.isLoopback && !it.isVirtual }
                .flatMap { it.inetAddresses.toList() }
            val addresses = (linkAddresses + interfaceAddresses)
                .filterIsInstance<Inet4Address>()
                .filter { !it.isLoopbackAddress && !it.isLinkLocalAddress }
                .mapNotNull { it.hostAddress }
                .distinct()
                .sortedWith(compareBy({ addressPriority(it) }, { it }))
            android.util.Log.i("StreamLauncher", "viewer IPv4 candidates=$addresses")
            promise.resolve(Arguments.fromList(addresses))
        } catch (t: Throwable) {
            promise.reject("ERR_LOCAL_ADDRESSES", t.message, t)
        }
    }

    private fun addressPriority(address: String): Int {
        val octets = address.split('.').mapNotNull { it.toIntOrNull() }
        if (octets.size == 4) {
            val a = octets[0]
            val b = octets[1]
            if (a == 10 || (a == 172 && b in 16..31) || (a == 192 && b == 168)) {
                return 0
            }
            if (a == 100 && b in 64..127) {
                return 2
            }
        }
        return 1
    }
}
