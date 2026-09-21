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
import android.view.ViewConfiguration
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.View
import dev.leftcar.viewer.shim.ViewerNative
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import androidx.lifecycle.lifecycleScope
import androidx.xr.runtime.Session
import androidx.xr.runtime.SessionCreateSuccess
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.Job

class StreamActivity : ComponentActivity(), SurfaceHolder.Callback {
    companion object {
        private const val SURFACE_ATTACH_DEBOUNCE_MS = 300L
        // 제어·미디어 끊김 복구 중 React 복원을 다시 깨우는 주기. RN 타이머는
        // StreamActivity 포그라운드에서 멈추므로 네이티브가 시계 역할을 한다.
        private const val TERMINATION_REEMIT_INTERVAL_MS = 10_000L
        private const val SYSTEM_BARS_REVEAL_THROTTLE_MS = 3_000L
        private const val SYSTEM_BARS_DWELL_MS = 600L
        private const val KEY_BALANCED_PRESENTATION = "balancedPresentation"
        private const val KEY_PRESENTATION_SMOOTH = "presentationSmooth"
        // A healthy rebind renders its first frame well inside a second; this
        // budget only fails rebinds whose media never arrived at all.
        private const val RECOVERY_FLOW_WATCHDOG_MS = 4_000L
        // Surface attach usually receives video in tens of milliseconds. If no
        // frames arrive after 2.5s, trigger recovery to avoid a permanent black screen.
        private const val ATTACH_FLOW_WATCHDOG_MS = 2_500L
    }

    // Read by the display-clock thread inside displayClock's deliver lambda.
    @Volatile private var instanceId: String = ""
    private var host: String = ""
    private var port: Int = 5000
    private var fps: Int = 60
    private var showFps: Boolean = false
    private var sourceWidth: Int = 1920
    private var sourceHeight: Int = 1080
    private var splitVertical = false
    private var splitDecoderName = ""
    // Split detach consumes both prepared receivers. A replacement Surface
    // must wait for React/Host to prepare fresh receivers and session keys.
    private var splitNeedsPreparation = false
    // Read by the display-clock thread inside displayClock's deliver lambda.
    @Volatile private var nativeState: Long = 0
    private var released = false
    private var streamSurfaces: StreamSurfaces? = null
    private val surfaceLifecycle = StreamSurfaceLifecycleGate<SurfaceHolder>()
    private val surfaceHandler = Handler(Looper.getMainLooper())
    private val recoveryHandler = Handler(Looper.getMainLooper())
    private val recoveryRetryPolicy = StreamRecoveryRetryPolicy()
    private var reemitTerminationRunnable: Runnable? = null
    private var surfaceChangeCount = 0
    private var pendingSurfaceAttach: Runnable? = null
    private val tabletCursorHandler = Handler(Looper.getMainLooper())
    private var lastSystemBarsRevealAt = 0L
    private var lastPointerInBottomZone = false
    private var systemBarsInteractive = false
    private var bottomZoneDwellRunnable: Runnable? = null
    private val hideTabletCursorRunnable = Runnable {
        streamSurfaces?.left?.pointerIcon = PointerIcon.getSystemIcon(this, PointerIcon.TYPE_NULL)
        streamSurfaces?.right?.pointerIcon = PointerIcon.getSystemIcon(this, PointerIcon.TYPE_NULL)
    }
    private var hud: StreamHudController? = null
    private var gestureHint: GestureHintOverlay? = null
    private var cursorOverlay: CursorOverlayView? = null
    private var audioPlayer: StreamAudioPlayer? = null
    private val inputOwnership = InputOwnershipController()
    private var keyBridgeInput: KeyBridgeInputAdapter? = null
    private var pendingCaptureView: View? = null
    private var consumeEscapeUp = false
    private var localCursorEnabled: Boolean = false
    // Read by the display-clock thread inside its deliver lambda.
    @Volatile private var balancedPresentation: Boolean = false
    // Micro-jitter presentation smoothing (전문 설정 토글 → JNI 전역 스위치).
    @Volatile private var presentationSmooth: Boolean = true
    private var activityStarted = false
    private val connectionOwner = Any()
    // Balanced pacing releases at most one frame per vsync slot, so its
    // cadence is capped by how fresh the Choreographer samples arrive. A
    // main-thread Choreographer skips frames under UI load and was measured
    // capping paced release at ~55fps (2026-09-17); Moonlight runs its pacing
    // Choreographer on a dedicated thread for exactly this reason.
    private val displayClockThread by lazy {
        android.os.HandlerThread(
            "LeftcarDisplayClock",
            android.os.Process.THREAD_PRIORITY_DEFAULT + android.os.Process.THREAD_PRIORITY_MORE_FAVORABLE,
        ).apply { start() }
    }
    private val displayClockHandler by lazy { Handler(displayClockThread.looper) }
    private val displayManager by lazy {
        getSystemService(DISPLAY_SERVICE) as? android.hardware.display.DisplayManager
    }
    // Main-thread snapshot; per-frame refresh reads go through DisplayManager,
    // which is safe off the main thread unlike View.getDisplay().
    @Volatile private var displayClockDisplayId: Int = -1
    private val displayClock by lazy {
        DisplayFrameClock(
            post = { callback ->
                displayClockHandler.post {
                    android.view.Choreographer.getInstance().postFrameCallback(callback)
                }
            },
            remove = { callback ->
                displayClockHandler.post {
                    android.view.Choreographer.getInstance().removeFrameCallback(callback)
                }
            },
            display = {
                val id = displayClockDisplayId
                if (id < 0) null
                else displayManager?.getDisplay(id)?.let { it.displayId to it.refreshRate }
            },
            deliver = { display, frame, period ->
                val rc = ViewerNative.displayFrame(nativeState, instanceId, balancedPresentation || presentationSmooth, display, frame, period)
                if (rc != 0) android.util.Log.w("LeftcarStream", "displayFrame rc=$rc display=$display")
            },
        )
    }
    private fun syncPresentation() {
        displayClockDisplayId = window.decorView.display?.displayId ?: -1
        displayClock.stop()
        val paced = balancedPresentation || presentationSmooth
        val rc = ViewerNative.displayFrame(nativeState, instanceId, paced, -1, 0, 0)
        val started = paced && activityStarted && surfaceLifecycle.isAttached
        if (started) displayClock.start()
        android.util.Log.i("LeftcarStream", "syncPresentation paced=$paced smooth=$presentationSmooth attach=${surfaceLifecycle.isAttached} started=$started displayFrameRc=$rc")
    }

    private var localAudioEnabled: Boolean = true
    private var opusAudioRequested: Boolean = false
    private var textLens: TextInputLensView? = null
    private var keyboardRequested = false
    private var terminationHandled = false
    private var recoveryRetryRunnable: Runnable? = null
    private var recoveryFallbackEmitted = false
    private var recoveryFlowWatchdog: Runnable? = null
    private var xrSession: Session? = null
    private var xrPreferredRatio: Float? = null
    /**
     * 창은 항상 소스 비율 힌트로 연다(2026-09-20 결정: 창 크기 개입은 전부
     * 철회). 시스템이 기본 폭에 비율을 맞춰 크고 고정된 16:9 패널을 유지해
     * 준다 — 절대 크기 지정(setMainWindowSize)은 수락돼도 실제 창에
     * 반영되지 않아 폐기했고, 배율 칩도 사용자 요청으로 제거했다.
     */
    private var xrRatioGeneration = 0L
    private var xrCreationInFlight: Job? = null
    private var ownershipGeneration: Long = 0L

    private fun applyXrPreferredAspectRatio(force: Boolean = false) {
        if (!packageManager.hasSystemFeature("android.software.xr.api.spatial")) return
        val ratio =
            sourceWidth.toFloat().coerceAtLeast(1f) / sourceHeight.coerceAtLeast(1).toFloat()
        if (!force && xrPreferredRatio == ratio) return
        val generation = ++xrRatioGeneration
        val existing = xrSession
        if (existing != null) {
            runCatching { SpatialWindowBridge.setPreferredAspectRatio(existing, this, ratio) }
                .onSuccess { xrPreferredRatio = ratio }
                .onFailure { android.util.Log.i("LeftcarStream", "XR preferred ratio unavailable; keeping system panel size", it) }
            return
        }
        if (xrCreationInFlight?.isActive == true) return
        xrCreationInFlight = lifecycleScope.launch {
            val created = runCatching {
                withContext(Dispatchers.IO) {
                    Session.create(this@StreamActivity, Dispatchers.Default, this@StreamActivity)
                }
            }.onFailure {
                android.util.Log.i("LeftcarStream", "XR session unavailable; using normal Android window", it)
            }.getOrNull() as? SessionCreateSuccess ?: return@launch
            xrSession = created.session
            if (generation != xrRatioGeneration) {
                applyXrPreferredAspectRatio(force = true)
                return@launch
            }
            runCatching { SpatialWindowBridge.setPreferredAspectRatio(created.session, this@StreamActivity, ratio) }
                .onSuccess { xrPreferredRatio = ratio }
                .onFailure { android.util.Log.i("LeftcarStream", "XR preferred ratio rejected; keeping system panel size", it) }
        }
    }

    /**
     * Both Host notices and local renderer watchdogs close the stale Surface.
     * Only local reasons notify React so it can reconnect with the original port.
     */
    private fun handleTermination(reason: Int) {
        if (terminationHandled) return
        terminationHandled = true
        val message = ViewerStrings.terminationMessage(reason)
        if (reason == 1 || reason == 4) {
            StreamLauncherModule.emitTermination(port, reason)
        }
        android.util.Log.i("LeftcarStream", "stream termination reason=$reason: $message")
        if (isSameWindowRecoveryReason(reason)) {
            if (reason == 5) {
                // Keep the visible Activity and Surface alive. First retry the
                // renderer directly on the same port; only exhaust the short
                // native budget before asking React/Host to recreate the session.
                hud?.showRebindIndicator(ViewerStrings.rebindReconnecting)
                scheduleRenderRecovery()
            } else {
                // A complete Wi-Fi outage tears down the Host session, so the
                // React controller owns the reconnect. Retain this Activity so
                // its existing window can receive the next stream intent.
                recoveryRetryRunnable?.let(recoveryHandler::removeCallbacks)
                recoveryRetryRunnable = null
                recoveryRetryPolicy.reset()
                recoveryFallbackEmitted = false
                hud?.showRebindIndicator(ViewerStrings.rebindReconnectingControl)
                startTerminationReemit(reason)
            }
            return
        }
        setResult(2, android.content.Intent().putExtra("terminationReason", reason))
        finish()
        android.widget.Toast.makeText(applicationContext, message, android.widget.Toast.LENGTH_LONG)
            .show()
    }

