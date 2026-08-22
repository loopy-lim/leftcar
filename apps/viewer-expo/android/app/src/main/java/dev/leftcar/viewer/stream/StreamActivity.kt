package dev.leftcar.viewer.stream

import android.app.Activity
import android.content.Intent
import android.content.res.Configuration
import android.graphics.PixelFormat
import android.os.Bundle
import android.os.SystemClock
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.Surface
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.View
import android.view.Gravity
import android.graphics.Color
import dev.leftcar.viewer.shim.ViewerNative

/**
 * Stream window (docs/03 §3.2): one remote source per OS window.
 *
 * Multi-instance: each unique host/port is a document task, while reopening
 * the same stream routes back into its existing task. The decode loop runs
 * entirely in Rust (libleftcar_viewer) — Kotlin only forwards lifecycle +
 * Surface, plus the per-window UDP port from intent extras.
 */
private class AspectRatioSurfaceView(context: android.content.Context) : SurfaceView(context) {
    private var videoWidth = 16
    private var videoHeight = 9

    fun setVideoSize(width: Int, height: Int) {
        if (width > 0 && height > 0) {
            videoWidth = width
            videoHeight = height
            requestLayout()
        }
    }

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val maxWidth = View.MeasureSpec.getSize(widthMeasureSpec)
        val maxHeight = View.MeasureSpec.getSize(heightMeasureSpec)
        if (maxWidth == 0 || maxHeight == 0) {
            super.onMeasure(widthMeasureSpec, heightMeasureSpec)
            return
        }
        val aspect = videoWidth.toDouble() / videoHeight.toDouble()
        var width = maxWidth
        var height = (width / aspect).toInt()
        if (height > maxHeight) {
            height = maxHeight
            width = (height * aspect).toInt()
        }
        setMeasuredDimension(width.coerceAtLeast(1), height.coerceAtLeast(1))
    }
}

class StreamActivity : Activity(), SurfaceHolder.Callback {
    private var instanceId: String = ""
    private var host: String = ""
    private var port: Int = 5000
    private var fps: Int = 60
    private var sourceWidth: Int = 1920
    private var sourceHeight: Int = 1080
    private var nativeState: Long = 0
    private var surfaceAttached = false
    private var released = false
    private var streamSurface: AspectRatioSurfaceView? = null
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

