package dev.leftcar.viewer.stream

import android.app.Activity
import android.content.Intent
import android.content.res.Configuration
import android.os.Handler
import android.os.Bundle
import android.os.Looper
import android.os.SystemClock
import android.view.SurfaceHolder
import android.view.Surface
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.PointerIcon
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.View
import dev.leftcar.viewer.shim.ViewerNative

class StreamActivity : Activity(), SurfaceHolder.Callback {
    companion object {
        private const val TABLET_CURSOR_IDLE_TIMEOUT_MS = 1_500L
        private const val SURFACE_ATTACH_DEBOUNCE_MS = 300L
    }

    private var instanceId: String = ""
    private var host: String = ""
    private var port: Int = 5000
    private var fps: Int = 60
    private var showFps: Boolean = true
    private var sourceWidth: Int = 1920
    private var sourceHeight: Int = 1080
    private var splitVertical = false
    private var splitDecoderName = ""
    private var nativeState: Long = 0
    private var surfaceAttached = false
    private var released = false
    private var streamSurfaces: StreamSurfaces? = null
    private val createdSurfaceHolders = mutableSetOf<SurfaceHolder>()
    private val surfaceHandler = Handler(Looper.getMainLooper())
    private val recoveryHandler = Handler(Looper.getMainLooper())
    private val recoveryRetryPolicy = StreamRecoveryRetryPolicy()
    private var surfaceGeneration = 0
    private var surfaceChangeCount = 0
    private var pendingSurfaceAttach: Runnable? = null
    private val tabletCursorHandler = Handler(Looper.getMainLooper())
    private val hideTabletCursorRunnable = Runnable {
        streamSurfaces?.left?.pointerIcon = PointerIcon.getSystemIcon(this, PointerIcon.TYPE_NULL)
        streamSurfaces?.right?.pointerIcon = PointerIcon.getSystemIcon(this, PointerIcon.TYPE_NULL)
    }
    private var hud: StreamHudController? = null
    private var terminationHandled = false
    private var recoveryRetryRunnable: Runnable? = null
    private var recoveryFallbackEmitted = false

    /**
     * Both Host notices and local renderer watchdogs close the stale Surface.
     * Only local reasons notify React so it can reconnect with the original port.
     */
    private fun handleTermination(reason: Int) {
        if (terminationHandled) return
        terminationHandled = true
        val message = when (reason) {
            1 -> "컴퓨터와의 연결이 끊어져 화면 공유를 종료했습니다."
            2 -> "컴퓨터에서 이 화면 공유를 종료했습니다."
            3 -> "컴퓨터에서 화면 공유를 종료했습니다."
            4 -> "화면 렌더러를 다시 연결하고 있습니다."
            5 -> "화면 공유를 다시 연결하고 있습니다."
            else -> "화면 공유를 다시 연결하고 있습니다."
        }
        if (reason == 4) {
            StreamLauncherModule.emitTermination(port, reason)
        }
        android.util.Log.i("LeftcarStream", "stream termination reason=$reason: $message")
        if (isSameWindowRecoveryReason(reason)) {
            if (reason == 5) {
                // Keep the visible Activity and Surface alive. First retry the
                // renderer directly on the same port; only exhaust the short
                // native budget before asking React/Host to recreate the session.
                hud?.showRebindIndicator("화면을 같은 창에서 다시 연결하는 중")
                scheduleRenderRecovery()
            } else {
                // A complete Wi-Fi outage tears down the Host session, so the
                // React controller owns the reconnect. Retain this Activity so
                // its existing window can receive the next stream intent.
                recoveryRetryRunnable?.let(recoveryHandler::removeCallbacks)
                recoveryRetryRunnable = null
                recoveryRetryPolicy.reset()
                recoveryFallbackEmitted = false
                hud?.showRebindIndicator("컴퓨터 연결을 같은 창에서 다시 연결하는 중")
            }
            return
        }
        setResult(2, android.content.Intent().putExtra("terminationReason", reason))
        finish()
        android.widget.Toast.makeText(applicationContext, message, android.widget.Toast.LENGTH_LONG)
            .show()
    }