    /**
     * React 복원은 단발 종료 이벤트에 의존하는데, 호스트가 내려가 있는 동안에는
     * 첫 restore가 반드시 실패하고 RN 타이머(setTimeout)는 StreamActivity
     * 포그라운드에서 멈춰 JS 측 재시도가 발화하지 않는다. 복구가 끝날 때까지
     * 같은 종료 이벤트를 주기적으로 재발행해 React 복원을 다시 깨운다 —
     * rebind 성공(rebindOnSameSurface·markRenderHealthy)이 이 루프를 지운다.
     */
    private fun startTerminationReemit(reason: Int) {
        cancelTerminationReemit()
        val runnable = object : Runnable {
            override fun run() {
                if (released || isFinishing || isDestroyed || !terminationHandled) return
                StreamLauncherModule.emitTermination(port, reason)
                recoveryHandler.postDelayed(this, TERMINATION_REEMIT_INTERVAL_MS)
            }
        }
        recoveryHandler.postDelayed(runnable, TERMINATION_REEMIT_INTERVAL_MS)
        reemitTerminationRunnable = runnable
    }

    private fun cancelTerminationReemit() {
        reemitTerminationRunnable?.let(recoveryHandler::removeCallbacks)
        reemitTerminationRunnable = null
    }

    private fun scheduleRenderRecovery() {
        if (released || isFinishing || isDestroyed) return
        cancelAttachFlowWatchdog()
        // A split renderer cannot rebind in place: its two UDP listeners are
        // stopped by detach, and the single rebindSurfacePort path would
        // rebuild a single renderer against a split-prepared Host. Split
        // recovery delegates to the React/Host re-preparation path at once.
        if (splitVertical) {
            if (!recoveryFallbackEmitted) {
                recoveryFallbackEmitted = true
                android.util.Log.w(
                    "LeftcarStream",
                    "split render recovery requires React/Host re-preparation; " +
                        "instanceId=$instanceId port=$port",
                )
                StreamLauncherModule.emitTermination(port, 5)
            }
            return
        }
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
            if (StreamLauncherModule.activeRegisteredReactContext() == null) {
                android.util.Log.i(
                    "LeftcarStream",
                    "React context unavailable (MainActivity closed); resetting recovery retry policy after delay",
                )
                recoveryRetryRunnable?.let(recoveryHandler::removeCallbacks)
                val selfHeal = Runnable {
                    recoveryRetryRunnable = null
                    recoveryFallbackEmitted = false
                    recoveryRetryPolicy.reset()
                    scheduleRenderRecovery()
                }
                recoveryRetryRunnable = selfHeal
                recoveryHandler.postDelayed(selfHeal, 5_000L)
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
        val result = rebindOnSameSurface()
        android.util.Log.i(
            "LeftcarStream",
            "local render recovery attempt=${attempt.number} result=$result " +
                "port=$port source=${sourceWidth}x${sourceHeight} fps=$fps",
        )
        // HUD 통지는 rebindOnSameSurface가 이미 했다 — 여기서 또 부르면
        // 재시도마다 종료 폴링 재무장과 인디케이터 갱신이 두 번 일어난다.
        if (result != 0) {
            scheduleRenderRecovery()
        } else {
            armRecoveryFlowWatchdog(attempt.number)
        }
    }

    /**
     * A successful rebind only proves the renderer spawned — not that media
     * arrived. When the Host session died mid-recovery (observed 2026-09-17:
     * reason-5 detach, host error, "no session media crypto") the rebinding
     * succeeds, every recovery flag resets, and the window sits on the last
     * frozen frame indefinitely. Watch for rendered progress; without it the
     * retry policy resumes and finally hands the stream to React's reconnect.
     */
    private fun armRecoveryFlowWatchdog(attempt: Int) {
        cancelRecoveryFlowWatchdog()
        val baseline = ViewerNative.streamStats(instanceId) and ((1L shl 28) - 1)
        val watchdog = Runnable {
            recoveryFlowWatchdog = null
            if (released || isFinishing || isDestroyed) return@Runnable
            val rendered = ViewerNative.streamStats(instanceId) and ((1L shl 28) - 1)
            if (rendered > baseline) return@Runnable
            android.util.Log.w(
                "LeftcarStream",
                "local render recovery attempt=$attempt produced no frames; retrying " +
                    "instanceId=$instanceId port=$port",
            )
            hud?.showRebindIndicator(ViewerStrings.rebindReconnecting)
            scheduleRenderRecovery()
        }
        recoveryFlowWatchdog = watchdog
        recoveryHandler.postDelayed(watchdog, RECOVERY_FLOW_WATCHDOG_MS)
    }

    private fun cancelRecoveryFlowWatchdog() {
        recoveryFlowWatchdog?.let(recoveryHandler::removeCallbacks)
        recoveryFlowWatchdog = null
    }

    private var attachFlowWatchdog: Runnable? = null

    /**
     * Initial or resumed surface attach succeeds at the JNI level immediately,
     * but media packets or the initial IDR can be lost over UDP. If rendered_frames
     * does not advance within [ATTACH_FLOW_WATCHDOG_MS], trigger recovery to
     * avoid a permanent black screen.
     */
    private fun armAttachFlowWatchdog() {
        cancelAttachFlowWatchdog()
        val baseline = ViewerNative.streamStats(instanceId) and ((1L shl 28) - 1)
        val watchdog = Runnable {
            attachFlowWatchdog = null
            if (released || isFinishing || isDestroyed) return@Runnable
            val rendered = ViewerNative.streamStats(instanceId) and ((1L shl 28) - 1)
            if (rendered > baseline) return@Runnable
            android.util.Log.w(
                "LeftcarStream",
                "surface attach produced no rendered frames within ${ATTACH_FLOW_WATCHDOG_MS}ms; triggering recovery " +
                    "instanceId=$instanceId port=$port",
            )
            hud?.showRebindIndicator(ViewerStrings.rebindReconnecting)
            scheduleRenderRecovery()
        }
        attachFlowWatchdog = watchdog
        recoveryHandler.postDelayed(watchdog, ATTACH_FLOW_WATCHDOG_MS)
    }

    private fun cancelAttachFlowWatchdog() {
        attachFlowWatchdog?.let(recoveryHandler::removeCallbacks)
        attachFlowWatchdog = null
    }

    /**
     * Swap the live renderer onto the current Surface and geometry. Shared by
     * the render-recovery retry and the same-window stream intent: both own
     * the success bookkeeping (reset termination state, notify the HUD), so
     * the two call sites cannot drift.
     */
    private fun rebindOnSameSurface(): Int {
        displayClock.stop()
        if (splitVertical) {
            // The split renderer binds two listeners; a single rebind would
            // strand the right port and poison the next split attach. (Also
            // unreachable: decideSurfaceTransition never rebinds in place on
            // a split hierarchy, and split recovery delegates to React.)
            android.util.Log.w(
                "LeftcarStream",
                "rebindOnSameSurface ignored in split mode; delegating to React",
            )
            return -1
        }
        val surface = streamSurfaces?.left?.holder?.surface
        val result = if (nativeState != 0L && !released && surface != null && surface.isValid) {
            ViewerNative.rebindSurfacePortWithPresentation(
                nativeState,
                instanceId,
                surface,
                port,
                host,
                sourceWidth,
                sourceHeight,
                fps,
                balancedPresentation,
            )
        } else {
            -1
        }
        surfaceLifecycle.confirmAttached(result == 0)
        inputLanguageMonitor.reset()
        if (result == 0) {
            terminationHandled = false
            recoveryFallbackEmitted = false
            recoveryRetryPolicy.reset()
            cancelTerminationReemit()
            // A rebind builds a fresh renderer session, so the cursor stream
            // subscription must ride again with the new control channel.
            subscribeCursorStream()
            syncAudioStream()
        }
        hud?.onRebindFinished(result == 0)
        return result
    }

    private fun markRenderHealthy() {
        recoveryRetryRunnable?.let(recoveryHandler::removeCallbacks)
        recoveryRetryRunnable = null
        recoveryRetryPolicy.reset()
        recoveryFallbackEmitted = false
        cancelRecoveryFlowWatchdog()
        cancelAttachFlowWatchdog()
        cancelTerminationReemit()
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

    /** Keep host cursor data available; ownership decides which cursor is visible. */
    private fun subscribeCursorStream() {
        // localCursor=false는 LCD1 구독 자체를 끈다(LCDOFF) — 호스트가 영상에
        // 심는 실제 커서만 남긴다. 켜져 있으면 소유권 프레젠테이션이 따라간다.
        ViewerNative.setCursorStream(instanceId, localCursorEnabled)
        if (!localCursorEnabled) {
            hideRemoteCursorOverlay()
            return
        }
        applyCursorPresentation()
    }

    private fun showRemoteCursorOverlay() {
        if (streamZoom.isZoomed) return
        val overlay = cursorOverlay ?: CursorOverlayView(this, instanceId).also { view ->
            cursorOverlay = view
        }
        overlay.setVideoSize(sourceWidth, sourceHeight)
        overlay.start()
    }

    private fun hideRemoteCursorOverlay() {
        cursorOverlay?.stop()
        cursorOverlay = null
    }

    private fun setLocalCursorVisible(visible: Boolean) {
        val type = if (visible) PointerIcon.TYPE_ARROW else PointerIcon.TYPE_NULL
        streamSurfaces?.left?.pointerIcon = PointerIcon.getSystemIcon(this, type)
        streamSurfaces?.right?.pointerIcon = PointerIcon.getSystemIcon(this, type)
    }

    private fun applyCursorPresentation() {
        if (inputOwnership.owner == InputOwner.REMOTE_MAC) {
            setLocalCursorVisible(false)
            showRemoteCursorOverlay()
        } else {
            hideRemoteCursorOverlay()
            setLocalCursorVisible(true)
        }
    }

    /**
     * SNDON/SNDOFF는 멱등 커맨드라서 코어가 1초 주기로 재전송하므로 여기서는
     * 렌더러에 뷰어의 현재 선호만 저장하면 된다. attach·재바인드 직후 호출해
     * 새로 만들어진 세션에도 선호가 즉시 반영되게 한다.
     */
    internal fun audioStats(): Map<String, Any?> {
        val value = audioPlayer?.metrics
        return mapOf("requestedCodec" to if (opusAudioRequested) "opus128k" else "pcm",
            "effectiveCodec" to value?.effectiveCodec, "enabled" to localAudioEnabled,
            "requestedBufferFrames" to value?.requestedBufferFrames, "actualBufferFrames" to value?.actualBufferFrames,
            "capacityFrames" to value?.capacityFrames, "underruns" to value?.underruns,
            "writtenFrames" to value?.writtenFrames?.toDouble(), "playbackFrames" to value?.playbackFrames?.toDouble(),
            "writeErrors" to value?.writeErrors?.toDouble(), "decodeNanoseconds" to value?.decodeNanoseconds?.toDouble(),
            "avSkewUs" to null)
    }

    private fun syncAudioStream() {
        ViewerNative.setAudioOwned(nativeState, instanceId, localAudioEnabled, audioPlayer?.configureOpus(opusAudioRequested) == true)
        if (localAudioEnabled) audioPlayer?.start() else audioPlayer?.stop()
        syncPresentation()
    }

    /**
     * 제스처 안내는 첫 스트림 창에서 자동으로 1회 보여 준다.
     * 닫힐 때 "본 적 있음" 플래그를 저장한다.
     */
    private fun showGestureHint() {
        val prefs = getSharedPreferences("leftcar_viewer", MODE_PRIVATE)
        if (prefs.getBoolean(GestureHintOverlay.PREF_SHOWN, false)) return
        gestureHint?.dismiss()
        // "본 적 있음"은 표시 시점에 기록한다 — 닫힘 시점 저장은 강제 종료로
        // 유실되면 다음 스트림마다 안내가 다시 떠 원격 입력을 가린다.
        prefs.edit().putBoolean(GestureHintOverlay.PREF_SHOWN, true).apply()
        gestureHint = GestureHintOverlay(this) {}.also { it.show() }
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

    private fun normalizedPoint(view: View, x: Float, y: Float): Pair<Float, Float> {
        val split = streamSurfaces?.right != null
        val videoWidth = if (split) sourceWidth / 2 else sourceWidth
        // 핀치줌 역변환: 뷰 좌표를 줌 이전 내용 좌표로 되돌린 뒤 어스펙트
        // 매핑을 적용한다(줌인 상태에서도 포인터가 정확한 원격 위치로 간다).
        val (contentX, contentY) = if (streamZoom.isZoomed) {
            streamZoom.toContent(x, y)
        } else {
            x to y
        }
        val mapped = mapAspectFitPoint(contentX, contentY, view.width, view.height, videoWidth, sourceHeight)
        val nx = when {
            !split -> mapped.first
            view === streamSurfaces?.right -> 0.5f + mapped.first * 0.5f
            else -> mapped.first * 0.5f
        }
        return nx to mapped.second
    }

    /**
     * 호스트가 원격 입력을 잠근 동안(상태 0)은 터치·마우스·키보드 이벤트를
     * 전송 단계에 넣기 전에 조용히 버린다. 호스트도 자체 게이트에서 폐기하지만,
     * 뷰어가 먼저 끊어야 잠금 내내 이어지는 UDP 전송·재전송과 무선 전력 낭비가
     * 없어지고 입력 배지와 실제 동작이 일치한다. 상태를 아직 모를 때(-1)는
     * 보낸다 — 세션 시작 직후 자동 허용 상태가 도착하기 전 첫 입력을 막지
     * 않기 위해서다.
     */
    private fun remoteInputLocked(): Boolean = ViewerNative.inputStatus(instanceId) == 0

    private val inputLanguageMonitor by lazy {
        StreamInputLanguageMonitor(
            this,
            active = { !released && hasWindowFocus() && ViewerNative.inputStatus(instanceId) == 1 },
            send = {
                val result = ViewerNative.sendInputLanguage(instanceId, it)
                if (result == 0) android.util.Log.i("LeftcarIme", "queued native input language=$it")
                result == 0
            },
        )
    }

    /** 잠금 중에는 전송하지 않고, 이벤트는 로컬에서 소비한 것으로 처리한다. */
    private fun sendPointerUnlocked(
        action: Int,
        x: Float,
        y: Float,
        buttons: Int,
        actionButton: Int,
        horizontalScroll: Float,
        verticalScroll: Float,
        pressure: Float = -1f,
    ): Boolean {
        if (remoteInputLocked()) return true
        val sent = ViewerNative.sendPointer(
            instanceId,
            action,
            x,
            y,
            buttons,
            actionButton,
            horizontalScroll,
            verticalScroll,
            pressure,
        ) == 0
        if (sent) cursorOverlay?.nudge()
        return sent
    }

    private fun sendKeyUnlocked(
        keyCode: Int,
        scanCode: Int,
        metaState: Int,
        down: Boolean,
        repeat: Int,
    ): Boolean {
        if (remoteInputLocked()) return true
        val sent = ViewerNative.sendKey(instanceId, keyCode, scanCode, metaState, down, repeat) == 0
        if (sent) cursorOverlay?.nudge()
        return sent
    }

    private fun sendTextUnlocked(text: String) {
        if (remoteInputLocked()) return
        ViewerNative.sendText(instanceId, text.toByteArray(Charsets.UTF_8))
        cursorOverlay?.nudge()
    }

    /** IME 텍스트 계열의 편집 키는 다운/업 페어로 왕복시킨다. */
    private fun sendKeyPairUnlocked(keyCode: Int) {
        sendKeyUnlocked(keyCode, 0, 0, true, 0)
        sendKeyUnlocked(keyCode, 0, 0, false, 0)
    }

    /**
     * 소프트키보드(IME)를 붙일 1×1 렌즈. decorView에 한 번만 붙으므로
     * Surface 재구성(rebuildStreamSurfaces)으로도 포커스·IME 상태가 유지된다.
     */
    private fun attachTextLens() {
        val relay = TextInputRelay(
            sendText = ::sendTextUnlocked,
            sendBackspace = { count ->
                repeat(count) { sendKeyPairUnlocked(TextInputRelay.KEYCODE_DEL) }
            },
            sendForwardDelete = { count ->
                repeat(count) { sendKeyPairUnlocked(TextInputRelay.KEYCODE_FORWARD_DEL) }
            },
            sendEnter = { sendKeyPairUnlocked(TextInputRelay.KEYCODE_ENTER) },
            sendKey = { keyCode -> sendKeyPairUnlocked(keyCode) },
        )
        val lens = TextInputLensView(this, relay).also { view ->
            view.onImeVisibilityChanged = { visible ->
                keyboardRequested = visible
            }
            textLens = view
        }
        (window.decorView as android.view.ViewGroup).addView(
            lens,
            android.widget.FrameLayout.LayoutParams(1, 1),
        )
    }

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
        // 스트림 창 안에서는 안드로이드 시스템 화살표를 끈다 — 커서 표시는
        // LCD1 오버레이가 담당하고, 오버레이가 꺼진 상태(줌·로컬 커서 끔·
        // 분할·구호스트)에서는 LCDOFF 뒤 호스트가 영상에 심는 실제 커서가
        // 남는다. 화살표를 보여주면 어느 상태든 이중 커서가 된다. 아이콘은
        // 이 창의 뷰에만 걸리므로 창 밖 시스템 동작은 그대로다.
        tabletCursorHandler.removeCallbacks(hideTabletCursorRunnable)
        view.pointerIcon = PointerIcon.getSystemIcon(this, PointerIcon.TYPE_NULL)
        // 하단 가장자리: 진입 즉시 트랜지언트 바(보이기만), 600ms 거주하면
        // 인터랙티브 바로 전환 — 트랜지언트 바는 터치를 받지 않아 트레이를
        // 눌러도 홈·최근 앱이 동작하지 않기 때문이다. 벗어나면 다시 숨긴다.
        val inBottomZone = event.y >= view.height - (24 * resources.displayMetrics.density).toInt()
        if (inBottomZone) {
            revealSystemBarsTransiently(view)
            if (!lastPointerInBottomZone) scheduleSystemBarsDwell(view)
        } else if (lastPointerInBottomZone) {
            cancelSystemBarsDwell()
            if (systemBarsInteractive) collapseSystemBars(view)
        }
        lastPointerInBottomZone = inBottomZone
    }

    private fun scheduleSystemBarsDwell(view: View) {
        bottomZoneDwellRunnable?.let(tabletCursorHandler::removeCallbacks)
        val runnable = Runnable {
            if (lastPointerInBottomZone && !released && !isFinishing) {
                view.windowInsetsController?.let { controller ->
                    controller.systemBarsBehavior = WindowInsetsController.BEHAVIOR_DEFAULT
                    controller.show(WindowInsets.Type.statusBars() or WindowInsets.Type.navigationBars())
                    systemBarsInteractive = true
                }
            }
        }
        bottomZoneDwellRunnable = runnable
        tabletCursorHandler.postDelayed(runnable, SYSTEM_BARS_DWELL_MS)
    }

    private fun cancelSystemBarsDwell() {
        bottomZoneDwellRunnable?.let(tabletCursorHandler::removeCallbacks)
        bottomZoneDwellRunnable = null
    }

    private fun collapseSystemBars(view: View) {
        view.windowInsetsController?.let { controller ->
            controller.systemBarsBehavior =
                WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            controller.hide(WindowInsets.Type.statusBars() or WindowInsets.Type.navigationBars())
        }
    }

    private fun revealSystemBarsTransiently(view: View) {
        val now = android.os.SystemClock.uptimeMillis()
        if (now - lastSystemBarsRevealAt < SYSTEM_BARS_REVEAL_THROTTLE_MS) return
        lastSystemBarsRevealAt = now
        view.windowInsetsController?.show(
            WindowInsets.Type.statusBars() or WindowInsets.Type.navigationBars()
        )
    }


    fun toggleSoftKeyboard() {
        setSoftKeyboard(!keyboardRequested)
    }

    private fun setSoftKeyboard(active: Boolean) {
        keyboardRequested = active
        val lens = textLens ?: return
        val imm = getSystemService(INPUT_METHOD_SERVICE)
            as? android.view.inputmethod.InputMethodManager ?: return
        if (active) {
            lens.requestFocus()
            // showSoftInput은 포커스 처리가 끝난 뒤에 호출돼야 확실히 붙는다.
            lens.post {
                if (!keyboardRequested || !lens.hasWindowFocus()) return@post
                imm.showSoftInput(lens, 0)
            }
        } else {
            imm.hideSoftInputFromWindow(lens.windowToken, 0)
            lens.clearFocus()
            streamSurfaces?.requestFocus()
        }
    }

    private fun applyInputOwnershipEffects(
        effects: List<InputOwnershipEffect>,
        activationView: View? = null,
    ) {
        for (effect in effects) {
            when (effect) {
                InputOwnershipEffect.ACQUIRE_KEYBRIDGE -> {
                    val target = activationView ?: pendingCaptureView ?: streamSurfaces?.left
                    keyBridgeInput?.acquire { acquired ->
                        if (!acquired) {
                            forceReleaseInput(InputOwnershipEvent.RemoteAcquireFailed)
                            val message = if (keyBridgeInput?.availability == KeyBridgeAvailability.UPDATE_REQUIRED) {
                                ViewerStrings.keyBridgeUpdateRequired
                            } else {
                                ViewerStrings.keyBridgeUnavailable
                            }
                            android.widget.Toast.makeText(this, message, android.widget.Toast.LENGTH_LONG).show()
                            return@acquire
                        }
                        if (inputOwnership.owner != InputOwner.ACQUIRING_REMOTE ||
                            target == null || !target.hasWindowFocus()
                        ) {
                            keyBridgeInput?.release()
                            forceReleaseInput(InputOwnershipEvent.FocusLost)
                            return@acquire
                        }
                        if (android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.O) {
                            forceReleaseInput(InputOwnershipEvent.RemoteAcquireFailed)
                            return@acquire
                        }
                        runCatching {
                            target.requestFocus()
                            target.requestPointerCapture()
                        }.onFailure {
                            forceReleaseInput(InputOwnershipEvent.RemoteAcquireFailed)
                        }
                        target.postDelayed({
                            if (inputOwnership.owner == InputOwner.ACQUIRING_REMOTE) {
                                forceReleaseInput(InputOwnershipEvent.RemoteAcquireFailed)
                            }
                        }, 1_000L)
                    }
                }
                // Pointer capture is requested only after the asynchronous
                // KeyBridge acquire above succeeds.
                InputOwnershipEffect.REQUEST_POINTER_CAPTURE -> Unit
                InputOwnershipEffect.HIDE_LOCAL_CURSOR -> setLocalCursorVisible(false)
                InputOwnershipEffect.SHOW_REMOTE_CURSOR -> showRemoteCursorOverlay()
                InputOwnershipEffect.FORWARD_POINTER -> Unit
                InputOwnershipEffect.RELEASE_REMOTE_INPUT -> ViewerNative.releaseInput(instanceId)
                InputOwnershipEffect.RELEASE_POINTER_CAPTURE -> {
                    if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.O) {
                        listOfNotNull(streamSurfaces?.left, streamSurfaces?.right)
                            .firstOrNull { it.hasPointerCapture() }
                            ?.releasePointerCapture()
                    }
                    pendingCaptureView = null
                }
                InputOwnershipEffect.RELEASE_KEYBRIDGE -> keyBridgeInput?.release()
                InputOwnershipEffect.HIDE_REMOTE_CURSOR -> hideRemoteCursorOverlay()
                InputOwnershipEffect.SHOW_LOCAL_CURSOR -> setLocalCursorVisible(true)
            }
        }
    }

    private fun forceReleaseInput(event: InputOwnershipEvent) {
        val effects = inputOwnership.on(event)
        if (effects.isEmpty()) ViewerNative.releaseInput(instanceId)
        applyInputOwnershipEffects(effects)
    }

    private fun activateRemoteInput(event: MotionEvent, view: View): Boolean {
        hideRemoteCursorOverlay()
        setLocalCursorVisible(true)
        if (inputOwnership.owner == InputOwner.ACQUIRING_REMOTE) return true
        if (inputOwnership.owner != InputOwner.LOCAL_ANDROID) return false
        if (event.actionMasked != MotionEvent.ACTION_BUTTON_PRESS &&
            event.actionMasked != MotionEvent.ACTION_DOWN
        ) return false
        if (remoteInputLocked()) {
            forceReleaseInput(InputOwnershipEvent.HostInputDisabled)
            return true
        }
        if (streamZoom.isZoomed) {
            streamZoom.reset()
            applyZoomToSurfaces()
        }
        val (nx, ny) = normalizedPoint(view, event.x, event.y)
        pendingCaptureView = view
        applyInputOwnershipEffects(
            inputOwnership.on(InputOwnershipEvent.MouseActivation(nx, ny)),
            view,
        )
        return true
    }


    private fun forwardPointer(event: MotionEvent, view: View): Boolean {
        StreamPointerDiagnostics.record(event)
        if (android.os.Build.VERSION.SDK_INT >= 34 &&
            event.classification == MotionEvent.CLASSIFICATION_TWO_FINGER_SWIPE
        ) {
            return forwardTouchpadScroll(event, view)
        }
        touchpadScrollPosition = null
        // Touchscreen input goes through the gesture machine (tap, drag,
        // two-finger scroll, long-press right click); physical mice and
        // styluses keep the direct event mapping below.
        if (event.isFromSource(InputDevice.SOURCE_TOUCHSCREEN)) {
            setLocalCursorVisible(false)
            showRemoteCursorOverlay()
            return forwardTouchGesture(event, view)
        }
        if (event.isFromSource(InputDevice.SOURCE_MOUSE) &&
            inputOwnership.owner != InputOwner.REMOTE_MAC
        ) {
            // 첫 물리 마우스 조작은 원격 소유권 획득(KeyBridge 연동)을 촉발한다.
            // 획득 결과와 무관하게 그 조작 자체는 아래 공용 매핑으로 호스트에
            // 그대로 전송된다 — 탭 착지 보존(실기기 검증 계약).
            activateRemoteInput(event, view)
        }
        // Touchscreen events never reach here (routed to the gesture machine
        // above), so only the stylus still counts as touch-like.
        val touchLike = event.isFromSource(InputDevice.SOURCE_STYLUS)
        if (touchLike) {
            setLocalCursorVisible(false)
            showRemoteCursorOverlay()
        }
        if (event.actionMasked == MotionEvent.ACTION_DOWN ||
            event.actionMasked == MotionEvent.ACTION_BUTTON_PRESS
        ) {
            hud?.revealInput()
            hud?.revealStats()
            // 즉각 피드백: 물리 버튼 누름/스타일러스 터치는 호스트 왕복 전에
            // 로컬 물결로 반응을 보여 준다.
            if (!remoteInputLocked() &&
                (event.actionMasked == MotionEvent.ACTION_BUTTON_PRESS || touchLike)
            ) {
                cursorOverlay?.showTapEcho(event.rawX, event.rawY)
            }
        }
        // Android normally batches/resamples pointer motion around display
        // frames. A remote-control Surface needs the hardware samples early;
        // Rust still coalesces them to the bounded 120Hz-floor wire target.
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R &&
            (event.actionMasked == MotionEvent.ACTION_HOVER_ENTER ||
                event.actionMasked == MotionEvent.ACTION_HOVER_MOVE ||
                event.actionMasked == MotionEvent.ACTION_MOVE ||
                event.actionMasked == MotionEvent.ACTION_BUTTON_PRESS)
        ) {
            view.requestUnbufferedDispatch(event.source)
        }
        val action = when (event.actionMasked) {
            MotionEvent.ACTION_HOVER_MOVE, MotionEvent.ACTION_MOVE -> 1
            MotionEvent.ACTION_BUTTON_PRESS -> 2
            MotionEvent.ACTION_BUTTON_RELEASE -> 3
            // A mouse also emits touch DOWN/UP around its generic button
            // events. Consume that touch sequence so ViewGroup keeps routing
            // drag MOVE samples here; only BUTTON_PRESS/RELEASE send edges.
            MotionEvent.ACTION_DOWN -> if (touchLike) 2 else return event.isFromSource(InputDevice.SOURCE_MOUSE)
            MotionEvent.ACTION_UP -> if (touchLike) 3 else return event.isFromSource(InputDevice.SOURCE_MOUSE)
            MotionEvent.ACTION_SCROLL -> 4
            MotionEvent.ACTION_CANCEL -> {
                forceReleaseInput(InputOwnershipEvent.Disconnected)
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
        // 스타일러스만 압력을 싣는다 — 손가락·마우스의 pressure는
        // 기기 의존적이라 의미가 없다(음수 = 압력 없음).
        val stylusPressure =
            if (event.getToolType(0) == MotionEvent.TOOL_TYPE_STYLUS) {
                event.pressure.coerceIn(0f, 1f)
            } else {
                -1f
            }
        val (nx, ny) = normalizedPoint(view, event.x, event.y)
        return sendPointerUnlocked(
            action,
            nx,
            ny,
            buttons,
            actionButton,
            event.getAxisValue(MotionEvent.AXIS_HSCROLL),
            event.getAxisValue(MotionEvent.AXIS_VSCROLL),
            stylusPressure,
        )
    }

    private var touchpadScrollPosition: Pair<Float, Float>? = null

    private fun forwardTouchpadScroll(event: MotionEvent, view: View): Boolean {
        // Android 14+ touchpads send classified MOVE events with pixel
        // scroll distances, not wheel axes. Consume the entire gesture so
        // it cannot become cursor movement or a mouse click.
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                touchpadScrollPosition = event.x to event.y
                return true
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL, MotionEvent.ACTION_HOVER_EXIT -> {
                touchpadScrollPosition = null
                return true
            }
            MotionEvent.ACTION_MOVE, MotionEvent.ACTION_SCROLL -> Unit
            else -> return true
        }
        var horizontal = event.getAxisValue(MotionEvent.AXIS_GESTURE_SCROLL_X_DISTANCE)
        var vertical = event.getAxisValue(MotionEvent.AXIS_GESTURE_SCROLL_Y_DISTANCE)
        for (history in 0 until event.historySize) {
            horizontal += event.getHistoricalAxisValue(MotionEvent.AXIS_GESTURE_SCROLL_X_DISTANCE, history)
            vertical += event.getHistoricalAxisValue(MotionEvent.AXIS_GESTURE_SCROLL_Y_DISTANCE, history)
        }
        val previous = touchpadScrollPosition
        touchpadScrollPosition = event.x to event.y
        // Lenovo's classified fake-finger gestures can omit the distance
        // axes. Android moves their X/Y in the opposite direction to the
        // scroll distance. Only this classified gesture gets the fallback;
        // ordinary cursor movement must never become scrolling. The latest
        // position includes batched samples, so the endpoint delta counts
        // each displacement once.
        if (horizontal == 0f && vertical == 0f && previous != null) {
            horizontal = previous.first - event.x
            vertical = previous.second - event.y
        }
        if (horizontal != 0f || vertical != 0f) {
            val (nx, ny) = normalizedPoint(view, event.x, event.y)
            sendPointerUnlocked(
                4, nx, ny, 0, 0,
                horizontal * TouchGestureStateMachine.DEFAULT_LINES_PER_PIXEL,
                vertical * TouchGestureStateMachine.DEFAULT_LINES_PER_PIXEL,
            )
        }
        return true
    }


    private fun forwardCapturedPointer(event: MotionEvent, view: View): Boolean {
        if (inputOwnership.owner != InputOwner.REMOTE_MAC) return false
        if (remoteInputLocked()) {
            forceReleaseInput(InputOwnershipEvent.HostInputDisabled)
            return true
        }
        if (event.actionMasked == MotionEvent.ACTION_CANCEL) {
            forceReleaseInput(InputOwnershipEvent.Disconnected)
            return true
        }
        val dx = event.getAxisValue(MotionEvent.AXIS_RELATIVE_X)
        val dy = event.getAxisValue(MotionEvent.AXIS_RELATIVE_Y)
        val surfaces = streamSurfaces
        val canvasWidth = if (surfaces?.right != null) {
            surfaces.left.width + surfaces.right.width
        } else {
            view.width
        }.coerceAtLeast(1)
        val canvasHeight = if (surfaces?.right != null) {
            maxOf(surfaces.left.height, surfaces.right.height)
        } else {
            view.height
        }.coerceAtLeast(1)
        applyInputOwnershipEffects(
            inputOwnership.on(
                InputOwnershipEvent.CapturedMove(dx, dy, canvasWidth, canvasHeight),
            ),
        )
        if (inputOwnership.owner != InputOwner.REMOTE_MAC) return true

        val action = when (event.actionMasked) {
            MotionEvent.ACTION_HOVER_MOVE, MotionEvent.ACTION_MOVE -> 1
            MotionEvent.ACTION_BUTTON_PRESS, MotionEvent.ACTION_DOWN -> 2
            MotionEvent.ACTION_BUTTON_RELEASE, MotionEvent.ACTION_UP -> 3
            MotionEvent.ACTION_SCROLL -> 4
            else -> return false
        }
        if (action == 2) {
            hud?.revealInput()
            hud?.revealStats()
        }
        val position = inputOwnership.pointerPosition
        val actionButton = when {
            event.actionButton != 0 -> event.actionButton
            action == 2 || action == 3 -> MotionEvent.BUTTON_PRIMARY
            else -> 0
        }
        return sendPointerUnlocked(
            action,
            position.x,
            position.y,
            event.buttonState,
            actionButton,
            event.getAxisValue(MotionEvent.AXIS_HSCROLL),
            event.getAxisValue(MotionEvent.AXIS_VSCROLL),
        )
    }

    private val gestureHandler = Handler(Looper.getMainLooper())
    // Activity field initializers run before onCreate attaches the base
    // context, so anything needing Context must wait for first use.
    private val touchGestures by lazy {
        TouchGestureStateMachine(
            touchSlopPx = ViewConfiguration.get(this).scaledTouchSlop.toFloat(),
        )
    }

    /** 핀치줌 상태 — 좌·우 분할 타일 모두 같은 변환으로 확대한다. */
    private val streamZoom = StreamZoomState()
    private var longPressRunnable: Runnable? = null
    private var gestureLastX = 0f
    private var gestureLastY = 0f

    private fun forwardTouchGesture(event: MotionEvent, view: View): Boolean {
        if (event.actionMasked == MotionEvent.ACTION_DOWN) {
            hud?.revealInput()
            hud?.revealStats()
            // 즉각 피드백: 손가락 탭도 첫 프레임 안에 로컬 물결로 확인시킨다.
            if (!remoteInputLocked()) {
                cursorOverlay?.showTapEcho(event.rawX, event.rawY)
            }
        }
        if (event.actionMasked == MotionEvent.ACTION_SCROLL) {
            // Wheel-style scroll events carry axis payloads directly; the
            // finger state machine has no equivalent phase.
            val (nx, ny) = normalizedPoint(view, event.x, event.y)
            sendPointerUnlocked(
                4,
                nx,
                ny,
                0,
                0,
                event.getAxisValue(MotionEvent.AXIS_HSCROLL),
                event.getAxisValue(MotionEvent.AXIS_VSCROLL),
            )
            return true
        }
        var centroidX = 0f
        var centroidY = 0f
        for (index in 0 until event.pointerCount) {
            centroidX += event.getX(index)
            centroidY += event.getY(index)
        }
        if (event.pointerCount > 0) {
            centroidX /= event.pointerCount
            centroidY /= event.pointerCount
        }
        val spanPx = if (event.pointerCount >= 2) {
            val dx = event.getX(0) - event.getX(1)
            val dy = event.getY(0) - event.getY(1)
            kotlin.math.sqrt(dx * dx + dy * dy)
        } else {
            0f
        }
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN,
            MotionEvent.ACTION_POINTER_DOWN,
            MotionEvent.ACTION_MOVE,
            -> {
                gestureLastX = event.x
                gestureLastY = event.y
            }
        }
        val commands = touchGestures.onTouchEvent(
            event.actionMasked,
            event.pointerCount,
            event.x,
            event.y,
            centroidX,
            centroidY,
            spanPx,
        )
        commands.forEach { command -> runGestureCommand(command, view) }
        syncLongPressTimer(view)
        return true
    }

