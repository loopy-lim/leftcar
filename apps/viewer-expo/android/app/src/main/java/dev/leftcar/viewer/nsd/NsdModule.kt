package dev.leftcar.viewer.nsd

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import com.facebook.react.modules.core.DeviceEventManagerModule
import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.bridge.WritableMap

/**
 * NSD discovery for `_leftcar._tcp.` hosts advertised by the Tauri host app.
 * Found hosts are emitted as `leftcar:host-found` events {name, host, port}.
 */
class NsdModule(reactContext: ReactApplicationContext) :
    ReactContextBaseJavaModule(reactContext) {

    override fun getName() = "NsdDiscovery"

    private var discoveryListener: NsdManager.DiscoveryListener? = null
    private var nsdManager: NsdManager? = null

    @ReactMethod
    fun startDiscovery() {
        if (discoveryListener != null) return
        val manager = reactApplicationContext
            .getSystemService(Context.NSD_SERVICE) as NsdManager
        nsdManager = manager

        val listener = object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(regType: String) {}
            override fun onDiscoveryStopped(serviceType: String) {}

            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                emit("discovery-failed", "start failed: $errorCode")
                discoveryListener = null
            }

            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {
                discoveryListener = null
            }

            override fun onServiceFound(service: NsdServiceInfo) {
                // resolve to get the host IP
                val resolveListener = object : NsdManager.ResolveListener {
                    override fun onResolveFailed(info: NsdServiceInfo, errorCode: Int) {}
                    override fun onServiceResolved(info: NsdServiceInfo) {
                        val host = info.host?.hostAddress ?: return
                        val map: WritableMap = Arguments.createMap()
                        map.putString("name", info.serviceName ?: service.serviceName)
                        map.putString("host", host)
                        map.putInt("port", if (info.port != 0) info.port else 7777)
                        emit("host-found", map)
                    }
                }
                try {
                    manager.resolveService(service, resolveListener)
                } catch (_: IllegalArgumentException) {
                    // another resolve in flight — next discovery pass retries
                }
            }

            override fun onServiceLost(service: NsdServiceInfo) {
                emit("host-lost", service.serviceName ?: "")
            }
        }
        discoveryListener = listener
        manager.discoverServices("_leftcar._tcp.", NsdManager.PROTOCOL_DNS_SD, listener)
    }

    @ReactMethod
    fun stopDiscovery() {
        val listener = discoveryListener ?: return
        discoveryListener = null
        try {
            nsdManager?.stopServiceDiscovery(listener)
        } catch (_: IllegalArgumentException) {
        }
    }

    private fun emit(event: String, data: Any) {
        // only WritableMap/String survive the RN bridge — plain Kotlin maps crash it
        val payload: Any = if (data is String) data else data
        reactApplicationContext
            .getJSModule(DeviceEventManagerModule.RCTDeviceEventEmitter::class.java)
            .emit("leftcar:$event", payload)
    }
}
