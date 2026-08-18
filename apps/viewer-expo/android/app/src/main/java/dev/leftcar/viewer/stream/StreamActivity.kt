package dev.leftcar.viewer.stream

import android.app.Activity
import android.os.Bundle
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.widget.TextView
import android.graphics.Color
import android.view.Gravity
import dev.leftcar.viewer.shim.ViewerNative

/**
 * Stream window (docs/03 §3.2): one remote source per OS window.
 *
 * Multi-instance: launched with documentLaunchMode="always" so each stream
 * becomes its own task/window (H05-proven manifest recipe). The decode loop
 * runs entirely in Rust (libleftcar_viewer) — Kotlin only forwards
 * lifecycle + Surface, plus the per-window TCP port from intent extras.
 */
class StreamActivity : Activity(), SurfaceHolder.Callback {
    private var instanceId: String = ""
    private var port: Int = 5000
    private var title: String = "stream"
    private var nativeState: Long = 0
    // 스트림 수신 중 라디오 절전이 프레임 유실의 주원인 — low-latency Wi-Fi lock 유지
    private var wifiLock: android.net.wifi.WifiManager.WifiLock? = null

    private fun acquireNetworkLocks() {
        try {
            val wifi = applicationContext.getSystemService(WIFI_SERVICE) as? android.net.wifi.WifiManager
            wifiLock = wifi?.createWifiLock(
                if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.Q)
                    android.net.wifi.WifiManager.WIFI_MODE_FULL_LOW_LATENCY
                else
                    android.net.wifi.WifiManager.WIFI_MODE_FULL_HIGH_PERF,
                "leftcar-stream-$port"
            )?.apply { acquire() }
        } catch (e: Throwable) {
            android.util.Log.w("LeftcarStream", "Failed to acquire wifiLock", e)
        }
    }

    private fun releaseNetworkLocks() {
        wifiLock?.takeIf { it.isHeld }?.release()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        instanceId = intent?.getStringExtra("instance")
            ?: savedInstanceState?.getString("instance")
            ?: "instance-${System.nanoTime()}"
        port = intent?.getIntExtra("port", 5000) ?: 5000
        title = intent?.getStringExtra("title") ?: "src-$port"

        val sv = SurfaceView(this)
        sv.holder.addCallback(this)
        sv.holder.setKeepScreenOn(true)
        val label = TextView(this).apply {
            text = title
            setTextColor(Color.WHITE)
            setBackgroundColor(0x88000000.toInt())
            gravity = Gravity.CENTER
            textSize = 14f
            setPadding(16, 8, 16, 8)
        }
        val fl = android.widget.FrameLayout(this)
        fl.addView(sv, android.widget.FrameLayout.LayoutParams(
            android.widget.FrameLayout.LayoutParams.MATCH_PARENT,
            android.widget.FrameLayout.LayoutParams.MATCH_PARENT))
        fl.addView(label, android.widget.FrameLayout.LayoutParams(
            android.widget.FrameLayout.LayoutParams.WRAP_CONTENT,
            android.widget.FrameLayout.LayoutParams.WRAP_CONTENT, Gravity.TOP or Gravity.START))
        android.util.Log.i("LeftcarStream", "onCreate: instanceId=$instanceId port=$port title=$title")
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R) {
            window.attributes.preferredRefreshRate = 90.0f
        }
        setContentView(fl)
        acquireNetworkLocks()
        nativeState = ViewerNative.start()
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        android.util.Log.i("LeftcarStream", "surfaceCreated: instanceId=$instanceId port=$port")
        val res = ViewerNative.attachSurfacePort(nativeState, instanceId, holder.surface, port)
        android.util.Log.i("LeftcarStream", "attachSurfacePort returned $res")
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        ViewerNative.surfaceChanged(nativeState, instanceId, width, height)
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        ViewerNative.detachSurface(nativeState, instanceId)
    }

    override fun onDestroy() {
        releaseNetworkLocks()
        ViewerNative.release(nativeState, instanceId)
        super.onDestroy()
    }
}
