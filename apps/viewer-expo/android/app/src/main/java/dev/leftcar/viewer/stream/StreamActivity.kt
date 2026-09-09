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
import androidx.lifecycle.lifecycleScope
import androidx.xr.runtime.Session
import androidx.xr.runtime.SessionCreateSuccess
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.Job

class StreamActivity : ComponentActivity(), SurfaceHolder.Callback {
    companion object {
        private const val TABLET_CURSOR_IDLE_TIMEOUT_MS = 1_500L
        private const val SURFACE_ATTACH_DEBOUNCE_MS = 300L
        private const val KEY_XR_WINDOW_RATIO = "xrWindowRatio"
    }

    private var instanceId: String = ""
    private var host: String = ""
    private var port: Int = 5000
    private var fps: Int = 60
    private var showFps: Boolean = false
    private var sourceWidth: Int = 1920
    private var sourceHeight: Int = 1080
    private var splitVertical = false
    private var splitDecoderName = ""
    private var nativeState: Long = 0
    private var released = false
    private var streamSurfaces: StreamSurfaces? = null
    private val surfaceLifecycle = StreamSurfaceLifecycleGate<SurfaceHolder>()
    private val surfaceHandler = Handler(Looper.getMainLooper())
    private val recoveryHandler = Handler(Looper.getMainLooper())
    private val recoveryRetryPolicy = StreamRecoveryRetryPolicy()
    private var surfaceChangeCount = 0
    private var pendingSurfaceAttach: Runnable? = null
    private val tabletCursorHandler = Handler(Looper.getMainLooper())
    private val hideTabletCursorRunnable = Runnable {
        streamSurfaces?.left?.pointerIcon = PointerIcon.getSystemIcon(this, PointerIcon.TYPE_NULL)
        streamSurfaces?.right?.pointerIcon = PointerIcon.getSystemIcon(this, PointerIcon.TYPE_NULL)
    }
    private var hud: StreamHudController? = null
    private var gestureHint: GestureHintOverlay? = null
    private var cursorOverlay: CursorOverlayView? = null
    private var audioPlayer: StreamAudioPlayer? = null
    private var localCursorEnabled: Boolean = false
    private var localAudioEnabled: Boolean = true
    private var textLens: TextInputLensView? = null
    private var keyboardRequested = false
    private var terminationHandled = false
    private var recoveryRetryRunnable: Runnable? = null
    private var recoveryFallbackEmitted = false
    private var xrSession: Session? = null
    private var xrPreferredRatio: Float? = null
    /**
     * 사용자가 선택한 창 비율 오버라이드(0 = 미지정 → 소스 비율 사용).
     * 멤버 변수로 유지되어 config-change/rotation에서 그대로 살아남고,
     * 프로세스 사망 시에는 onSaveInstanceState로 복원된다.
     */
    private var xrWindowRatio: Float = 0f
    private var ownershipGeneration: Long = 0L
    private var xrRatioGeneration = 0L
    private var xrCreationInFlight: Job? = null