    private fun scheduleRenderRecovery() {
        if (released || isFinishing || isDestroyed) return
        val attempt = recoveryRetryPolicy.nextAttempt()
        if (attempt == null) {
            if (!recoveryFallbackEmitted) {
                recoveryFallbackEmitted = true
                android.util.Log.w(
                    "LeftcarStream",
                    "local render recovery exhausted; requesting React/Host retry " +
                        "instanceId=$instanceId port=$port",
                )
                StreamLauncherModule.emitTermination(port, 5)
            }
            return
        }
        recoveryRetryRunnable?.let(recoveryHandler::removeCallbacks)
        val retry = Runnable {
            recoveryRetryRunnable = null
            attemptRenderRecovery(attempt)
        }
        recoveryRetryRunnable = retry
        recoveryHandler.postDelayed(retry, attempt.delayMs)
    }

    private fun attemptRenderRecovery(attempt: RebindRetryAttempt) {
        if (released || isFinishing || isDestroyed) return
        val surface = streamSurfaces?.left?.holder?.surface
        if (nativeState == 0L || surface == null || !surface.isValid) {
            android.util.Log.w(
                "LeftcarStream",
                "local render recovery attempt=${attempt.number} skipped: Surface unavailable",
            )
            scheduleRenderRecovery()
            return
        }
        val result = ViewerNative.rebindSurfacePort(
            nativeState,
            instanceId,
            surface,
            port,
            host,
            sourceWidth,
            sourceHeight,
            fps,
        )
        surfaceAttached = result == 0
        android.util.Log.i(
            "LeftcarStream",
            "local render recovery attempt=${attempt.number} result=$result " +
                "port=$port source=${sourceWidth}x$sourceHeight fps=$fps",
        )
        if (result == 0) {
            terminationHandled = false
            recoveryFallbackEmitted = false
            hud?.onRebindFinished(true)
        } else {
            hud?.onRebindFinished(false)
            scheduleRenderRecovery()
        }
    }

    private fun markRenderHealthy() {
        recoveryRetryRunnable?.let(recoveryHandler::removeCallbacks)
        recoveryRetryRunnable = null
        recoveryRetryPolicy.reset()
        recoveryFallbackEmitted = false
    }

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
        streamSurfaces?.normalizedX(event.x, view)
            ?: (event.x / view.width.coerceAtLeast(1).toFloat()).coerceIn(0f, 1f)

    private fun normalizedY(event: MotionEvent, view: View): Float =
        (event.y / view.height.coerceAtLeast(1).toFloat()).coerceIn(0f, 1f)

    private fun hideTabletCursor() {
        tabletCursorHandler.removeCallbacks(hideTabletCursorRunnable)
        hideTabletCursorRunnable.run()
    }

    private fun updateTabletCursor(event: MotionEvent, view: View) {
        if (!event.isFromSource(InputDevice.SOURCE_MOUSE)) {
            if (event.isFromSource(InputDevice.SOURCE_TOUCHSCREEN) ||
                event.isFromSource(InputDevice.SOURCE_STYLUS)
            ) {
                hideTabletCursor()
            }
            return
        }
        if (event.actionMasked == MotionEvent.ACTION_HOVER_EXIT ||
            event.actionMasked == MotionEvent.ACTION_CANCEL
        ) {
            hideTabletCursor()
            return
        }
        view.pointerIcon = PointerIcon.getSystemIcon(this, PointerIcon.TYPE_ARROW)
        tabletCursorHandler.removeCallbacks(hideTabletCursorRunnable)
        tabletCursorHandler.postDelayed(
            hideTabletCursorRunnable,
            TABLET_CURSOR_IDLE_TIMEOUT_MS,
        )
    }

