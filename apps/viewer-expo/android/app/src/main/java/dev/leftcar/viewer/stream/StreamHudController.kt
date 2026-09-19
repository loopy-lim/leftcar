package dev.leftcar.viewer.stream

import android.app.Activity
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.ColorDrawable
import android.graphics.drawable.GradientDrawable
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.Gravity
import android.view.View
import android.view.animation.AccelerateDecelerateInterpolator
import android.view.animation.DecelerateInterpolator
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.PopupWindow
import android.widget.TextView
import dev.leftcar.viewer.R
import dev.leftcar.viewer.shim.ViewerNative

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

internal class StreamHudController(
    private val activity: Activity,
    private val instanceId: String,
    private val sourceFps: Int,
    private val showDiagnostics: Boolean,
    private val onTermination: (Int) -> Unit,
    private val onRenderedFrame: () -> Unit = {},
) {
    companion object {
        private const val INPUT_STATUS_VISIBLE_MS = 900L
        private const val INPUT_STATUS_FADE_MS = 320L
        /** 입력이 켜져 있는 동안 유지하는 은은한 배지 투명도 — 꺼짐과 구분되되
         *  잠금 배너보다 조용한다. */
        private const val INPUT_ALLOWED_IDLE_ALPHA = 0.38f
        private const val DEBUG_STATS_VISIBLE_MS = 6_000L
        private const val DEBUG_STATS_FADE_MS = 420L
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
    private var rememberChipPopup: PopupWindow? = null
    private var rememberChipText: TextView? = null
    private var controls: StreamHudControls? = null
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
            ?.setDuration(INPUT_STATUS_FADE_MS)
            ?.setInterpolator(AccelerateDecelerateInterpolator())
            ?.start()
    }
    private val fadeStats = Runnable {
        statsView?.animate()
            ?.alpha(0f)
            ?.setDuration(DEBUG_STATS_FADE_MS)
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
        // 정적 텍스트는 멈춤처럼 보인다 — 인디케이터 회전 스피너가 진행 중임을
        // 즉시 전달한다.
        val container = rebindView as? LinearLayout
        if (container == null) {
            val spinner = android.widget.ProgressBar(activity).apply {
                isIndeterminate = true
                val size = dp(16)
                layoutParams = LinearLayout.LayoutParams(size, size)
                indeterminateTintList = android.content.res.ColorStateList.valueOf(
                    Color.argb(224, 255, 255, 255),
                )
            }
            val text = TextView(activity).apply {
                setTextColor(Color.argb(224, 255, 255, 255))
                textSize = 12f * panelScale
                contentDescription = ViewerStrings.rebindDescription
                layoutParams = LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.WRAP_CONTENT,
                    LinearLayout.LayoutParams.WRAP_CONTENT,
                ).apply { leftMargin = dp(8) }
            }
            val row = LinearLayout(activity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(dp(12), dp(7), dp(12), dp(7))
                background = badgeBackground(Color.argb(168, 15, 23, 42))
                addView(spinner)
                addView(text)
            }
            rebindView = row
            rebindText = text
            rebindPopup = PopupWindow(
                row,
                FrameLayout.LayoutParams.WRAP_CONTENT,
                FrameLayout.LayoutParams.WRAP_CONTENT,
                false,
            ).apply {
                isTouchable = false
                isFocusable = false
                isOutsideTouchable = false
                setBackgroundDrawable(ColorDrawable(Color.TRANSPARENT))
                elevation = dp(2).toFloat()
            }
        }
        rebindText?.text = message
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
    }

    fun onRebindFinished(success: Boolean) {
        if (success) {
            resetTerminationPolling()
            armTerminationPolling()
            clearRebindIndicator()
        } else {
            showRebindIndicator(ViewerStrings.rebindFailed)
        }
        handler.removeCallbacks(poll)
        handler.post(poll)
    }

    /**
     * "이 크기 기억" 칩 — 창 크기가 호스트 저장값과 달라졌을 때만 입력 배지
     * 아래에 나타난다. 유일하게 터치를 받는 HUD 요소라 자체 팝업으로 띄운다;
     * 누르면 [onRemember]로 현재 크기를 저장하고 사라진다.
     */
    fun showRememberSizeChip(onRemember: () -> Unit) {
        rememberChipText?.let { existing ->
            existing.alpha = 1f
            return
        }
        val chip = TextView(activity).apply {
            text = ViewerStrings.rememberSize
            contentDescription = ViewerStrings.rememberSize
            setTextColor(Color.argb(232, 255, 255, 255))
            textSize = 11f * panelScale
            setPadding(dp(12), dp(7), dp(12), dp(7))
            background = badgeBackground(Color.argb(178, 15, 23, 42))
            elevation = dp(3).toFloat()
            // 이 팝업은 터치를 받는다 — 배지와 달리 눌러야 하는 버튼이다.
            setOnClickListener {
                it.isEnabled = false
                onRemember()
            }
        }
        rememberChipText = chip
        rememberChipPopup = PopupWindow(
            chip,
            FrameLayout.LayoutParams.WRAP_CONTENT,
            FrameLayout.LayoutParams.WRAP_CONTENT,
            false,
        ).apply {
            isTouchable = true
            isFocusable = false
            isOutsideTouchable = false
            setBackgroundDrawable(ColorDrawable(Color.TRANSPARENT))
            elevation = dp(3).toFloat()
        }
        val popup = rememberChipPopup ?: return
        activity.window.decorView.post {
            if (!popup.isShowing && !activity.isFinishing && !activity.isDestroyed) {
                popup.showAtLocation(
                    activity.window.decorView,
                    Gravity.TOP or Gravity.END,
                    dp(12),
                    // 입력 배지(상단 12dp + ~30dp) 바로 아래 줄.
                    dp(52),
                )
            }
        }
    }

    fun hideRememberSizeChip() {
        rememberChipPopup?.dismiss()
        rememberChipPopup = null
        rememberChipText = null
    }

    fun revealInput() {
        val badge = inputView ?: return
        handler.removeCallbacks(fadeInput)
        badge.animate().cancel()
        badge.animate()
            .alpha(0.82f)
            .setDuration(110L)
            .setInterpolator(DecelerateInterpolator())
            .withEndAction { handler.postDelayed(fadeInput, INPUT_STATUS_VISIBLE_MS) }
            .start()
    }

    fun revealStats() {
        val stats = statsView ?: return
        handler.removeCallbacks(fadeStats)
        stats.animate().cancel()
        stats.animate()
            .alpha(0.76f)
            .setDuration(130L)
            .setInterpolator(DecelerateInterpolator())
            .withEndAction { handler.postDelayed(fadeStats, DEBUG_STATS_VISIBLE_MS) }
            .start()
    }

    fun stop() {
        handler.removeCallbacks(poll)
        handler.removeCallbacks(fadeInput)
        handler.removeCallbacks(fadeStats)
        inputView?.animate()?.cancel()
        statsView?.animate()?.cancel()
        inputPopup?.dismiss()
        statsPopup?.dismiss()
        rebindPopup?.dismiss()
        inputPopup = null
        statsPopup = null
        rebindPopup = null
        rememberChipPopup?.dismiss()
        rememberChipPopup = null
        rememberChipText = null
        controls = null
        inputView = null
        inputIcon = null
        inputLabel = null
        statsView = null
        rebindView = null
        rebindText = null
        persistentFpsOverlay.stop()
    }

    private fun dp(value: Int): Int =
        StreamPanelDensity.dp(
            value.toFloat(),
            activity.resources.displayMetrics.density,
            panelScale,
        )

    /** 창 폭 기반 HUD 배율 — XR 대형 패널에서 배지·칩이 확대된다. */
    private val panelScale = StreamPanelDensity.scaleOf(activity)

    private fun badgeBackground(color: Int): GradientDrawable = GradientDrawable().apply {
        shape = GradientDrawable.RECTANGLE
        cornerRadius = dp(10).toFloat()
        setColor(color)
        setStroke(dp(1), Color.argb(36, 255, 255, 255))
    }

    private fun updateInput(status: Int) {
        if (status == lastInputStatus) return
        lastInputStatus = status
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
        inputView?.background = badgeBackground(Color.argb(118, 15, 23, 42))
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
        }
        val label = TextView(activity).apply {
            setTextColor(Color.argb(224, 255, 255, 255))
            textSize = 11f * panelScale
        }
        val badge = LinearLayout(activity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(6), dp(5), dp(8), dp(5))
            addView(icon)
            addView(label)
            alpha = 0f
            elevation = dp(2).toFloat()
        }
        inputIcon = icon
        inputLabel = label
        inputView = badge
        updateInput(-1)
        val column = StreamHudControls(activity, panelScale)
        column.view.addView(badge)
        controls = column
        val popup = PopupWindow(
            column.view,
            FrameLayout.LayoutParams.WRAP_CONTENT,
            FrameLayout.LayoutParams.WRAP_CONTENT,
            false,
        ).apply {
            isTouchable = false
            isFocusable = false
            isOutsideTouchable = false
            setBackgroundDrawable(ColorDrawable(Color.TRANSPARENT))
            elevation = dp(2).toFloat()
        }
        inputPopup = popup
        activity.window.decorView.post {
            if (!activity.isFinishing && !activity.isDestroyed) {
                popup.showAtLocation(
                    activity.window.decorView,
                    Gravity.TOP or Gravity.END,
                    dp(12),
                    dp(12),
                )
                handler.removeCallbacks(poll)
                handler.post(poll)
            }
        }
    }

    private fun updateStats(packed: Long, latency: Long, surfaceReleaseLatency: Int) {
        val stats = statsView ?: return
        if (packed == -1L) {
            lastRenderedFrames = null
            renderedFpsSample = null
            persistentFpsOverlay.update(null)
            stats.text = "SRC $sourceFps / DISPLAY -- Hz  NET --/-- ms  CAP→SURF --/-- ms\n-- FPS  FEED -- ms"
            return
        }
        val rendered = packed and ((1L shl 28) - 1)
        lastRenderedFrames?.let { previous ->
            if (rendered > previous) onRenderedFrame()
        }
        lastRenderedFrames = rendered
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
        val displayHz = activity.window.decorView.display?.refreshRate?.toInt() ?: 0
        stats.text = "SRC $sourceFps / DISPLAY ${if (displayHz > 0) displayHz else "--"} Hz  " +
            "NET ${formatLatency(networkLatencySamples)} ms  " +
            "CAP→SURF ${formatLatency(surfaceReleaseLatencySamples)} ms\n" +
            "${persistentFpsText(displayedFps)}  FEED ${feedMs} ms  " +
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
            setTextColor(Color.argb(196, 255, 255, 255))
            textSize = 10f * panelScale
            typeface = Typeface.MONOSPACE
            setPadding(dp(9), dp(4), dp(9), dp(4))
            background = badgeBackground(Color.argb(92, 15, 23, 42))
            alpha = 0f
            text = "SRC $sourceFps / DISPLAY -- Hz  NET --/-- ms  CAP→SURF --/-- ms\n-- FPS  FEED -- ms"
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
            setBackgroundDrawable(ColorDrawable(Color.TRANSPARENT))
            elevation = dp(1).toFloat()
        }
        statsPopup = popup
        activity.window.decorView.post {
            if (!activity.isFinishing && !activity.isDestroyed) {
                popup.showAtLocation(
                    activity.window.decorView,
                    Gravity.TOP or Gravity.CENTER_HORIZONTAL,
                    0,
                    dp(12),
                )
                revealStats()
            }
        }
    }
}

/** All HUD targets share one measured column; labels and font scale determine
 * actual height, and margins are applied after that height (never fixed y slots).
 */
internal class StreamHudControls(context: android.content.Context, private val scale: Float) {
    private val density = context.resources.displayMetrics.density
    private fun dp(value: Int) = StreamPanelDensity.dp(value.toFloat(), density, scale)
    val view = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        gravity = Gravity.END
    }
}