    private fun applyXrPreferredAspectRatio(force: Boolean = false, ratioOverride: Float? = null) {
        if (!packageManager.hasSystemFeature("android.software.xr.api.spatial")) return
        val sourceRatio =
            sourceWidth.toFloat().coerceAtLeast(1f) / sourceHeight.coerceAtLeast(1).toFloat()
        val ratio = ratioOverride ?: sourceRatio
        if (!force && xrPreferredRatio == ratio) return
        val generation = ++xrRatioGeneration
        val existing = xrSession
        if (existing != null) {
            runCatching { setXrRatio(existing, ratio) }
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
                // 생성 도중 더 새 비율 요청이 들어왔으면 최신 상태로 다시 적용.
                applyXrPreferredAspectRatio(force = true, ratioOverride = requestedRatioOverride())
                return@launch
            }
            runCatching { setXrRatio(created.session, ratio) }
                .onSuccess { xrPreferredRatio = ratio }
                .onFailure { android.util.Log.i("LeftcarStream", "XR preferred ratio rejected; keeping system panel size", it) }
        }
    }

    private fun setXrRatio(session: Session, ratio: Float) {
        // Keep the XR path optional at runtime: the AndroidX API is present in
        // the APK, while non-XR devices simply never create a Session.
        SpatialWindowBridge.setPreferredAspectRatio(session, this, ratio)
    }

    /**
     * 런타임 비율 프리셋 적용 진입점. 활성 Session이 있으면 즉시
     * setPreferredAspectRatio를 다시 호출하고, 없으면 값을 저장해 다음 Session
     * 생성 시 사용한다. applyXrPreferredAspectRatio가 아직 XR 기기 검사를
     * 수행하므로 비 XR 기기에서는 no-op으로 끝난다.
     */
    fun applyWindowAspectRatio(ratio: Float) {
        xrWindowRatio = normalizedAspectRatio(sourceWidth, sourceHeight, ratio)
        if (xrWindowRatio == xrPreferredRatio) return
        applyXrPreferredAspectRatio(ratioOverride = xrWindowRatio)
    }

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
                "port=$port source=${sourceWidth}x$sourceHeight fps=$fps",
        )
        if (result == 0) {
            hud?.onRebindFinished(true)
        } else {
            hud?.onRebindFinished(false)
            scheduleRenderRecovery()
        }
    }

    /**
     * Swap the live renderer onto the current Surface and geometry. Shared by
     * the render-recovery retry and the same-window stream intent: both own
     * the success bookkeeping (reset termination state, notify the HUD), so
     * the two call sites cannot drift.
     */
    private fun rebindOnSameSurface(): Int {
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
        surfaceLifecycle.confirmAttached(result == 0)
        if (result == 0) {
            terminationHandled = false
            recoveryFallbackEmitted = false
            recoveryRetryPolicy.reset()
            // A rebind builds a fresh renderer session, so the host opt-in
            // (LCDON) must ride again with the new control channel.
            enableCursorOverlay()
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

    /**
     * 화면 오른쪽 위 입력 배지처럼 커서도 스트림 창 밖 팝업 윈도우로 띄운다 —
     * 스트림 SurfaceView가 setZOrderOnTop으로 합성돼 창 안 어떤 뷰도 비디오 위를
     * 그릴 수 없다. attach·재바인드마다 LCDON을 보내 새 호스트 세션에 옵트인을
     * 다시 알린다. 사용자가 오버레이를 끄면 LCDOFF를 즉시 보내고, 세션 종료는
     * BYE가 최종 정리를 맡는다.
     */
    private fun enableCursorOverlay() {
        if (!localCursorEnabled) return
        ViewerNative.setCursorStream(instanceId, true)
        val overlay = cursorOverlay ?: CursorOverlayView(this, instanceId).also { view ->
            cursorOverlay = view
        }
        overlay.setVideoSize(sourceWidth, sourceHeight)
        overlay.start()
    }

    private fun disableCursorOverlay() {
        ViewerNative.setCursorStream(instanceId, false)
        cursorOverlay?.stop()
        cursorOverlay = null
    }

    /**
     * SNDON/SNDOFF는 멱등 커맨드라서 코어가 1초 주기로 재전송하므로 여기서는
     * 렌더러에 뷰어의 현재 선호만 저장하면 된다. attach·재바인드 직후 호출해
     * 새로 만들어진 세션에도 선호가 즉시 반영되게 한다.
     */
    private fun syncAudioStream() {
        ViewerNative.setAudioStream(instanceId, localAudioEnabled)
    }

    /**
     * 제스처 안내는 첫 스트림 창에서 자동으로 1회 보여 주고(force=false),
     * 이후에는 HUD 물음표 칩으로 다시 연다(force=true). 닫힐 때 "본 적
     * 있음" 플래그를 저장한다.
     */
    private fun showGestureHint(force: Boolean) {
        val prefs = getSharedPreferences("leftcar_viewer", MODE_PRIVATE)
        if (!force && prefs.getBoolean(GestureHintOverlay.PREF_SHOWN, false)) return
        gestureHint?.dismiss()
        gestureHint = GestureHintOverlay(this) {
            prefs.edit().putBoolean(GestureHintOverlay.PREF_SHOWN, true).apply()
        }.also { it.show() }
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
        val mapped = mapAspectFitPoint(x, y, view.width, view.height, videoWidth, sourceHeight)
        val nx = when {
            !split -> mapped.first
            view === streamSurfaces?.right -> 0.5f + mapped.first * 0.5f
            else -> mapped.first * 0.5f
        }
        return nx to mapped.second
    }

    private fun normalizedX(event: MotionEvent, view: View): Float =
        normalizedPoint(view, event.x, event.y).first

    private fun normalizedY(event: MotionEvent, view: View): Float =
        normalizedPoint(view, event.x, event.y).second

    /**
     * 호스트가 원격 입력을 잠근 동안(상태 0)은 터치·마우스·키보드 이벤트를
     * 전송 단계에 넣기 전에 조용히 버린다. 호스트도 자체 게이트에서 폐기하지만,
     * 뷰어가 먼저 끊어야 잠금 내내 이어지는 UDP 전송·재전송과 무선 전력 낭비가
     * 없어지고 입력 배지와 실제 동작이 일치한다. 상태를 아직 모를 때(-1)는
     * 보낸다 — 세션 시작 직후 자동 허용 상태가 도착하기 전 첫 입력을 막지
     * 않기 위해서다.
     */
    private fun remoteInputLocked(): Boolean = ViewerNative.inputStatus(instanceId) == 0

    /** 잠금 중에는 전송하지 않고, 이벤트는 로컬에서 소비한 것으로 처리한다. */
    private fun sendPointerUnlocked(
        action: Int,
        x: Float,
        y: Float,
        buttons: Int,
        actionButton: Int,
        horizontalScroll: Float,
        verticalScroll: Float,
    ): Boolean {
        if (remoteInputLocked()) return true
        return ViewerNative.sendPointer(
            instanceId,
            action,
            x,
            y,
            buttons,
            actionButton,
            horizontalScroll,
            verticalScroll,
        ) == 0
    }

    private fun sendKeyUnlocked(
        keyCode: Int,
        scanCode: Int,
        metaState: Int,
        down: Boolean,
        repeat: Int,
    ): Boolean {
        if (remoteInputLocked()) return true
        return ViewerNative.sendKey(instanceId, keyCode, scanCode, metaState, down, repeat) == 0
    }

    private fun sendTextUnlocked(text: String) {
        if (remoteInputLocked()) return
        ViewerNative.sendText(instanceId, text.toByteArray(Charsets.UTF_8))
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
        )
        val lens = TextInputLensView(this, relay).also { view ->
            view.onImeVisibilityChanged = { visible ->
                keyboardRequested = visible
                hud?.setKeyboardChipActive(visible)
            }
            textLens = view
        }
        (window.decorView as android.view.ViewGroup).addView(
            lens,
            android.widget.FrameLayout.LayoutParams(1, 1),
        )
    }

    /**
     * HUD "ABC" 칩의 토글. 열 때는 포커스가 렌즈로 넘어가고(하드웨어 키는
     * Activity dispatchKeyEvent가 여전히 Mac으로 포워딩한다), 닫으면 포커스를
     * 스트림 Surface로 돌려 놓는다. [keyboardRequested]는 우리가 요청한 상태고,
     * 렌즈의 insets 콜백(API 30+)이 뒤로가기 닫기 같은 시스템 주도 변화로
     * 어긋난 상태를 실측값으로 되돌린다.
     */
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
            hud?.setKeyboardChipActive(false)
        }
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
        // Touchscreen input goes through the gesture machine (tap, drag,
        // two-finger scroll, long-press right click); physical mice and
        // styluses keep the direct event mapping below.
        if (event.isFromSource(InputDevice.SOURCE_TOUCHSCREEN)) {
            return forwardTouchGesture(event, view)
        }
        // Touchscreen events never reach here (routed to the gesture machine
        // above), so only the stylus still counts as touch-like.
        val touchLike = event.isFromSource(InputDevice.SOURCE_STYLUS)
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
        return sendPointerUnlocked(
            action,
            normalizedX(event, view),
            normalizedY(event, view),
            buttons,
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
    private var longPressRunnable: Runnable? = null
    private var gestureLastX = 0f
    private var gestureLastY = 0f

    private fun forwardTouchGesture(event: MotionEvent, view: View): Boolean {
        updateTabletCursor(event, view)
        if (event.actionMasked == MotionEvent.ACTION_DOWN) {
            hud?.revealInput()
            hud?.revealStats()
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
                sendPointerUnlocked(
                    4,
                    normalizedPoint(view, gestureLastX, gestureLastY).first,
                    normalizedPoint(view, gestureLastX, gestureLastY).second,
                    0,
                    0,
                    command.horizontalLines,
                    command.verticalLines,
                )
            }
        }
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
        val result = sendKeyUnlocked(
            event.keyCode,
            event.scanCode,
            event.metaState,
            event.action == KeyEvent.ACTION_DOWN,
            event.repeatCount,
        )
        return result || super.dispatchKeyEvent(event)
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
        ownershipGeneration = intent?.getLongExtra("ownershipGeneration", 0L) ?: 0L
        // Process-death 이후에도 사용자가 고른 창 비율을 복원한다.
        xrWindowRatio =
            savedInstanceState?.getFloat(KEY_XR_WINDOW_RATIO, xrWindowRatio) ?: xrWindowRatio

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
        localCursorEnabled = intent?.getBooleanExtra("localCursor", true) ?: true
        localAudioEnabled = intent?.getBooleanExtra("localAudio", true) ?: true
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
        ).also { hud ->
            hud.onGestureHelpTapped = { showGestureHint(true) }
            hud.onKeyboardToggle = { toggleSoftKeyboard() }
        }
        hud?.show()
        attachTextLens()
        showGestureHint(false)
        surfaces.requestFocus()
        hideSystemBars()
        acquireNetworkLocks()
        nativeState = ViewerNative.start()
        // Host audio is a passive plane: start draining with the renderer and
        // keep running across surface transitions. Rebinds clear the native
        // ring via the LCH1 challenge, so a replacement session never plays
        // stale chunks.
        audioPlayer = StreamAudioPlayer(instanceId).also { it.start() }
        lifecycleEvent(1) // ACTIVITY_CREATE
        applyXrPreferredAspectRatio(force = true, ratioOverride = requestedRatioOverride())
    }

    /** The user-chosen window ratio, when one differs from the source ratio. */
    private fun requestedRatioOverride(): Float? =
        xrWindowRatio.takeIf { it > 0f }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putFloat(KEY_XR_WINDOW_RATIO, xrWindowRatio)
    }

    override fun onNewIntent(newIntent: Intent) {
        super.onNewIntent(newIntent)
        val nextHost = newIntent.getStringExtra("host") ?: host
        val nextPort = newIntent.getIntExtra("port", port)
        val nextFps = newIntent.getIntExtra("fps", fps).coerceIn(1, 90)
        val nextShowFps = newIntent.getBooleanExtra("showFps", showFps)
        val nextLocalCursor = newIntent.getBooleanExtra("localCursor", localCursorEnabled)
        val nextLocalAudio = newIntent.getBooleanExtra("localAudio", localAudioEnabled)
        val nextWidth = newIntent.getIntExtra("width", sourceWidth)
        val nextHeight = newIntent.getIntExtra("height", sourceHeight)
        val nextSplitVertical = newIntent.getBooleanExtra("splitVertical", splitVertical)
        val reconnectRequested = newIntent.getBooleanExtra("reconnect", false)
        val sourceRatioChanged = !sameAspectRatio(nextWidth, nextHeight, sourceWidth, sourceHeight)
        val ratioChangeRequested = newIntent.hasExtra(KEY_XR_WINDOW_RATIO)
        val togglesOnly = (nextLocalCursor != localCursorEnabled ||
            nextLocalAudio != localAudioEnabled) &&
            nextHost == host && nextPort == port && nextFps == fps &&
            nextWidth == sourceWidth && nextHeight == sourceHeight &&
            nextSplitVertical == splitVertical && nextShowFps == showFps
        val streamConfigurationChanged =
            nextHost != host || nextPort != port || nextFps != fps ||
                nextWidth != sourceWidth || nextHeight != sourceHeight ||
                nextSplitVertical != splitVertical || nextShowFps != showFps ||
                nextLocalCursor != localCursorEnabled || nextLocalAudio != localAudioEnabled

        setIntent(newIntent)
        if (newIntent.hasExtra("ownershipGeneration")) {
            ownershipGeneration = newIntent.getLongExtra("ownershipGeneration", ownershipGeneration)
        }
        if (ratioChangeRequested) {
            // 비율 프리셋만 바꾸는 호출: 소스 해상도는 그대로 둔다. 값은
            // 다음 setIntent 이전 스트림 재구성에도 유지된다.
            applyWindowAspectRatio(newIntent.getFloatExtra(KEY_XR_WINDOW_RATIO, xrWindowRatio))
        }
        if (togglesOnly) {
            localCursorEnabled = nextLocalCursor
            if (localCursorEnabled) enableCursorOverlay() else disableCursorOverlay()
            localAudioEnabled = nextLocalAudio
            syncAudioStream()
            return
        }
        if (streamConfigurationChanged || reconnectRequested) {
            host = nextHost
            port = nextPort
            fps = nextFps
            showFps = nextShowFps
            localCursorEnabled = nextLocalCursor
            localAudioEnabled = nextLocalAudio
            sourceWidth = nextWidth
            sourceHeight = nextHeight
            splitVertical = nextSplitVertical
            splitDecoderName = newIntent.getStringExtra("splitDecoderName") ?: splitDecoderName
            streamSurfaces?.updateVideoSize(sourceWidth, sourceHeight)
            if (sourceRatioChanged) {
                xrPreferredRatio = null
                applyXrPreferredAspectRatio(force = true, ratioOverride = requestedRatioOverride())
            }
            if (!localCursorEnabled) disableCursorOverlay()
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
            // 창 포커스를 잃으면 시스템이 IME를 닫으므로 요청 상태도 원점으로.
            keyboardRequested = false
            hud?.setKeyboardChipActive(false)
            ViewerNative.releaseInput(instanceId)
        }
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
        )
        streamSurfaces = next
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
        surfaceLifecycle.confirmAttached(res == 0)
        if (res == 0) {
            // Native attach clears a retained reason for this logical instance
            // before creating the new renderer. Only then may the HUD consume
            // a fresh termination reason.
            hud?.resetTerminationPolling()
            hud?.armTerminationPolling()
            hud?.clearRebindIndicator()
            enableCursorOverlay()
            syncAudioStream()
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
        android.util.Log.i(
            "LeftcarStream",
            "surfaceDestroyed: stop=$stop generation=${surfaceLifecycle.currentGeneration} " +
                "geometryChanges=$surfaceChangeCount instanceId=$instanceId",
        )
        lifecycleEvent(8) // SURFACE_DESTROY
        hideTabletCursor()
        ViewerNative.releaseInput(instanceId)
        // Desktop-mode Back can destroy the Surface before onDestroy. Split
        // detach intentionally stops silently for resize/rebind, so on a
        // final Activity finish release the renderer here while its peer and
        // authentication token are still available; otherwise the later
        // onDestroy release has no active renderer left to send BYE. A holder
        // from a replaced (rebuilt) hierarchy consumes the retired stop flag
        // once — it can never detach a renderer the new hierarchy attached.
        when (stop) {
            StreamSurfaceStop.NONE -> {}
            StreamSurfaceStop.DETACH_RENDERER -> {
                val res = ViewerNative.detachSurface(nativeState, instanceId)
                android.util.Log.i(
                    "LeftcarStream",
                    "detachSurface returned $res; waiting for Surface recreation",
                )
            }
            StreamSurfaceStop.FINAL_RELEASE -> {
                released = true
                ViewerNative.release(nativeState, instanceId)
                android.util.Log.i("LeftcarStream", "final release completed before onDestroy")
            }
        }
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
            "onDestroy: final release instanceId=$instanceId attached=${surfaceLifecycle.isAttached}",
        )
        if (!released) {
            lifecycleEvent(12) // TASK_REMOVE / final Activity destruction
            released = true
        }
        surfaceLifecycle.invalidate()
        cancelPendingSurfaceAttach()
        recoveryRetryRunnable?.let(recoveryHandler::removeCallbacks)
        recoveryRetryRunnable = null
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
        ViewerNative.releaseInput(instanceId)
        ViewerNative.release(nativeState, instanceId)
        StreamLauncherModule.forgetStream(instanceId, ownershipGeneration)
        super.onDestroy()
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
