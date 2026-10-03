package dev.leftcar.viewer.stream

import android.app.Activity
import android.animation.ValueAnimator
import android.graphics.Color
import android.graphics.Rect
import android.graphics.Typeface
import android.graphics.drawable.ColorDrawable
import android.graphics.drawable.GradientDrawable
import android.os.Handler
import android.os.Build
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import android.view.Gravity
import android.view.View
import android.view.accessibility.AccessibilityNodeInfo
import android.view.animation.AccelerateDecelerateInterpolator
import android.view.animation.DecelerateInterpolator
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.PopupWindow
import android.widget.TextView
import android.widget.Button
import android.widget.ProgressBar
import dev.leftcar.viewer.R
import dev.leftcar.viewer.shim.ViewerNative
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

internal fun streamChromeInsets(activity: Activity): Rect {
    val insets = ViewCompat.getRootWindowInsets(activity.window.decorView)?.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout() or
            WindowInsetsCompat.Type.mandatorySystemGestures(),
    )
    return Rect(insets?.left ?: 0, insets?.top ?: 0, insets?.right ?: 0, insets?.bottom ?: 0)
}

internal fun streamChromeWidth(activity: Activity): Int {
    val insets = streamChromeInsets(activity)
    val width = activity.window.decorView.width.takeIf { it > 0 } ?: activity.resources.displayMetrics.widthPixels
    val padding = StreamPanelDensity.dp(32f, activity.resources.displayMetrics.density, StreamPanelDensity.scaleOf(activity))
    return (width - insets.left - insets.right - padding).coerceAtLeast(1)
}

internal class TerminationPollGate {
    private var armed = false
    private var consumed = false

    fun armAfterRendererAttached() {
        armed = true
    }

    fun reset() {
        armed = false
        consumed = false
    }

    fun consume(reason: Int): Int? {
        if (!armed || consumed || reason < 0) return null
        consumed = true
        return reason
    }
}

/** Pointer, keyboard and screen-reader clicks share one request cooldown. */
internal class InputApprovalRequestGate(private val cooldownMs: Long = 3_000L) {
    private var lastRequestAt: Long? = null

    fun claim(status: Int, nowMs: Long): Boolean {
        if (status != 0) return false
        if (lastRequestAt?.let { nowMs - it < cooldownMs } == true) return false
        lastRequestAt = nowMs
        return true
    }
}