    private fun syncLongPressTimer(view: View) {
        longPressRunnable?.let(gestureHandler::removeCallbacks)
        longPressRunnable = null
        if (!touchGestures.longPressPending) return
        val x = gestureLastX
        val y = gestureLastY
        val runnable = Runnable {
            longPressRunnable = null
            val fired = touchGestures.longPressFired(x, y)
            fired.forEach { runGestureCommand(it, view) }
        }
        longPressRunnable = runnable
        gestureHandler.postDelayed(runnable, ViewConfiguration.getLongPressTimeout().toLong())
    }

    private fun runGestureCommand(command: TouchGestureCommand, view: View) {
        when (command) {
            is TouchGestureCommand.Move -> {
                val (nx, ny) = normalizedPoint(view, command.x, command.y)
                sendPointerUnlocked(
                    1,
                    nx,
                    ny,
                    command.buttons,
                    0,
                    0f,
                    0f,
                )
            }
            is TouchGestureCommand.Button -> {
                val (nx, ny) = normalizedPoint(view, command.x, command.y)
                sendPointerUnlocked(
                    if (command.down) 2 else 3,
                    nx,
                    ny,
                    command.button,
                    command.button,
                    0f,
                    0f,
                )
            }
            is TouchGestureCommand.Scroll -> {
                val (nx, ny) = normalizedPoint(view, gestureLastX, gestureLastY)
                sendPointerUnlocked(
                    4,
                    nx,
                    ny,
                    0,
                    0,
                    command.horizontalLines,
                    command.verticalLines,
                )
            }
            is TouchGestureCommand.Zoom -> {
                streamZoom.applyScale(command.factor, command.focusX, command.focusY, view.width, view.height)
                applyZoomToSurfaces()
            }
        }
    }