    private fun forwardPointer(event: MotionEvent, view: View): Boolean {
        val touchLike = event.isFromSource(InputDevice.SOURCE_TOUCHSCREEN) ||
            event.isFromSource(InputDevice.SOURCE_STYLUS)
        updateTabletCursor(event, view)
        if (event.actionMasked == MotionEvent.ACTION_DOWN ||
            event.actionMasked == MotionEvent.ACTION_BUTTON_PRESS
        ) {
            hud?.revealInput()
            hud?.revealStats()
        }
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
        if (event.action == KeyEvent.ACTION_DOWN) {
            hud?.revealInput()
            hud?.revealStats()
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

        splitVertical = intent?.getBooleanExtra("splitVertical", false) ?: false
        splitDecoderName = intent?.getStringExtra("splitDecoderName") ?: ""
        val surfaces = createStreamSurfaces(
            this,
            sourceWidth,
            sourceHeight,
            splitVertical,
            this,
            { view, event -> forwardPointer(event, view) },
        )
        streamSurfaces = surfaces
        android.util.Log.i("LeftcarStream", "onCreate: instanceId=$instanceId port=$port host=$host")
        if (host.isEmpty() || (splitVertical && splitDecoderName.isEmpty())) {
            // No paired host = no stream. Fail loudly instead of rendering a
            // silently black window the user cannot diagnose.
            val error = if (host.isEmpty()) "missing host" else "missing split decoder"
            android.util.Log.e("LeftcarStream", "$error — refusing to attach stream")
            setResult(1, android.content.Intent().putExtra("error", error))
            finish()
            return
        }
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R) {
            window.attributes.preferredRefreshRate = fps.toFloat()
        }
        setContentView(surfaces.root)
        val displayName = intent?.getStringExtra("displayName")?.takeIf { it.isNotBlank() } ?: "디스플레이"
        title = displayName
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.LOLLIPOP) {
            setTaskDescription(android.app.ActivityManager.TaskDescription(displayName))
        }
        showFps = intent?.getBooleanExtra("showFps", true) ?: true
        hud = StreamHudController(
            this,
            instanceId,
            fps,
            showFps,
            ::handleTermination,
            ::markRenderHealthy,
        )
        hud?.show()
        surfaces.requestFocus()
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
        val nextShowFps = newIntent.getBooleanExtra("showFps", showFps)
        val nextWidth = newIntent.getIntExtra("width", sourceWidth)
        val nextHeight = newIntent.getIntExtra("height", sourceHeight)
        val nextSplitVertical = newIntent.getBooleanExtra("splitVertical", splitVertical)
        val reconnectRequested = newIntent.getBooleanExtra("reconnect", false)
        val streamConfigurationChanged =
            nextHost != host || nextPort != port || nextFps != fps ||
                nextWidth != sourceWidth || nextHeight != sourceHeight ||
                nextSplitVertical != splitVertical || nextShowFps != showFps