internal class StreamHudController(
    private val activity: Activity,
    private val instanceId: String,
    private val sourceFps: Int,
    private val showDiagnostics: Boolean,
    private val onTermination: (Int) -> Unit,
    private val onRenderedFrame: () -> Unit = {},
    private val onInputStatusChanged: (Int) -> Unit = {},
    private val onInputRequest: (String) -> Boolean = { false },
    private val onRetryRebind: () -> Unit = {},
) {
    companion object {
        private const val INPUT_STATUS_VISIBLE_MS = 900L
        private const val INPUT_STATUS_FADE_MS = 320L
        /** Opaque chrome keeps its contrast independent of the video pixels. */
        private const val INPUT_ALLOWED_IDLE_ALPHA = 1f
        private const val DEBUG_STATS_VISIBLE_MS = 6_000L
        private const val DEBUG_STATS_FADE_MS = 420L
        private const val INPUT_REQUEST_FEEDBACK_MS = 1_600L
        private const val INPUT_REQUEST_TIMEOUT_MS = 8_000L
    }

    private val handler = Handler(Looper.getMainLooper())
    private var inputPopup: PopupWindow? = null
    private var inputView: View? = null
    private var inputIcon: ImageView? = null
    private var inputLabel: TextView? = null
    private var lastInputStatus = Int.MIN_VALUE
    private var statsPopup: PopupWindow? = null
    private var statsView: TextView? = null
    private var rebindPopup: PopupWindow? = null
    private var rebindView: View? = null
    private var rebindText: TextView? = null
    private var rebindSpinner: ProgressBar? = null
    private var rebindRetry: Button? = null
    private val inputRequestGate = InputApprovalRequestGate()
    // Activity recreation can retain the logical stream generation. Its new
    // HUD must still reject replies to requests from the previous physical view.
    private val inputRequestScope = java.util.UUID.randomUUID().toString()
    private var inputRequestSerial = 0L
    private var pendingInputRequest: String? = null
    private val inputRequestTimeout = Runnable {
        pendingInputRequest?.let { onInputRequestResult(it, ViewerStrings.inputRequestTimeout) }
    }
    private var renderedFpsSample: RenderedFpsSample? = null
    private var lastRenderedFrames: Long? = null
    private var terminationHandled = false
    private val terminationPollGate = TerminationPollGate()
    private val persistentFpsOverlay = PersistentFpsOverlay(activity)
    private val networkLatencySamples = mutableListOf<Long>()
    private val surfaceReleaseLatencySamples = mutableListOf<Long>()

    private val fadeInput = Runnable {
        inputView?.animate()
            ?.alpha(if (lastInputStatus == 1) INPUT_ALLOWED_IDLE_ALPHA else 0f)
            ?.setDuration(animationDuration(INPUT_STATUS_FADE_MS))
            ?.setInterpolator(AccelerateDecelerateInterpolator())
            ?.start()
    }
    private val fadeStats = Runnable {
        statsView?.animate()
            ?.alpha(0f)
            ?.setDuration(animationDuration(DEBUG_STATS_FADE_MS))
            ?.setInterpolator(AccelerateDecelerateInterpolator())
            ?.start()
    }
    private val poll = object : Runnable {
        override fun run() {
            if (activity.isFinishing || activity.isDestroyed) return
            updateInput(ViewerNative.inputStatus(instanceId))
            updateStats(
                ViewerNative.streamStats(instanceId),
                ViewerNative.streamLatency(instanceId),
                ViewerNative.surfaceReleaseLatency(instanceId),
            )
            if (!terminationHandled) {
                val reason = ViewerNative.terminationReason(instanceId)
                val termination = terminationPollGate.consume(reason)
                if (termination != null) {
                    terminationHandled = true
                    onTermination(termination)
                    return
                }
            }
            handler.postDelayed(this, 250L)
        }
    }

    fun show() {
        if (inputPopup != null) return
        showInput()
        // 진단 표시 설정(showFps)은 FPS 배지와 상세 통계 HUD를 함께 통제한다.
        // 꺼져 있으면 statsView를 만들지 않아 탭/키 입력의 revealStats도 no-op이다.
        if (showDiagnostics) {
            showStats()
            persistentFpsOverlay.show()
        }
    }

    fun armTerminationPolling() {
        terminationPollGate.armAfterRendererAttached()
    }

    fun resetTerminationPolling() {
        terminationPollGate.reset()
        terminationHandled = false
    }

    fun showRebindIndicator(message: String) {
        showRebindState(message, busy = true)
    }

    private fun showRebindState(message: String, busy: Boolean) {
        val container = rebindView as? LinearLayout
        if (container == null) {
            val spinner = ProgressBar(activity).apply {
                isIndeterminate = true
                val size = dp(16)
                layoutParams = LinearLayout.LayoutParams(size, size)
                indeterminateTintList = android.content.res.ColorStateList.valueOf(
                    StreamUiTokens.INK,
                )
                importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
            }
            val text = TextView(activity).apply {
                setTextColor(StreamUiTokens.INK)
                textSize = StreamUiTokens.BODY_SP * panelScale
                maxWidth = (chromeWidth() - dp(84)).coerceAtLeast(1)
                accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
                layoutParams = LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.WRAP_CONTENT,
                    LinearLayout.LayoutParams.WRAP_CONTENT,
                ).apply { leftMargin = dp(8) }
            }
            val retry = Button(activity).apply {
                this.text = ViewerStrings.retry
                contentDescription = ViewerStrings.retryRebind
                textSize = StreamUiTokens.CAPTION_SP * panelScale
                setTextColor(StreamUiTokens.INK)
                minWidth = dp(StreamUiTokens.MIN_TARGET_DP)
                minimumHeight = dp(StreamUiTokens.MIN_TARGET_DP)
                background = badgeBackground(StreamUiTokens.SUBTLE)
                setOnClickListener {
                    showRebindIndicator(ViewerStrings.rebindReconnecting)
                    onRetryRebind()
                }
                layoutParams = LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.WRAP_CONTENT,
                    LinearLayout.LayoutParams.WRAP_CONTENT,
                ).apply { leftMargin = dp(8) }
            }
            val row = LinearLayout(activity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(dp(12), dp(7), dp(12), dp(7))
                background = badgeBackground(StreamUiTokens.SURFACE)
                addView(spinner)
                addView(text)
                addView(retry)
            }
            rebindView = row
            rebindText = text
            rebindSpinner = spinner
            rebindRetry = retry
            rebindPopup = PopupWindow(
                row,
                FrameLayout.LayoutParams.WRAP_CONTENT,
                FrameLayout.LayoutParams.WRAP_CONTENT,
                false,
            ).apply {
                isTouchable = false
                isFocusable = false
                isOutsideTouchable = false
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) setTouchModal(false)
                animationStyle = 0
                setBackgroundDrawable(ColorDrawable(Color.TRANSPARENT))
                elevation = dp(2).toFloat()
            }
        }
        rebindText?.text = message
        rebindSpinner?.visibility = if (busy && animationsEnabled()) View.VISIBLE else View.GONE
        rebindRetry?.visibility = if (busy) View.GONE else View.VISIBLE
        rebindPopup?.isTouchable = !busy
        rebindPopup?.update()
        rebindView?.alpha = 1f
        activity.window.decorView.post {
            val popup = rebindPopup ?: return@post
            if (!popup.isShowing && !activity.isFinishing && !activity.isDestroyed) {
                popup.showAtLocation(
                    activity.window.decorView,
                    Gravity.CENTER,
                    0,
                    0,
                )
            }
        }
    }

    fun clearRebindIndicator() {
        rebindPopup?.dismiss()
        rebindPopup = null
        rebindView = null
        rebindText = null
        rebindSpinner = null
        rebindRetry = null
    }

    fun onRebindFinished(success: Boolean) {
        if (success) {
            resetTerminationPolling()
            armTerminationPolling()
            clearRebindIndicator()
        } else {
            showRebindState(ViewerStrings.rebindFailed, busy = false)
        }
        handler.removeCallbacks(poll)
        handler.post(poll)
    }

    fun revealInput() {
        val badge = inputView ?: return
        handler.removeCallbacks(fadeInput)
        badge.animate().cancel()
        badge.animate()
            .alpha(1f)
            .setDuration(animationDuration(110L))
            .setInterpolator(DecelerateInterpolator())
            .withEndAction { handler.postDelayed(fadeInput, INPUT_STATUS_VISIBLE_MS) }
            .start()
    }

    fun revealStats() {
        val stats = statsView ?: return
        handler.removeCallbacks(fadeStats)
        stats.animate().cancel()
        stats.animate()
            .alpha(1f)
            .setDuration(animationDuration(130L))
            .setInterpolator(DecelerateInterpolator())
            .withEndAction { handler.postDelayed(fadeStats, DEBUG_STATS_VISIBLE_MS) }
            .start()
    }

    private fun requestInputApproval() {
        if (pendingInputRequest != null) return
        if (!inputRequestGate.claim(lastInputStatus, SystemClock.elapsedRealtime())) return
        val requestId = "$inputRequestScope:${++inputRequestSerial}"
        pendingInputRequest = requestId
        handler.removeCallbacks(restoreInputBanner)
        setInputRequestMessage(ViewerStrings.inputRequestPending)
        inputView?.isEnabled = false
        handler.postDelayed(inputRequestTimeout, INPUT_REQUEST_TIMEOUT_MS)
        if (!onInputRequest(requestId)) onInputRequestResult(requestId, ViewerStrings.inputRequestUnavailable)
    }

    fun onInputRequestResult(requestId: String, error: String?): Boolean {
        if (pendingInputRequest != requestId || lastInputStatus != 0) return false
        handler.removeCallbacks(inputRequestTimeout)
        pendingInputRequest = null
        inputView?.isEnabled = true
        if (error == null) onInputRequestSent()
        else setInputRequestMessage("${error.trim().take(160)}\n${ViewerStrings.inputRequestRetry}")
        return true
    }

    fun invalidateInputRequests() {
        pendingInputRequest = null
        handler.removeCallbacks(inputRequestTimeout)
        handler.removeCallbacks(restoreInputBanner)
        inputView?.isEnabled = true
        if (lastInputStatus == 0) setInputRequestMessage(ViewerStrings.inputLockedBanner)
    }

    private fun setInputRequestMessage(message: String) {
        inputLabel?.text = message
        inputView?.contentDescription = message
        inputView?.animate()?.cancel()
        inputView?.alpha = 1f
    }

    /** 요청 전송 피드백 — 배너 문구를 잠시 바꿔 탭이 닿았음을 보여 준 뒤
     * 원래 잠금 문구로 돌아온다(상태가 풀리면 updateInput이 덮어쓴다). */
    fun onInputRequestSent() {
        setInputRequestMessage(ViewerStrings.inputRequestSent)
        handler.removeCallbacks(restoreInputBanner)
        handler.postDelayed(restoreInputBanner, INPUT_REQUEST_FEEDBACK_MS)
    }

    private val restoreInputBanner = Runnable {
        if (lastInputStatus == 0) {
            inputLabel?.text = ViewerStrings.inputLockedBanner
            inputView?.contentDescription = ViewerStrings.inputLockedBanner
        }
    }

    fun stop() {
        invalidateInputRequests()
        handler.removeCallbacks(poll)
        handler.removeCallbacks(fadeInput)
        handler.removeCallbacks(fadeStats)
        handler.removeCallbacks(restoreInputBanner)
        inputView?.animate()?.cancel()
        statsView?.animate()?.cancel()
        inputPopup?.dismiss()
        statsPopup?.dismiss()
        rebindPopup?.dismiss()
        inputPopup = null
        statsPopup = null
        rebindPopup = null
        inputView = null
        inputIcon = null
        inputLabel = null
        statsView = null
        rebindView = null
        rebindText = null
        rebindSpinner = null
        rebindRetry = null
        persistentFpsOverlay.stop()
    }

    private fun dp(value: Int): Int =
        StreamPanelDensity.dp(
            value.toFloat(),
            activity.resources.displayMetrics.density,
            panelScale,
        )

    /** 창 폭 기반 HUD 배율 — XR 대형 패널에서 배지가 확대된다. */
    private val panelScale = StreamPanelDensity.scaleOf(activity)

    private fun chromeWidth(): Int = streamChromeWidth(activity)

    private fun animationsEnabled(): Boolean =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) ValueAnimator.areAnimatorsEnabled()
        else Settings.Global.getFloat(activity.contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) > 0f

    private fun animationDuration(requestedMs: Long): Long = if (animationsEnabled()) requestedMs else 0L

    private fun badgeBackground(color: Int): GradientDrawable = GradientDrawable().apply {
        shape = GradientDrawable.RECTANGLE
        cornerRadius = dp(StreamUiTokens.RADIUS_DP).toFloat()
        setColor(color)
        setStroke(dp(1), StreamUiTokens.OUTLINE)
    }

    private fun updateInput(status: Int) {
        if (status == lastInputStatus) return
        lastInputStatus = status
        if (status != 0) invalidateInputRequests()
        onInputStatusChanged(status)
        val icon = inputIcon ?: return
        when (status) {
            1 -> {
                icon.setImageResource(R.drawable.ic_remote_unlocked)
                inputLabel?.text = ""
                inputView?.contentDescription = ViewerStrings.inputAllowed
            }
            0 -> {
                icon.setImageResource(R.drawable.ic_remote_locked)
                inputLabel?.text = ViewerStrings.inputLockedBanner
                inputView?.contentDescription = ViewerStrings.inputLockedBanner
            }
            else -> {
                icon.setImageResource(R.drawable.ic_remote_locked)
                inputLabel?.text = ""
                inputView?.contentDescription = ViewerStrings.inputChecking
            }
        }
        inputView?.background = badgeBackground(StreamUiTokens.SURFACE)
        inputView?.isClickable = status == 0
        inputView?.isFocusable = status == 0
        inputPopup?.isTouchable = status == 0
        // Keep the stream window's Back/keyboard routing. Native click actions
        // and accessibility focus work without making this popup a key window.
        inputPopup?.update()
        if (status == 0) {
            // 잠김 동안은 배너를 유지한다 — 입력이 죽은 이유와 승인 장소를
            // 알려 주는 유일한 창구다.
            handler.removeCallbacks(fadeInput)
            inputView?.animate()?.cancel()
            inputView?.alpha = 1f
        } else if (status == 1) {
            // 허용됨: 완전히 사라지지 않고 은은하게 남아 "지금 입력이 살아
            // 있다"를 스트림 내내 보여 준다. 상호작용 시 revealInput이 강조한다.
            handler.removeCallbacks(fadeInput)
            inputView?.animate()?.cancel()
            inputView?.alpha = INPUT_ALLOWED_IDLE_ALPHA
        } else {
            revealInput()
        }
    }

    private fun showInput() {
        if (inputPopup != null) return
        val icon = ImageView(activity).apply {
            scaleType = ImageView.ScaleType.CENTER
            minimumWidth = dp(20)
            minimumHeight = dp(20)
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        }
        val label = TextView(activity).apply {
            setTextColor(StreamUiTokens.INK)
            textSize = StreamUiTokens.BODY_SP * panelScale
            maxWidth = (chromeWidth() - dp(48)).coerceAtLeast(1)
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        }
        val badge = LinearLayout(activity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(6), dp(5), dp(8), dp(5))
            minimumHeight = dp(StreamUiTokens.MIN_TARGET_DP)
            minimumWidth = dp(StreamUiTokens.MIN_TARGET_DP)
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_YES
            accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
            setOnClickListener { requestInputApproval() }
            accessibilityDelegate = object : View.AccessibilityDelegate() {
                override fun onInitializeAccessibilityNodeInfo(host: View, info: AccessibilityNodeInfo) {
                    super.onInitializeAccessibilityNodeInfo(host, info)
                    info.className = if (lastInputStatus == 0) Button::class.java.name else TextView::class.java.name
                    info.isEnabled = pendingInputRequest == null
                }
            }
            setOnFocusChangeListener { _, focused ->
                background = badgeBackground(StreamUiTokens.SURFACE).apply {
                    if (focused) setStroke(dp(2), StreamUiTokens.INK)
                }
            }
            addOnLayoutChangeListener { _, _, _, _, _, _, _, _, _ ->
                statsPopup?.takeIf { it.isShowing }?.update(0, statsTopOffset(), -1, -1)
            }
            addView(icon)
            addView(label)
            alpha = 0f
            elevation = dp(2).toFloat()
        }
        inputIcon = icon
        inputLabel = label
        inputView = badge
        updateInput(-1)
        val popup = PopupWindow(
            badge,
            FrameLayout.LayoutParams.WRAP_CONTENT,
            FrameLayout.LayoutParams.WRAP_CONTENT,
            false,
        ).apply {
            isTouchable = false
            isFocusable = false
            isOutsideTouchable = false
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) setTouchModal(false)
            animationStyle = 0
            setBackgroundDrawable(ColorDrawable(Color.TRANSPARENT))
            elevation = dp(2).toFloat()
        }
        inputPopup = popup
        activity.window.decorView.post {
            if (inputPopup === popup && !activity.isFinishing && !activity.isDestroyed) {
                val insets = streamChromeInsets(activity)
                popup.showAtLocation(
                    activity.window.decorView,
                    Gravity.TOP or Gravity.END,
                    dp(12) + insets.right,
                    dp(12) + insets.top,
                )
                handler.removeCallbacks(poll)
                handler.post(poll)
            }
        }
    }

    private fun updateStats(packed: Long, latency: Long, surfaceReleaseLatency: Int) {
        if (packed == -1L) {
            lastRenderedFrames = null
            renderedFpsSample = null
            persistentFpsOverlay.update(null)
            statsView?.text = "FPS --  ENC --/--ms  DEC --ms  NET --/--ms  SKIP -  LOSS -"
            return
        }
        val rendered = packed and ((1L shl 28) - 1)
        lastRenderedFrames?.let { previous ->
            if (rendered > previous) onRenderedFrame()
        }
        lastRenderedFrames = rendered
        // Renderer health is independent of whether diagnostic chrome exists.
        val stats = statsView ?: return
        val stale = (packed ushr 28) and 0x0fff
        val inputDrops = (packed ushr 40) and 0xff
        val frameGaps = (packed ushr 48) and 0xff
        val feedMs = (packed ushr 56) and 0xff
        val now = SystemClock.elapsedRealtime()
        renderedFpsSample = nextRenderedFpsSample(renderedFpsSample, rendered, now)
        val displayedFps = renderedFpsSample?.displayedFps
        persistentFpsOverlay.update(displayedFps)
        addLatencySample(networkLatencySamples, if (latency == -1L) 0xffff else latency and 0xffff)
        addLatencySample(surfaceReleaseLatencySamples, surfaceReleaseLatency.toLong())
        stats.text = "${persistentFpsText(displayedFps)}  " +
            "ENC ${formatLatency(surfaceReleaseLatencySamples)}ms  " +
            "DEC ${feedMs}ms  " +
            "NET ${formatLatency(networkLatencySamples)}ms  " +
            "SKIP ${stale}  LOSS ${inputDrops + frameGaps}"
    }

    private fun addLatencySample(samples: MutableList<Long>, value: Long) {
        if (value == 0xffffL) {
            samples.clear()
            return
        }
        samples += value
        if (samples.size > 40) samples.removeAt(0)
    }

    private fun formatLatency(samples: List<Long>): String {
        if (samples.isEmpty()) return "--/--"
        val sorted = samples.sorted()
        fun percentile(percent: Int): Long {
            val index = ((sorted.size * percent + 99) / 100 - 1).coerceIn(0, sorted.lastIndex)
            return sorted[index]
        }
        return "${percentile(50)}/${percentile(95)}"
    }

    private fun showStats() {
        if (statsPopup != null) return
        val stats = TextView(activity).apply {
            setTextColor(StreamUiTokens.INK)
            textSize = StreamUiTokens.CAPTION_SP * panelScale
            typeface = Typeface.MONOSPACE
            setPadding(dp(9), dp(4), dp(9), dp(4))
            background = badgeBackground(StreamUiTokens.SURFACE)
            maxWidth = chromeWidth()
            alpha = 0f
            text = "FPS --  ENC --/--ms  DEC --ms  NET --/--ms  SKIP -  LOSS -"
            contentDescription = ViewerStrings.statsDescription
        }
        statsView = stats
        val popup = PopupWindow(
            stats,
            FrameLayout.LayoutParams.WRAP_CONTENT,
            FrameLayout.LayoutParams.WRAP_CONTENT,
            false,
        ).apply {
            isTouchable = false
            isFocusable = false
            isOutsideTouchable = false
            animationStyle = 0
            setBackgroundDrawable(ColorDrawable(Color.TRANSPARENT))
            elevation = dp(1).toFloat()
        }
        statsPopup = popup
        activity.window.decorView.post {
            if (statsPopup === popup && !activity.isFinishing && !activity.isDestroyed) {
                popup.showAtLocation(
                    activity.window.decorView,
                    Gravity.TOP or Gravity.CENTER_HORIZONTAL,
                    0,
                    statsTopOffset(),
                )
                revealStats()
            }
        }
    }

    private fun statsTopOffset(): Int = streamChromeInsets(activity).top +
        maxOf(dp(84), (inputView?.height ?: 0) + dp(24))
}
