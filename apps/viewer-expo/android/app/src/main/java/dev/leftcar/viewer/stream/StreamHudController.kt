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
import android.view.animation.AccelerateDecelerateInterpolator
import android.view.animation.DecelerateInterpolator
import android.widget.FrameLayout
import android.widget.ImageView
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
        private const val DEBUG_STATS_VISIBLE_MS = 6_000L
        private const val DEBUG_STATS_FADE_MS = 420L
    }

    private val handler = Handler(Looper.getMainLooper())
    private var inputPopup: PopupWindow? = null
    private var inputView: ImageView? = null
    private var lastInputStatus = Int.MIN_VALUE
    private var statsPopup: PopupWindow? = null
    private var statsView: TextView? = null
    private var rebindPopup: PopupWindow? = null
    private var rebindView: TextView? = null
    private var renderedFpsSample: RenderedFpsSample? = null
    private var lastRenderedFrames: Long? = null
    private var terminationHandled = false
    private val terminationPollGate = TerminationPollGate()
    private val persistentFpsOverlay = PersistentFpsOverlay(activity)
    private val networkLatencySamples = mutableListOf<Long>()
    private val surfaceReleaseLatencySamples = mutableListOf<Long>()

    private val fadeInput = Runnable {
        inputView?.animate()
            ?.alpha(0f)
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
        val indicator = rebindView ?: TextView(activity).apply {
            setTextColor(Color.argb(224, 255, 255, 255))
            textSize = 12f
            setPadding(dp(12), dp(7), dp(12), dp(7))
            background = badgeBackground(Color.argb(168, 15, 23, 42))
            contentDescription = ViewerStrings.rebindDescription
        }.also { view ->
            rebindView = view
            rebindPopup = PopupWindow(
                view,
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
        indicator.text = message
        indicator.alpha = 1f
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
        inputView = null
        statsView = null
        rebindView = null
        persistentFpsOverlay.stop()
    }

    private fun dp(value: Int): Int =
        (value * activity.resources.displayMetrics.density).toInt().coerceAtLeast(1)

    private fun badgeBackground(color: Int): GradientDrawable = GradientDrawable().apply {
        shape = GradientDrawable.RECTANGLE
        cornerRadius = dp(10).toFloat()
        setColor(color)
        setStroke(dp(1), Color.argb(36, 255, 255, 255))
    }

    private fun updateInput(status: Int) {
        if (status == lastInputStatus) return
        lastInputStatus = status
        inputView?.apply {
            when (status) {
                1 -> {
                    setImageResource(R.drawable.ic_remote_unlocked)
                    contentDescription = ViewerStrings.inputAllowed
                }
                0 -> {
                    setImageResource(R.drawable.ic_remote_locked)
                    contentDescription = ViewerStrings.inputLocked
                }
                else -> {
                    setImageResource(R.drawable.ic_remote_locked)
                    contentDescription = ViewerStrings.inputChecking
                }
            }
            background = badgeBackground(Color.argb(118, 15, 23, 42))
        }
        revealInput()
    }

    private fun showInput() {
        if (inputPopup != null) return
        val badge = ImageView(activity).apply {
            scaleType = ImageView.ScaleType.CENTER
            minimumWidth = dp(30)
            minimumHeight = dp(30)
            setPadding(dp(6), dp(6), dp(6), dp(6))
            alpha = 0f
            elevation = dp(2).toFloat()
        }
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
            textSize = 10f
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