        setIntent(newIntent)
        if (streamConfigurationChanged || reconnectRequested) {
            host = nextHost
            port = nextPort
            fps = nextFps
            showFps = nextShowFps
            sourceWidth = nextWidth
            sourceHeight = nextHeight
            splitVertical = nextSplitVertical
            splitDecoderName = newIntent.getStringExtra("splitDecoderName") ?: splitDecoderName
            if (!splitVertical && streamSurfaces?.left?.holder?.surface?.isValid == true) {
                val surface = streamSurfaces?.left?.holder?.surface
                val result = if (surface != null && nativeState != 0L && !released) {
                    ViewerNative.rebindSurfacePort(
                        nativeState,
                        instanceId,
                        surface,
                        port,
                        host,
                        sourceWidth,
                        sourceHeight,
                        fps,
                    )
                } else {
                    -1
                }
                surfaceAttached = result == 0
                hud?.onRebindFinished(result == 0)
                if (result == 0) {
                    terminationHandled = false
                    recoveryRetryPolicy.reset()
                    recoveryFallbackEmitted = false
                }
                if (result != 0) {
                    // Leave the Activity and Surface visible. The controller's
                    // bounded retry can deliver another intent to this same
                    // instance without opening a second window.
                    android.util.Log.w(
                        "LeftcarStream",
                        "same-window rebind failed result=$result; retaining Activity",
                    )
                }
            } else {
                surfaceGeneration += 1
                cancelPendingSurfaceAttach()
                hud?.showRebindIndicator("화면을 다시 연결할 준비 중")
            }
        } else {
            val nextDisplayName = newIntent.getStringExtra("displayName")?.takeIf { it.isNotBlank() }
            if (nextDisplayName != null) {
                title = nextDisplayName
                if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.LOLLIPOP) {
                    setTaskDescription(android.app.ActivityManager.TaskDescription(nextDisplayName))
                }
            }
            streamSurfaces?.requestFocus()
            window.decorView.post { hideSystemBars() }
        }
    }

    override fun onStart() {
        super.onStart()
        lifecycleEvent(2) // ACTIVITY_START
    }

    override fun onResume() {
        super.onResume()
        hud?.show()
        lifecycleEvent(3) // ACTIVITY_RESUME
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        lifecycleEvent(if (hasFocus) 4 else 5) // FOCUS_GAIN / FOCUS_LOSS
        if (hasFocus) {
            streamSurfaces?.requestFocus()
            window.decorView.post { hideSystemBars() }
        } else {
            hideTabletCursor()
            ViewerNative.releaseInput(instanceId)
        }
    }

    private fun cancelPendingSurfaceAttach() {
        pendingSurfaceAttach?.let(surfaceHandler::removeCallbacks)
        pendingSurfaceAttach = null
    }

    private fun attachStableSurfaces(generation: Int) {
        pendingSurfaceAttach = null
        val surfaces = streamSurfaces ?: return
        if (
            released || isFinishing || isDestroyed || surfaceAttached ||
            generation != surfaceGeneration || !surfaces.allValid() ||
            createdSurfaceHolders.size != surfaces.holders.size
        ) {
            return
        }
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R) {
            surfaces.holders.forEach { holder ->
                holder.surface.setFrameRate(
                    fps.toFloat(),
                    Surface.FRAME_RATE_COMPATIBILITY_FIXED_SOURCE,
                    Surface.CHANGE_FRAME_RATE_ALWAYS,
                )
            }
        }
        val res = if (splitVertical) {
            val right = surfaces.right ?: return
            ViewerNative.attachSplitSurfaces(
                nativeState,
                instanceId,
                surfaces.left.holder.surface,
                right.holder.surface,
                port,
                host,
                sourceWidth,
                sourceHeight,
                fps,
                splitDecoderName,
            )
        } else {
            ViewerNative.attachSurfacePort(
                nativeState,
                instanceId,
                surfaces.left.holder.surface,
                port,
                host,
                sourceWidth,
                sourceHeight,
                fps,
            )
        }
        surfaceAttached = res == 0
        if (surfaceAttached) {
            // Native attach clears a retained reason for this logical instance
            // before creating the new renderer. Only then may the HUD consume
            // a fresh termination reason.
            hud?.resetTerminationPolling()
            hud?.armTerminationPolling()
            hud?.clearRebindIndicator()
        }
        android.util.Log.i(
            "LeftcarStream",
            "stable Surface attach returned $res after $surfaceChangeCount geometry changes, " +
                "host=$host, source=${sourceWidth}x${sourceHeight}, fps=$fps, split=$splitVertical",
        )
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        createdSurfaceHolders += holder
        surfaceGeneration += 1
        surfaceChangeCount = 0
        val generation = surfaceGeneration
        cancelPendingSurfaceAttach()
        android.util.Log.i(
            "LeftcarStream",
            "surfaceCreated: debounce generation=$generation instanceId=$instanceId port=$port",
        )
        val surfaceCount = streamSurfaces?.holders?.size ?: 1
        if (createdSurfaceHolders.size == surfaceCount) {
            val attach = Runnable { attachStableSurfaces(generation) }
            pendingSurfaceAttach = attach
            surfaceHandler.postDelayed(attach, SURFACE_ATTACH_DEBOUNCE_MS)
        }
        lifecycleEvent(6) // SURFACE_CREATE
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        surfaceChangeCount += 1
        if (surfaceAttached && width > 0 && height > 0) {
            val surfaces = streamSurfaces
            val fullWidth = if (surfaces?.right != null) {
                surfaces.left.width + surfaces.right.width
            } else {
                width
            }
            val fullHeight = if (surfaces?.right != null) {
                maxOf(surfaces.left.height, surfaces.right.height)
            } else {
                height
            }
            ViewerNative.surfaceChanged(nativeState, instanceId, fullWidth, fullHeight)
        }
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        createdSurfaceHolders -= holder
        surfaceGeneration += 1
        cancelPendingSurfaceAttach()
        android.util.Log.i(
            "LeftcarStream",
            "surfaceDestroyed: cancel pending attach generation=$surfaceGeneration " +
                "geometryChanges=$surfaceChangeCount instanceId=$instanceId",
        )
        lifecycleEvent(8) // SURFACE_DESTROY
        hideTabletCursor()
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
        hideTabletCursor()
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
        surfaceGeneration += 1
        cancelPendingSurfaceAttach()
        recoveryRetryRunnable?.let(recoveryHandler::removeCallbacks)
        recoveryRetryRunnable = null
        tabletCursorHandler.removeCallbacks(hideTabletCursorRunnable)
        hud?.stop()
        hud = null
        releaseNetworkLocks()
        ViewerNative.releaseInput(instanceId)
        ViewerNative.release(nativeState, instanceId)
        super.onDestroy()
    }
}