    private fun hideSystemBars() {
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R) {
            window.setDecorFitsSystemWindows(false)
            window.decorView.windowInsetsController?.let { controller ->
                controller.hide(WindowInsets.Type.statusBars() or WindowInsets.Type.navigationBars())
                controller.systemBarsBehavior =
                    WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            }
        } else {
            @Suppress("DEPRECATION")
            window.decorView.systemUiVisibility = (
                View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                    or View.SYSTEM_UI_FLAG_FULLSCREEN
                    or View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                    or View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    or View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                    or View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                )
        }
    }

    private fun lifecycleEvent(code: Int) {
        if (nativeState != 0L && !released) {
            ViewerNative.updateWindowEvent(
                nativeState,
                instanceId,
                code,
                SystemClock.elapsedRealtime(),
            )
        }
    }

    private fun normalizedX(event: MotionEvent, view: View): Float =
        (event.x / view.width.coerceAtLeast(1).toFloat()).coerceIn(0f, 1f)

    private fun normalizedY(event: MotionEvent, view: View): Float =
        (event.y / view.height.coerceAtLeast(1).toFloat()).coerceIn(0f, 1f)

    private fun forwardPointer(event: MotionEvent, view: View): Boolean {
        val touchLike = event.isFromSource(InputDevice.SOURCE_TOUCHSCREEN) ||
            event.isFromSource(InputDevice.SOURCE_STYLUS)
        // Android normally batches/resamples pointer motion around display
        // frames. A remote-control Surface needs the hardware samples early;
        // Rust still coalesces them to the bounded 2x-stream-FPS wire target.
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R &&
            (event.actionMasked == MotionEvent.ACTION_HOVER_ENTER ||
                event.actionMasked == MotionEvent.ACTION_BUTTON_PRESS)
        ) {
            view.requestUnbufferedDispatch(event.source)
        }
        val action = when (event.actionMasked) {
            MotionEvent.ACTION_HOVER_MOVE, MotionEvent.ACTION_MOVE -> 1
            MotionEvent.ACTION_BUTTON_PRESS -> 2
            MotionEvent.ACTION_BUTTON_RELEASE -> 3
            MotionEvent.ACTION_DOWN -> if (touchLike) 2 else return false
            MotionEvent.ACTION_UP -> if (touchLike) 3 else return false
            MotionEvent.ACTION_SCROLL -> 4
            MotionEvent.ACTION_CANCEL -> {
                ViewerNative.releaseInput(instanceId)
                return true
            }
            else -> return false
        }
        val actionButton = when {
            event.actionButton != 0 -> event.actionButton
            touchLike -> MotionEvent.BUTTON_PRIMARY
            else -> 0
        }
        val buttons = when {
            touchLike && event.actionMasked != MotionEvent.ACTION_UP -> MotionEvent.BUTTON_PRIMARY
            else -> event.buttonState
        }
        val result = ViewerNative.sendPointer(
            instanceId,
            action,
            normalizedX(event, view),
            normalizedY(event, view),
            buttons,
            actionButton,
            event.getAxisValue(MotionEvent.AXIS_HSCROLL),
            event.getAxisValue(MotionEvent.AXIS_VSCROLL),
        )
        return result == 0
    }

    private fun isRemoteKey(keyCode: Int): Boolean = keyCode !in setOf(
        KeyEvent.KEYCODE_HOME,
        KeyEvent.KEYCODE_BACK,
        KeyEvent.KEYCODE_POWER,
        KeyEvent.KEYCODE_VOLUME_UP,
        KeyEvent.KEYCODE_VOLUME_DOWN,
        KeyEvent.KEYCODE_VOLUME_MUTE,
        KeyEvent.KEYCODE_APP_SWITCH,
    )

    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        if (!isRemoteKey(event.keyCode)) return super.dispatchKeyEvent(event)
        if (event.action != KeyEvent.ACTION_DOWN && event.action != KeyEvent.ACTION_UP) {
            return super.dispatchKeyEvent(event)
        }
        val result = ViewerNative.sendKey(
            instanceId,
            event.keyCode,
            event.scanCode,
            event.metaState,
            event.action == KeyEvent.ACTION_DOWN,
            event.repeatCount,
        )
        return result == 0 || super.dispatchKeyEvent(event)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        instanceId = intent?.getStringExtra("instance")
            ?: savedInstanceState?.getString("instance")
            ?: "instance-${System.nanoTime()}"
        host = intent?.getStringExtra("host") ?: savedInstanceState?.getString("host") ?: ""
        port = intent?.getIntExtra("port", 5000) ?: 5000
        fps = (intent?.getIntExtra("fps", 60) ?: 60).coerceIn(1, 90)
        sourceWidth = intent?.getIntExtra("width", 1920) ?: 1920
        sourceHeight = intent?.getIntExtra("height", 1080) ?: 1080

        val sv = AspectRatioSurfaceView(this).apply {
            setVideoSize(sourceWidth, sourceHeight)
            setBackgroundColor(Color.BLACK)
            isFocusable = true
            isFocusableInTouchMode = true
            // This Activity has no UI overlay. Keep the decoder Surface above
            // the opaque window buffer; some XR/vendor compositors otherwise
            // leave the default z=-2 SurfaceView hidden behind the black root.
            setZOrderOnTop(true)
        }
        streamSurface = sv
        sv.setOnGenericMotionListener { view, event -> forwardPointer(event, view) }
        sv.setOnTouchListener { view, event ->
            if (event.actionMasked == MotionEvent.ACTION_DOWN) view.requestFocus()
            forwardPointer(event, view)
        }
        // Keep the decoding Surface tied to the actual window geometry.
        // A fixed source-sized buffer can be discarded by some freeform/XR
        // compositors when the task is resized, leaving the window black.
        sv.holder.setSizeFromLayout()
        sv.holder.setFormat(PixelFormat.OPAQUE)
        sv.holder.addCallback(this)
        sv.holder.setKeepScreenOn(true)
        val fl = android.widget.FrameLayout(this).apply {
            setBackgroundColor(Color.BLACK)
        }
        fl.addView(sv, android.widget.FrameLayout.LayoutParams(
            android.widget.FrameLayout.LayoutParams.WRAP_CONTENT,
            android.widget.FrameLayout.LayoutParams.WRAP_CONTENT,
            Gravity.CENTER))
        android.util.Log.i("LeftcarStream", "onCreate: instanceId=$instanceId port=$port host=$host")
        if (host.isEmpty()) {
            // No paired host = no stream. Fail loudly instead of rendering a
            // silently black window the user cannot diagnose.
            android.util.Log.e("LeftcarStream", "Missing host extra — refusing to attach stream")
            setResult(1, android.content.Intent().putExtra("error", "missing host"))
            finish()
            return
        }
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R) {
            window.attributes.preferredRefreshRate = fps.toFloat()
        }
        setContentView(fl)
        sv.requestFocus()
        hideSystemBars()
        acquireNetworkLocks()
        nativeState = ViewerNative.start()
        lifecycleEvent(1) // ACTIVITY_CREATE
    }

    override fun onNewIntent(newIntent: Intent) {
        super.onNewIntent(newIntent)
        val nextHost = newIntent.getStringExtra("host") ?: host
        val nextPort = newIntent.getIntExtra("port", port)
        val nextFps = newIntent.getIntExtra("fps", fps).coerceIn(1, 90)
        val nextWidth = newIntent.getIntExtra("width", sourceWidth)
        val nextHeight = newIntent.getIntExtra("height", sourceHeight)
        val streamConfigurationChanged =
            nextHost != host || nextPort != port || nextFps != fps ||
                nextWidth != sourceWidth || nextHeight != sourceHeight

        setIntent(newIntent)
        if (streamConfigurationChanged) {
            // Recreate inside the same document task so the native decoder and
            // Surface are rebuilt with the new stream configuration.
            recreate()
        } else {
            streamSurface?.requestFocus()
            window.decorView.post { hideSystemBars() }
        }
    }

    override fun onStart() {
        super.onStart()
        lifecycleEvent(2) // ACTIVITY_START
    }

    override fun onResume() {
        super.onResume()
        lifecycleEvent(3) // ACTIVITY_RESUME
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        lifecycleEvent(if (hasFocus) 4 else 5) // FOCUS_GAIN / FOCUS_LOSS
        if (hasFocus) {
            streamSurface?.requestFocus()
            window.decorView.post { hideSystemBars() }
        } else {
            ViewerNative.releaseInput(instanceId)
        }
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        android.util.Log.i("LeftcarStream", "surfaceCreated: instanceId=$instanceId port=$port")
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R) {
            holder.surface.setFrameRate(
                fps.toFloat(),
                Surface.FRAME_RATE_COMPATIBILITY_FIXED_SOURCE,
                Surface.CHANGE_FRAME_RATE_ALWAYS,
            )
        }
        val res = ViewerNative.attachSurfacePort(
            nativeState,
            instanceId,
            holder.surface,
            port,
            host,
            sourceWidth,
            sourceHeight,
            fps,
        )
        surfaceAttached = res == 0
        android.util.Log.i(
            "LeftcarStream",
            "attachSurfacePort returned $res, host=$host, source=${sourceWidth}x${sourceHeight}, fps=$fps",
        )
        lifecycleEvent(6) // SURFACE_CREATE
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        android.util.Log.i(
            "LeftcarStream",
            "surfaceChanged: ${width}x${height}, source=${sourceWidth}x${sourceHeight}",
        )
        if (surfaceAttached && width > 0 && height > 0) {
            ViewerNative.surfaceChanged(nativeState, instanceId, width, height)
        }
        lifecycleEvent(7) // SURFACE_CHANGE
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        android.util.Log.i("LeftcarStream", "surfaceDestroyed: transient detach instanceId=$instanceId")
        lifecycleEvent(8) // SURFACE_DESTROY
        ViewerNative.releaseInput(instanceId)
        val res = if (surfaceAttached) {
            surfaceAttached = false
            ViewerNative.detachSurface(nativeState, instanceId)
        } else {
            0
        }
        android.util.Log.i("LeftcarStream", "detachSurface returned $res; waiting for Surface recreation")
    }

    override fun onPause() {
        ViewerNative.releaseInput(instanceId)
        lifecycleEvent(9) // ACTIVITY_PAUSE
        super.onPause()
    }

    override fun onStop() {
        lifecycleEvent(10) // ACTIVITY_STOP
        super.onStop()
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        android.util.Log.i("LeftcarStream", "onConfigurationChanged: orientation=${newConfig.orientation}")
        lifecycleEvent(11) // CONFIGURATION_CHANGE
    }

    override fun onDestroy() {
        android.util.Log.i(
            "LeftcarStream",
            "onDestroy: final release instanceId=$instanceId attached=$surfaceAttached",
        )
        if (!released) {
            lifecycleEvent(12) // TASK_REMOVE / final Activity destruction
            released = true
        }
        releaseNetworkLocks()
        ViewerNative.releaseInput(instanceId)
        ViewerNative.release(nativeState, instanceId)
        super.onDestroy()
    }
}
