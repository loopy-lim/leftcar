package dev.leftcar.viewer.shim

import android.app.Activity
import android.os.Bundle
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.widget.TextView
import android.graphics.Color
import android.view.Gravity

/**
 * Stream window (docs/03 §3.2): one remote source per task.
 *
 * REAL implementation: hosts a SurfaceView whose Surface is handed to the
 * Rust core via JNI (leftcar_stream_attach_surface); a real hardware decoder
 * is configured per instance and renders the test pattern stream. The decode
 * loop runs entirely in Rust — Kotlin only forwards lifecycle + Surface.
 */
class StreamActivity : Activity(), SurfaceHolder.Callback {
    private var instanceId: String = ""
    private var nativeState: Long = 0
    // UDP 스트림 수신 중 라디오 절전이 프레임 유실의 주원인 — low-latency Wi-Fi lock 유지
    private var wifiLock: android.net.wifi.WifiManager.WifiLock? = null
    private var multicastLock: android.net.wifi.WifiManager.MulticastLock? = null

    private fun acquireNetworkLocks() {
        val wifi = applicationContext.getSystemService(WIFI_SERVICE) as android.net.wifi.WifiManager
        wifiLock = wifi.createWifiLock(
            if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.Q)
                android.net.wifi.WifiManager.WIFI_MODE_FULL_LOW_LATENCY
            else android.net.wifi.WifiManager.WIFI_MODE_FULL_HIGH_PERF,
            "leftcar-stream"
        ).apply { acquire() }
        multicastLock = wifi.createMulticastLock("leftcar-mcast").apply { acquire() }
    }

    private fun releaseNetworkLocks() {
        wifiLock?.takeIf { it.isHeld }?.release()
        multicastLock?.takeIf { it.isHeld }?.release()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        instanceId = intent?.data?.getQueryParameter("instance")
            ?: savedInstanceState?.getString("instance")
            ?: "instance-${System.nanoTime()}"
        val idx = intent?.data?.getQueryParameter("idx")?.toIntOrNull() ?: 0
        val colors = intArrayOf(0xFFE53935.toInt(), 0xFF43A047.toInt(), 0xFF1E88E5.toInt(), 0xFFFB8C00.toInt())
        val sv = SurfaceView(this)
        sv.holder.addCallback(this)
        sv.holder.setKeepScreenOn(true)
        val label = TextView(this).apply {
            text = "#$idx ${instanceId.takeLast(6)}"
            setTextColor(Color.WHITE)
            setBackgroundColor(colors[idx % colors.size])
            gravity = Gravity.CENTER
            textSize = 20f
        }
        val fl = android.widget.FrameLayout(this)
        fl.addView(sv, android.widget.FrameLayout.LayoutParams(
            android.widget.FrameLayout.LayoutParams.MATCH_PARENT,
            android.widget.FrameLayout.LayoutParams.MATCH_PARENT))
        fl.addView(label, android.widget.FrameLayout.LayoutParams(
            400, android.widget.FrameLayout.LayoutParams.WRAP_CONTENT, Gravity.TOP or Gravity.START))
        android.util.Log.i("LeftcarStream", "onCreate: instanceId=$instanceId idx=$idx")
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R) {
            window.attributes.preferredRefreshRate = 90.0f
        }
        setContentView(fl)
        acquireNetworkLocks()
        nativeState = ViewerNative.start()
        ViewerNative.updateWindowEvent(nativeState, instanceId, 1, 0) // ACTIVITY_CREATE
        ViewerNative.updateWindowEvent(nativeState, instanceId, 6, 0) // SURFACE_CREATE
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        android.util.Log.i("LeftcarStream", "surfaceCreated: instanceId=$instanceId surface=${holder.surface}")
        val res = ViewerNative.attachSurface(nativeState, instanceId, holder.surface)
        android.util.Log.i("LeftcarStream", "attachSurface returned $res")
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        android.util.Log.i("LeftcarStream", "surfaceChanged: width=$width height=$height")
        ViewerNative.surfaceChanged(nativeState, instanceId, width, height)
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        ViewerNative.detachSurface(nativeState, instanceId)
    }

    override fun onDestroy() {
        releaseNetworkLocks()
        ViewerNative.updateWindowEvent(nativeState, instanceId, 12, 0) // TASK_REMOVE
        ViewerNative.release(nativeState, instanceId)
        super.onDestroy()
    }
}