    /** Apply zoom and keep the remote overlay hidden while its coordinates differ. */
    private fun applyZoomToSurfaces() {
        val surfaces = streamSurfaces ?: return
        val split = surfaces.right != null
        surfaces.left?.let {
            if (split) streamZoom.applyToTile(it, leftTile = true) else streamZoom.applyToView(it)
        }
        surfaces.right?.let { streamZoom.applyToTile(it, leftTile = false) }
        if (streamZoom.isZoomed) hideRemoteCursorOverlay() else applyCursorPresentation()
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

    /**
     * 하드웨어 자판의 US 기준 글자 — kind-4 물리 키 경로가 그대로 맞는 경우를
     * 가려내는 기준선. 매핑이 없는 키(기능키 등)는 null.
     */
    private val usCharByKeyCode: Map<Int, Int> = buildMap {
        for (offset in 0..25) put(KeyEvent.KEYCODE_A + offset, 'A'.code + offset)
        for (offset in 0..9) put(KeyEvent.KEYCODE_0 + offset, '0'.code + offset)
        put(KeyEvent.KEYCODE_COMMA, ','.code)
        put(KeyEvent.KEYCODE_PERIOD, '.'.code)
        put(KeyEvent.KEYCODE_MINUS, '-'.code)
        put(KeyEvent.KEYCODE_EQUALS, '='.code)
        put(KeyEvent.KEYCODE_LEFT_BRACKET, '['.code)
        put(KeyEvent.KEYCODE_RIGHT_BRACKET, ']'.code)
        put(KeyEvent.KEYCODE_BACKSLASH, '\\'.code)
        put(KeyEvent.KEYCODE_SEMICOLON, ';'.code)
        put(KeyEvent.KEYCODE_APOSTROPHE, '\''.code)
        put(KeyEvent.KEYCODE_SLASH, '/'.code)
        put(KeyEvent.KEYCODE_GRAVE, '`'.code)
        put(KeyEvent.KEYCODE_SPACE, ' '.code)
    }

    /**
     * 하드웨어 글쇠 중 "US 배치가 아닌 문자"가 나오는 키는 kind-6 텍스트로
     * 보낸다(예: 태블릿이 한국어 하드웨어 자판이면 A 키가 'ㅁ'로 온다).
     * 문자를 그대로 타이핑하는 것이 Mac 입력 소스와 무관하게 정확하다.
     * US 기준 글자와 같은 키는 기존 kind-4 물리 키 경로를 유지한다 —
     * 키 홀드(게임 이동 등)·수식어 조합·반복 의미가 살아 있어야 하기 때문.
     * Ctrl/Alt/Meta/Sym/Function 조합과 Shift 조합은 항상 kind-4다.
     */
    private fun nonUsPrintableChar(event: KeyEvent): Int? {
        if (event.action != KeyEvent.ACTION_DOWN) return null
        val meta = event.metaState
        val allowedMeta = KeyEvent.META_SHIFT_ON or
            KeyEvent.META_SHIFT_LEFT_ON or KeyEvent.META_SHIFT_RIGHT_ON or
            KeyEvent.META_CAPS_LOCK_ON
        if (meta and allowedMeta.inv() != 0) return null
        if (meta and (KeyEvent.META_SHIFT_ON or
                KeyEvent.META_SHIFT_LEFT_ON or KeyEvent.META_SHIFT_RIGHT_ON) != 0
        ) {
            return null
        }
        val char = event.getUnicodeChar()
        // 제어 문자(Enter·Tab·Backspace 등)와 C1 제어는 문자가 아니라 키다.
        if (char < 0x20 || char == 0x7F || char in 0x80..0x9F) return null
        val baseline = usCharByKeyCode[event.keyCode] ?: return null
        // Android returns lowercase ASCII without Shift, while the physical
        // key labels above are uppercase. Both represent the same US key;
        // routing lowercase through text would synthesize an immediate UP.
        val keyLabel = if (char in 'a'.code..'z'.code) char - ('a'.code - 'A'.code) else char
        return if (keyLabel != baseline) char else null
    }

    /** DOWN이 텍스트 경로로 간 키의 UP — 호스트가 이미 down/up을 쳤으니 조용히 소비한다. */
    private val textPathDownKeyCodes = HashSet<Int>()

    private var closeConfirmation: android.app.AlertDialog? = null
    private var windowCloseNotified = false

    private fun notifyWindowClosed() {
        if (windowCloseNotified || isChangingConfigurations) return
        windowCloseNotified = true
        StreamLauncherModule.emitWindowClosed(instanceId, ownershipGeneration, port)
    }

    private fun confirmWindowClose() {
        if (isFinishing || isDestroyed || closeConfirmation?.isShowing == true) return
        ViewerNative.releaseInput(instanceId)
        android.util.Log.i("LeftcarStream", "window retained: Back confirmation port=$port")
        closeConfirmation = android.app.AlertDialog.Builder(this)
            .setTitle(ViewerStrings.closeWindowTitle)
            .setMessage(ViewerStrings.closeWindowMessage)
            .setNegativeButton(ViewerStrings.continueViewing) { _, _ -> }
            .setPositiveButton(ViewerStrings.closeWindow) { _, _ ->
                android.util.Log.i("LeftcarStream", "window close confirmed port=$port")
                notifyWindowClosed()
                finish()
            }
            .create().also { dialog ->
                dialog.setOnDismissListener { closeConfirmation = null }
                dialog.show()
                dialog.getButton(android.content.DialogInterface.BUTTON_NEGATIVE).requestFocus()
            }
    }

    override fun dispatchTouchEvent(event: MotionEvent): Boolean {
        // 잠금 배너 탭(2026-09-21): HUD 배너 팝업은 터치를 받지 않으므로
        // 배지 영역 히트 테스트를 여기서 대행한다. 요청을 보내면 탭을 소비해
        // 스트림 입력(원격 클릭)으로 새지 않게 한다.
        if (event.action == MotionEvent.ACTION_DOWN &&
            hud?.consumeInputRequestTap(event.x, event.y) == true
        ) {
            StreamLauncherModule.emitInputEnableRequested(port)
            hud?.onInputRequestSent()
            return true
        }
        return super.dispatchTouchEvent(event)
    }

    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        StreamPointerDiagnostics.recordLanguageKey(event)
        // Read actual IME state before the next letter joins the reliable queue.
        val languageSynced = inputLanguageMonitor.refresh()
        if (languageSynced && event.keyCode == KeyEvent.KEYCODE_LANGUAGE_SWITCH) {
            if (event.action == KeyEvent.ACTION_UP) {
                // Android가 소비한 토글의 subtype 변경은 observer보다 늦게
                // 관측될 수 있다 — 다음 글자가 이전 언어로 나가기 전에
                // kind-7이 먼저 나가도록 짧은 재확인을 심는다.
                inputLanguageMonitor.refreshSoon()
            }
            return super.dispatchKeyEvent(event)
        }
        // Mouse navigation can also arrive as KEYCODE_BACK. Consuming both
        // halves prevents an ordinary pointer action from closing this task.
        val isMouseSource = event.isFromSource(InputDevice.SOURCE_MOUSE) ||
            (event.source and InputDevice.SOURCE_MOUSE != 0) ||
            (event.device?.let { (it.sources and InputDevice.SOURCE_MOUSE) != 0 } == true)
        if (event.keyCode == KeyEvent.KEYCODE_BACK && isMouseSource) {
            if (event.action == KeyEvent.ACTION_UP) {
                android.util.Log.i("LeftcarStream", "window retained: mouse Back port=$port")
            }
            return true
        }
        if (!isRemoteKey(event.keyCode)) return super.dispatchKeyEvent(event)
        if (event.action != KeyEvent.ACTION_DOWN && event.action != KeyEvent.ACTION_UP) {
            return super.dispatchKeyEvent(event)
        }
        val down = event.action == KeyEvent.ACTION_DOWN
        if (!down && event.keyCode == KeyEvent.KEYCODE_ESCAPE && consumeEscapeUp) {
            consumeEscapeUp = false
            return true
        }
        when (inputOwnership.routePhysicalKey(event.keyCode, down)) {
            // KeyBridge 연동이 없으면 물리 키보드는 언제나 Mac으로 간다 —
            // 태블릿 하드웨어 키보드가 주 사용 흐름이다. 소유권에 따른 로컬
            // 소비는 KeyBridge가 실제 연결된 환경에서만 의미가 있다.
            KeyRoute.LOCAL_ANDROID ->
                if (keyBridgeInput != null) return super.dispatchKeyEvent(event)
            KeyRoute.RELEASE_REMOTE -> {
                if (down) consumeEscapeUp = true
                forceReleaseInput(InputOwnershipEvent.Escape)
                return true
            }
            KeyRoute.REMOTE_MAC -> Unit
        }
        if (remoteInputLocked()) {
            forceReleaseInput(InputOwnershipEvent.HostInputDisabled)
            return true
        }
        if (down) {
            hud?.revealInput()
            hud?.revealStats()
            val char = nonUsPrintableChar(event)
            if (char != null) {
                sendTextUnlocked(String(Character.toChars(char)))
                textPathDownKeyCodes.add(event.keyCode)
                return true
            }
        } else if (textPathDownKeyCodes.remove(event.keyCode)) {
            return true
        }
        val result = sendKeyUnlocked(
            event.keyCode,
            event.scanCode,
            event.metaState,
            down,
            event.repeatCount,
        )
        return result || super.dispatchKeyEvent(event)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // 이 창은 소프트 입력을 절대 쓰지 않는다(하드웨어 키보드 전용 제품
        // 정책). 하드웨어 키보드 연결/해제 때 시스템이 소프트 IME를 띄우려
        // 들면 XR은 이 창을 stopped로 숨겨 surface를 파괴한다(검정화면).
        window.setSoftInputMode(
            android.view.WindowManager.LayoutParams.SOFT_INPUT_STATE_ALWAYS_HIDDEN or
                android.view.WindowManager.LayoutParams.SOFT_INPUT_ADJUST_NOTHING,
        )
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() = confirmWindowClose()
        })
        // Saved state holds the effective configuration, including controls
        // received since the original start intent. Never restore a toggle as
        // a fresh stream launch.
        savedInstanceState?.getBundle("effectiveStreamConfiguration")?.let { effective ->
            setIntent(Intent(intent).replaceExtras(effective))
        }
        instanceId = intent?.getStringExtra("instance")
            ?: savedInstanceState?.getString("instance")
            ?: "instance-${System.nanoTime()}"
        host = intent?.getStringExtra("host") ?: savedInstanceState?.getString("host") ?: ""
        port = intent?.getIntExtra("port", 5000) ?: 5000
        fps = (intent?.getIntExtra("fps", 60) ?: 60).coerceIn(1, 90)
        sourceWidth = intent?.getIntExtra("width", 1920) ?: 1920
        sourceHeight = intent?.getIntExtra("height", 1080) ?: 1080
        ownershipGeneration = intent?.getLongExtra("ownershipGeneration", 0L) ?: 0L

        splitVertical = intent?.getBooleanExtra("splitVertical", false) ?: false
        splitDecoderName = intent?.getStringExtra("splitDecoderName") ?: ""
        val surfaces = createStreamSurfaces(
            this,
            sourceWidth,
            sourceHeight,
            splitVertical,
            this,
            { view, event -> forwardPointer(event, view) },
            { view, event -> forwardCapturedPointer(event, view) },
        )
        streamSurfaces = surfaces
        surfaceLifecycle.hierarchySwapped(surfaces.holders)
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
        keyBridgeInput = KeyBridgeInputAdapter.create(this).also { it.prepare() }
        localAudioEnabled = intent?.getBooleanExtra("localAudio", true) ?: true
        opusAudioRequested = intent?.getBooleanExtra("opusAudio", false) ?: false
        balancedPresentation = savedInstanceState?.takeIf { it.containsKey(KEY_BALANCED_PRESENTATION) }
            ?.getBoolean(KEY_BALANCED_PRESENTATION)
            ?: (intent?.getBooleanExtra(KEY_BALANCED_PRESENTATION, false) ?: false)
        presentationSmooth = savedInstanceState?.takeIf { it.containsKey(KEY_PRESENTATION_SMOOTH) }
            ?.getBoolean(KEY_PRESENTATION_SMOOTH)
            ?: (intent?.getBooleanExtra(KEY_PRESENTATION_SMOOTH, true) ?: true)
        // 프레임 스무딩도 vsync 시계를 필요로 한다 — 전역 스위치와 동기화.
        ViewerNative.setPresentationSmooth(presentationSmooth)
        // JS가 전달한 언어가 있으면 저장해 두고, 창 재생성 시에도 유지한다.
        intent?.getStringExtra("language")?.let { stored ->
            ViewerStrings.applyLanguage(stored)
            getSharedPreferences("leftcar_viewer", MODE_PRIVATE)
                .edit().putString(ViewerStrings.PREF_LANGUAGE, stored).apply()
        } ?: ViewerStrings.applyLanguage(
            getSharedPreferences("leftcar_viewer", MODE_PRIVATE)
                .getString(ViewerStrings.PREF_LANGUAGE, null),
        )
        val displayName = intent?.getStringExtra("displayName")?.takeIf { it.isNotBlank() } ?: ViewerStrings.displayFallback
        title = displayName
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.LOLLIPOP) {
            setTaskDescription(android.app.ActivityManager.TaskDescription(displayName))
        }
        showFps = intent?.getBooleanExtra("showFps", false) ?: false
        hud = StreamHudController(
            this,
            instanceId,
            fps,
            showFps,
            ::handleTermination,
            ::markRenderHealthy,
            onInputStatusChanged = { status ->
                if (status == 0) forceReleaseInput(InputOwnershipEvent.HostInputDisabled)
            },
        )
        hud?.show()
        // 첫 레이아웃 이후 저장되지 않은 크기(저장값 없음 포함)면 칩을 보인다.
        attachTextLens()
        showGestureHint()
        surfaces.requestFocus()
        hideSystemBars()
        acquireNetworkLocks()
        // Register before allocating JNI state. A closed/stale restored intent
        // may finish this Activity, but cannot resurrect a released native stream.
        if (!StreamLauncherModule.registerStreamActivity(instanceId, ownershipGeneration, this)) return
        nativeState = ViewerNative.start()
        // Host audio is a passive plane: start draining with the renderer and
        // keep running across surface transitions. Rebinds clear the native
        // ring via the LCH1 challenge, so a replacement session never plays
        // stale chunks.
        audioPlayer = StreamAudioPlayer(instanceId, { bytes -> ViewerNative.pollAudioOwned(nativeState, instanceId, bytes) },
            onCodecFallback = { ViewerNative.setAudioOwned(nativeState, instanceId, localAudioEnabled, false) }, signaledPoll = true).also { if (localAudioEnabled) it.start() }
        lifecycleEvent(1) // ACTIVITY_CREATE
        applyXrPreferredAspectRatio(force = true)
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putBoolean(KEY_BALANCED_PRESENTATION, balancedPresentation)
        outState.putBoolean(KEY_PRESENTATION_SMOOTH, presentationSmooth)
        outState.putBundle("effectiveStreamConfiguration", intent.extras?.let(::Bundle))
    }

    override fun onNewIntent(newIntent: Intent) {
        super.onNewIntent(newIntent)
        val nextHost = newIntent.getStringExtra("host") ?: host
        val nextPort = newIntent.getIntExtra("port", port)
        val nextFps = newIntent.getIntExtra("fps", fps).coerceIn(1, 90)
        val nextShowFps = newIntent.getBooleanExtra("showFps", showFps)
        val nextBalanced = newIntent.getBooleanExtra("balancedPresentation", balancedPresentation)
        val nextSmooth = newIntent.getBooleanExtra("presentationSmooth", presentationSmooth)
        val nextLocalCursor = newIntent.getBooleanExtra("localCursor", localCursorEnabled)
        val nextLocalAudio = newIntent.getBooleanExtra("localAudio", localAudioEnabled)
        val nextOpusAudio = newIntent.getBooleanExtra("opusAudio", opusAudioRequested)
        val nextWidth = newIntent.getIntExtra("width", sourceWidth)
        val nextHeight = newIntent.getIntExtra("height", sourceHeight)
        val nextSplitVertical = newIntent.getBooleanExtra("splitVertical", splitVertical)
        val reconnectRequested = newIntent.getBooleanExtra("reconnect", false)
        val sourceRatioChanged = !sameAspectRatio(nextWidth, nextHeight, sourceWidth, sourceHeight)
        val togglesOnly = (nextLocalCursor != localCursorEnabled ||
            nextLocalAudio != localAudioEnabled || nextOpusAudio != opusAudioRequested || nextBalanced != balancedPresentation ||
            nextSmooth != presentationSmooth) && !reconnectRequested &&
            nextHost == host && nextPort == port && nextFps == fps &&
            nextWidth == sourceWidth && nextHeight == sourceHeight &&
            nextSplitVertical == splitVertical && nextShowFps == showFps
        val streamConfigurationChanged =
            nextHost != host || nextPort != port || nextFps != fps ||
                nextWidth != sourceWidth || nextHeight != sourceHeight ||
                nextSplitVertical != splitVertical || nextShowFps != showFps ||
                nextLocalAudio != localAudioEnabled || nextOpusAudio != opusAudioRequested || nextBalanced != balancedPresentation

        if (newIntent.hasExtra("ownershipGeneration")) {
            val nextGeneration = newIntent.getLongExtra("ownershipGeneration", ownershipGeneration)
            if (!StreamLauncherModule.registerStreamActivity(instanceId, nextGeneration, this)) return
            ownershipGeneration = nextGeneration
        }
        // Preserve start configuration through partial controls. Keep the
        // current invocation's reconnect decision above separate from the
        // saved effective data so a previous start cannot turn a toggle into
        // a renderer restart.
        setIntent(Intent(intent).apply {
            newIntent.extras?.let { putExtras(it) }
            putExtra("reconnect", false)
        })
        if (togglesOnly) {
            localCursorEnabled = nextLocalCursor
            subscribeCursorStream()
            balancedPresentation = nextBalanced
            presentationSmooth = nextSmooth
            ViewerNative.setPresentationSmooth(nextSmooth)
            localAudioEnabled = nextLocalAudio
            opusAudioRequested = nextOpusAudio
            syncAudioStream()
            return
        }
        if (streamConfigurationChanged || reconnectRequested) {
            forceReleaseInput(InputOwnershipEvent.Disconnected)
            splitNeedsPreparation = false
            terminationHandled = false
            recoveryFallbackEmitted = false
            recoveryRetryPolicy.reset()
            host = nextHost
            port = nextPort
            fps = nextFps
            showFps = nextShowFps
            balancedPresentation = nextBalanced
            presentationSmooth = nextSmooth
            ViewerNative.setPresentationSmooth(nextSmooth)
            localAudioEnabled = nextLocalAudio
            opusAudioRequested = nextOpusAudio
            sourceWidth = nextWidth
            sourceHeight = nextHeight
            splitVertical = nextSplitVertical
            splitDecoderName = newIntent.getStringExtra("splitDecoderName") ?: splitDecoderName
            streamSurfaces?.updateVideoSize(sourceWidth, sourceHeight)
            if (sourceRatioChanged) {
                xrPreferredRatio = null
                applyXrPreferredAspectRatio(force = true)
            }
            when (
                decideSurfaceTransition(
                    nextSplitVertical = splitVertical,
                    hierarchyBuiltForSplit = streamSurfaces?.right != null,
                    leftSurfaceValid = streamSurfaces?.left?.holder?.surface?.isValid == true,
                )
            ) {
                StreamSurfaceTransition.REBIND_IN_PLACE -> {
                    val result = rebindOnSameSurface()
                    if (result != 0) {
                        // Leave the Activity and Surface visible. The
                        // controller's bounded retry can deliver another
                        // intent to this same instance without opening a
                        // second window.
                        android.util.Log.w(
                            "LeftcarStream",
                            "same-window rebind failed result=$result; retaining Activity",
                        )
                    }
                }
                // A live-window mode change cannot swap the renderer in
                // place: the hierarchy still only contains the previous
                // mode's SurfaceView(s), so no SurfaceHolder callback could
                // ever re-run the attach path. Rebuild it (4K splitVertical
                // promotion, demotion, or a dead Surface) so fresh
                // surfaceCreated callbacks drive attachStableSurfaces.
                StreamSurfaceTransition.REBUILD_SURFACES -> rebuildStreamSurfaces()
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
        if (!isFinishing) StreamConnectionOwners.acquire(this, connectionOwner)
        activityStarted = true
        syncPresentation()
        lifecycleEvent(2) // ACTIVITY_START
    }

    override fun onResume() {
        super.onResume()
        inputLanguageMonitor.start()
        hud?.show()
        lifecycleEvent(3) // ACTIVITY_RESUME
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        inputLanguageMonitor.reset()
        if (hasFocus) inputLanguageMonitor.refresh()
        lifecycleEvent(if (hasFocus) 4 else 5) // FOCUS_GAIN / FOCUS_LOSS
        if (hasFocus) {
            streamSurfaces?.requestFocus()
            window.decorView.post { hideSystemBars() }
        } else {
            hideTabletCursor()
            // 창 포커스를 잃으면 시스템이 IME를 닫으므로 요청 상태도 원점으로.
            keyboardRequested = false
            forceReleaseInput(InputOwnershipEvent.FocusLost)
        }
    }

    override fun onPointerCaptureChanged(hasCapture: Boolean) {
        super.onPointerCaptureChanged(hasCapture)
        applyInputOwnershipEffects(
            inputOwnership.on(InputOwnershipEvent.PointerCaptureChanged(hasCapture)),
        )
    }

    /**
     * Rebuild the Surface view hierarchy for the current mode on this same
     * Activity window. onCreate builds exactly one hierarchy; a live-window
     * mode change (4K splitVertical promotion, demotion, or a Surface that
     * can no longer host a rebind) needs fresh SurfaceViews so the normal
     * SurfaceHolder.Callback traffic re-runs attachStableSurfaces — including
     * attachSplitSurfaces, which no other path can reach here.
     */
    private fun rebuildStreamSurfaces() {
        if (released || isFinishing || isDestroyed) return
        // Retire this hierarchy's callback even if replacement attach is delayed
        // or fails. Only successful attach may start the next clock epoch.
        displayClock.stop()
        ViewerNative.displayFrame(nativeState, instanceId, balancedPresentation || presentationSmooth, -1, 0, 0)
        // Stop the renderer still bound to the replaced hierarchy BEFORE the
        // swap: the old SurfaceViews' late destroys then own nothing, so their
        // arrival order — interleaved with the new holders' creates or after
        // the new attach — can never detach the fresh renderer (device
        // session-3 failure mode). This split detach intentionally stops
        // silently; it never suspends the receivers.
        val oldRendererLive = surfaceLifecycle.isAttached
        if (oldRendererLive) {
            val res = ViewerNative.detachSurface(nativeState, instanceId)
            android.util.Log.i(
                "LeftcarStream",
                "pre-swap detach of replaced renderer returned $res",
            )
        }
        val next = createStreamSurfaces(
            this,
            sourceWidth,
            sourceHeight,
            splitVertical,
            this,
            { view, event -> forwardPointer(event, view) },
            { view, event -> forwardCapturedPointer(event, view) },
        )
        streamSurfaces = next
        // 새 서피스는 변환 없이 태어나므로 줌 상태도 원점으로 되돌린다 —
        // 남은 줌으로 두면 toContent가 낡은 변환으로 탭을 역산해 원격 입력이
        // 엉뚱한 곳에 찍힌다. 커서는 attach 직후 소유권에 맞춰 복원된다.
        streamZoom.reset()
        // Retired holders are forgotten here; their destroys are inert.
        surfaceLifecycle.hierarchySwapped(next.holders)
        cancelPendingSurfaceAttach()
        hud?.showRebindIndicator(ViewerStrings.rebindPreparing)
        setContentView(next.root)
        next.requestFocus()
        window.decorView.post { hideSystemBars() }
        android.util.Log.i(
            "LeftcarStream",
            "rebuilt stream surfaces split=$splitVertical " +
                "source=${sourceWidth}x$sourceHeight port=$port; " +
                "awaiting SurfaceHolder callbacks",
        )
    }

    private fun cancelPendingSurfaceAttach() {
        pendingSurfaceAttach?.let(surfaceHandler::removeCallbacks)
        pendingSurfaceAttach = null
    }

    private fun attachStableSurfaces(scheduledGeneration: Int) {
        pendingSurfaceAttach = null
        val surfaces = streamSurfaces ?: return
        if (
            released || isFinishing || isDestroyed ||
            !surfaceLifecycle.canAttachStableSurfaces(scheduledGeneration) ||
            !surfaces.allValid() ||
            surfaces.holders.size != surfaceLifecycle.trackedHolderCount
        ) {
            return
        }
        if (splitVertical && splitNeedsPreparation) {
            hud?.showRebindIndicator(ViewerStrings.rebindReconnectingControl)
            scheduleRenderRecovery()
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
            ViewerNative.attachSplitSurfacesWithPresentation(
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
                balancedPresentation,
            )
        } else {
            ViewerNative.attachSurfacePortWithPresentation(
                nativeState,
                instanceId,
                surfaces.left.holder.surface,
                port,
                host,
                sourceWidth,
                sourceHeight,
                fps,
                balancedPresentation,
            )
        }
        surfaceLifecycle.confirmAttached(res == 0)
        inputLanguageMonitor.reset()
        if (res == 0) {
            // Native attach clears a retained reason for this logical instance
            // before creating the new renderer. Only then may the HUD consume
            // a fresh termination reason.
            hud?.resetTerminationPolling()
            hud?.armTerminationPolling()
            hud?.clearRebindIndicator()
            subscribeCursorStream()
            syncAudioStream()
            armAttachFlowWatchdog()
        } else if (splitVertical) {
            // A failed split attach can already have consumed one or both
            // receivers. Repeating JNI attach cannot recreate their keys.
            splitNeedsPreparation = true
            hud?.showRebindIndicator(ViewerStrings.rebindReconnectingControl)
            scheduleRenderRecovery()
        }
        android.util.Log.i(
            "LeftcarStream",
            "stable Surface attach returned $res after $surfaceChangeCount geometry changes, " +
                "host=$host, source=${sourceWidth}x${sourceHeight}, fps=$fps, split=$splitVertical",
        )
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        val tracked = surfaceLifecycle.tracks(holder)
        val scheduled = surfaceLifecycle.onSurfaceCreated(holder)
        if (!tracked) {
            // A replaced hierarchy's late create must not disturb the live
            // hierarchy's debounce attach or activity bookkeeping.
            return
        }
        surfaceChangeCount = 0
        cancelPendingSurfaceAttach()
        android.util.Log.i(
            "LeftcarStream",
            "surfaceCreated: debounce generation=${surfaceLifecycle.currentGeneration} " +
                "instanceId=$instanceId port=$port",
        )
        if (scheduled != null) {
            val attach = Runnable { attachStableSurfaces(scheduled) }
            pendingSurfaceAttach = attach
            surfaceHandler.postDelayed(attach, SURFACE_ATTACH_DEBOUNCE_MS)
        }
        lifecycleEvent(6) // SURFACE_CREATE
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        if (!surfaceLifecycle.tracks(holder)) return
        surfaceChangeCount += 1
        if (surfaceLifecycle.shouldUpdateGeometry(holder) && width > 0 && height > 0) {
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
        val tracked = surfaceLifecycle.tracks(holder)
        val stop = surfaceLifecycle.onSurfaceDestroyed(holder, isFinishing)
        if (stop == StreamSurfaceStop.NONE && !tracked) {
            // A replaced hierarchy's late destroy owns nothing: no live
            // renderer, no pending attach, no activity bookkeeping.
            android.util.Log.i(
                "LeftcarStream",
                "surfaceDestroyed: retired holder ignored instanceId=$instanceId",
            )
            return
        }
        cancelPendingSurfaceAttach()
        cancelAttachFlowWatchdog()
        displayClock.stop()
        ViewerNative.displayFrame(nativeState, instanceId, balancedPresentation || presentationSmooth, -1, 0, 0)
        android.util.Log.i(
            "LeftcarStream",
            "surfaceDestroyed: stop=$stop generation=${surfaceLifecycle.currentGeneration} " +
                "geometryChanges=$surfaceChangeCount instanceId=$instanceId",
        )
        lifecycleEvent(8) // SURFACE_DESTROY
        forceReleaseInput(InputOwnershipEvent.SurfaceLost)
        // A final Surface loss defers release to onDestroy so lifecycle/state
        // callbacks finish before native memory is freed off the UI thread.
        // A retired holder consumes its stop flag without touching a replacement.
        when (stop) {
            StreamSurfaceStop.NONE -> {}
            StreamSurfaceStop.DETACH_RENDERER -> {
                if (splitVertical) splitNeedsPreparation = true
                val res = ViewerNative.detachSurface(nativeState, instanceId)
                android.util.Log.i(
                    "LeftcarStream",
                    "detachSurface returned $res; waiting for Surface recreation",
                )
            }
            StreamSurfaceStop.FINAL_RELEASE -> {
                released = true
                // Keep the owned native-window reference until onDestroy has
                // finished all state callbacks, then release off the UI thread.
                android.util.Log.i("LeftcarStream", "final release queued for onDestroy")
            }
        }
    }

    override fun onPause() {
        inputLanguageMonitor.stop()
        hideTabletCursor()
        forceReleaseInput(InputOwnershipEvent.FocusLost)
        lifecycleEvent(9) // ACTIVITY_PAUSE
        super.onPause()
    }

    override fun onStop() {
        activityStarted = false
        syncPresentation()
        lifecycleEvent(10) // ACTIVITY_STOP
        super.onStop()
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        android.util.Log.i("LeftcarStream", "onConfigurationChanged: orientation=${newConfig.orientation}")
        lifecycleEvent(11) // CONFIGURATION_CHANGE

    }

    private fun releaseOwnedNativeStream() {
        val releaseState = nativeState
        val releaseInstance = instanceId
        StreamLauncherModule.releaseStreamActivity(instanceId, ownershipGeneration, this) {
            releaseState == 0L || ViewerNative.release(releaseState, releaseInstance) == 0
        }
    }

    override fun onDestroy() {
        if (isFinishing) notifyWindowClosed()
        inputLanguageMonitor.stop()
        closeConfirmation?.dismiss()
        android.util.Log.i("LeftcarStream", "window destroyed port=$port finishing=$isFinishing configuration=$isChangingConfigurations")
        displayClock.stop()
        displayClockThread.quitSafely()
        android.util.Log.i(
            "LeftcarStream",
            "onDestroy: final release instanceId=$instanceId attached=${surfaceLifecycle.isAttached}",
        )
        if (!released) {
            lifecycleEvent(12) // TASK_REMOVE / final Activity destruction
            released = true
        }
        forceReleaseInput(InputOwnershipEvent.Disconnected)
        keyBridgeInput?.close()
        keyBridgeInput = null
        surfaceLifecycle.invalidate()
        cancelPendingSurfaceAttach()
        recoveryRetryRunnable?.let(recoveryHandler::removeCallbacks)
        recoveryRetryRunnable = null
        cancelRecoveryFlowWatchdog()
        cancelAttachFlowWatchdog()
        tabletCursorHandler.removeCallbacks(hideTabletCursorRunnable)
        gestureHandler.removeCallbacksAndMessages(null)
        cursorOverlay?.stop()
        cursorOverlay = null
        textLens?.let { (it.parent as? android.view.ViewGroup)?.removeView(it) }
        textLens = null
        audioPlayer?.stop()
        audioPlayer = null
        gestureHint?.dismiss()
        gestureHint = null
        hud?.stop()
        hud = null
        releaseNetworkLocks()
        StreamConnectionOwners.release(this, connectionOwner)
        // forceReleaseInput(Disconnected)가 이미 입력을 놓았다 — 커서 스트림
        // 구독만 끈다.
        ViewerNative.setCursorStream(instanceId, false)
        super.onDestroy()
        releaseOwnedNativeStream()
    }
}

/** Live-window stream intent transition decision (U3 single↔split fix). */
internal enum class StreamSurfaceTransition {
    /** Swap the renderer onto the existing single Surface. */
    REBIND_IN_PLACE,

    /** Rebuild the view hierarchy so fresh Surface callbacks re-run attach. */
    REBUILD_SURFACES,
}

/**
 * A same-mode single stream with a still-valid Surface swaps its renderer in
 * place; every other transition changes what the hierarchy must contain —
 * most importantly a 4K splitVertical promotion on a window created with a
 * single SurfaceView — so the hierarchy itself is rebuilt and the normal
 * SurfaceHolder.Callback attach path runs again.
 */
internal fun decideSurfaceTransition(
    nextSplitVertical: Boolean,
    hierarchyBuiltForSplit: Boolean,
    leftSurfaceValid: Boolean,
): StreamSurfaceTransition =
    if (!nextSplitVertical && !hierarchyBuiltForSplit && leftSurfaceValid) {
        StreamSurfaceTransition.REBIND_IN_PLACE
    } else {
        StreamSurfaceTransition.REBUILD_SURFACES
    }

/** Renderer stop policy produced by [StreamSurfaceLifecycleGate.onSurfaceDestroyed]. */
internal enum class StreamSurfaceStop {
    /** Nothing to stop; the holder never hosted a renderer. */
    NONE,

    /** Stop the renderer bound to the Surface, keep the Activity window alive. */
    DETACH_RENDERER,

    /** The Activity is finishing: release the renderer so the final BYE goes out. */
    FINAL_RELEASE,
}

/**
 * JVM-pure owner of the SurfaceHolder.Callback bookkeeping StreamActivity
 * mirrors 1:1: which holders belong to the live hierarchy, when the debounced
 * attach may run, and how a destroy stops its renderer.
 *
 * A live-window single↔split promotion replaces the view hierarchy. The
 * Activity stops the replaced hierarchy's renderer eagerly (detach before the
 * swap), so retired holders carry no stop duty at all: their late destroys —
 * in any arrival order, before or after the new attach — are fully inert and
 * can never detach a renderer the new hierarchy attached (device session-3
 * failure).
 */
internal class StreamSurfaceLifecycleGate<T : Any> {
    private var tracked: Set<T> = emptySet()
    private val created = LinkedHashSet<T>()
    private val stopped = mutableSetOf<T>()
    private var generation: Int = 0
    private var attached: Boolean = false

    val isAttached: Boolean get() = attached
    val currentGeneration: Int get() = generation
    val trackedHolderCount: Int get() = tracked.size

    fun tracks(holder: T): Boolean = holder in tracked

    /**
     * onCreate and onNewIntent rebuilds: [holders] now form the live
     * hierarchy. The caller detached any renderer bound to the replaced
     * hierarchy before the swap, so the attached state ends here and retired
     * holders are forgotten outright — also keeping [stopped] bounded on
     * long-lived Activities.
     */
    fun hierarchySwapped(holders: List<T>) {
        tracked = holders.toSet()
        created.clear()
        stopped.clear()
        attached = false
        generation += 1
    }

    /**
     * SURFACE_CREATE for a live holder. Returns the generation to
     * debounce-schedule once every holder of the hierarchy has a Surface.
     */
    fun onSurfaceCreated(holder: T): Int? {
        if (holder !in tracked) return null
        created += holder
        stopped -= holder
        generation += 1
        if (tracked.isNotEmpty() && created.size == tracked.size) {
            return generation
        }
        return null
    }

    /** SURFACE_CHANGED feeds geometry only for the live, attached hierarchy. */
    fun shouldUpdateGeometry(holder: T): Boolean = attached && holder in tracked

    /** SURFACE_DESTROY: how the renderer bound to [holder] must be stopped. */
    fun onSurfaceDestroyed(holder: T, finishing: Boolean): StreamSurfaceStop {
        if (holder !in tracked) {
            // Retired hierarchy: its renderer was detached at swap time, so
            // this destroy owns nothing — never a stop, never a generation
            // bump, never a pending-attach cancellation.
            return StreamSurfaceStop.NONE
        }
        // The framework destroys a tracked holder's Surface at most once per
        // creation; treat any repeat as inert.
        if (holder in stopped) return StreamSurfaceStop.NONE
        stopped += holder
        val wasAttached = attached
        created -= holder
        generation += 1
        if (!wasAttached) return StreamSurfaceStop.NONE
        attached = false
        return if (finishing) StreamSurfaceStop.FINAL_RELEASE else StreamSurfaceStop.DETACH_RENDERER
    }

    /**
     * The debounced attach runnable: run only when the captured generation is
     * still current, every live holder has (re)created its Surface, and no
     * renderer is attached yet.
     */
    fun canAttachStableSurfaces(scheduledGeneration: Int): Boolean =
        !attached && scheduledGeneration == generation && created.size == tracked.size

    fun confirmAttached(success: Boolean) {
        attached = success
    }

    /** Final teardown: no further attach may run. */
    fun invalidate() {
        generation += 1
    }
}
