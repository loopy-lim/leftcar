package dev.leftcar.viewer.stream

import android.app.Activity
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.ColorDrawable
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.view.Gravity
import android.view.View
import android.view.ViewTreeObserver
import android.view.WindowInsets
import android.widget.FrameLayout
import android.widget.PopupWindow
import android.widget.TextView
import kotlin.math.roundToInt

internal data class RenderedFpsSample(
    val frames: Long,
    val sampledAtMs: Long,
    val displayedFps: Double?,
)

internal fun nextRenderedFpsSample(
    previous: RenderedFpsSample?,
    renderedFrames: Long,
    nowMs: Long,
): RenderedFpsSample {
    if (previous == null || renderedFrames < previous.frames || nowMs <= previous.sampledAtMs) {
        return RenderedFpsSample(renderedFrames, nowMs, null)
    }
    val elapsedMs = (nowMs - previous.sampledAtMs).coerceAtLeast(1L)
    val sampledFps = ((renderedFrames - previous.frames) * 1_000.0 / elapsedMs)
        .coerceIn(0.0, 240.0)
    val displayedFps = previous.displayedFps
        ?.takeIf { it != 0.0 }
        ?.let { it * 0.65 + sampledFps * 0.35 }
        ?: sampledFps
    return RenderedFpsSample(renderedFrames, nowMs, displayedFps)
}

internal data class PersistentFpsOverlayPolicy(
    val textSizeSp: Float = 13f,
    val textColorAlpha: Int = 235,
    val backgroundAlpha: Int = 144,
    val horizontalPaddingDp: Int = 8,
    val verticalPaddingDp: Int = 4,
    val cornerRadiusDp: Int = 5,
    val edgeOffsetDp: Int = 16,
    val gravity: Int = Gravity.BOTTOM or Gravity.END,
    val touchable: Boolean = false,
    val focusable: Boolean = false,
    val outsideTouchable: Boolean = false,
    val animate: Boolean = false,
    val elevationDp: Float = 0f,
)

internal val persistentFpsOverlayPolicy = PersistentFpsOverlayPolicy()

internal fun persistentFpsText(displayedFps: Double?): String =
    displayedFps?.takeIf { it.isFinite() && it >= 0.0 }?.let { "${it.roundToInt()} FPS" } ?: "-- FPS"

internal fun persistentFpsContentDescription(
    displayedFps: Double?,
    language: String = ViewerStrings.language,
): String = if (language == "en") {
    "Actual render rate ${persistentFpsText(displayedFps)}"
} else {
    "실제 렌더링 속도 ${persistentFpsText(displayedFps)}"
}

internal class PersistentFpsOverlay(private val activity: Activity) {
    private var popup: PopupWindow? = null
    private var textView: TextView? = null
    private var decorView: View? = null
    private var displayedFps: Double? = null
    private val layoutListener = ViewTreeObserver.OnGlobalLayoutListener { updateLocation() }

    fun show() {
        if (popup != null) return
        val policy = persistentFpsOverlayPolicy
        val view = TextView(activity).apply {
            setTextColor(Color.argb(policy.textColorAlpha, 255, 255, 255))
            textSize = policy.textSizeSp
            typeface = Typeface.MONOSPACE
            setPadding(
                dp(policy.horizontalPaddingDp),
                dp(policy.verticalPaddingDp),
                dp(policy.horizontalPaddingDp),
                dp(policy.verticalPaddingDp),
            )
            background = GradientDrawable().apply {
                shape = GradientDrawable.RECTANGLE
                cornerRadius = dp(policy.cornerRadiusDp).toFloat()
                setColor(Color.argb(policy.backgroundAlpha, 15, 23, 42))
            }
            elevation = 0f
            text = persistentFpsText(displayedFps)
            contentDescription = persistentFpsContentDescription(displayedFps)
        }
        val nextPopup = PopupWindow(
            view,
            FrameLayout.LayoutParams.WRAP_CONTENT,
            FrameLayout.LayoutParams.WRAP_CONTENT,
            policy.focusable,
        ).apply {
            isTouchable = policy.touchable
            isFocusable = policy.focusable
            isOutsideTouchable = policy.outsideTouchable
            setBackgroundDrawable(ColorDrawable(Color.TRANSPARENT))
            elevation = 0f
            animationStyle = 0
        }
        textView = view
        popup = nextPopup
        val decor = activity.window.decorView
        decorView = decor
        decor.post {
            if (popup === nextPopup && !activity.isFinishing && !activity.isDestroyed) {
                val offsets = offsets()
                nextPopup.showAtLocation(decor, policy.gravity, offsets.first, offsets.second)
                decor.viewTreeObserver.addOnGlobalLayoutListener(layoutListener)
            }
        }
    }

    fun update(displayedFps: Double?) {
        this.displayedFps = displayedFps
        textView?.apply {
            text = persistentFpsText(displayedFps)
            contentDescription = persistentFpsContentDescription(displayedFps)
        }
    }

    fun stop() {
        decorView?.viewTreeObserver?.let { observer ->
            if (observer.isAlive) observer.removeOnGlobalLayoutListener(layoutListener)
        }
        popup?.dismiss()
        popup = null
        textView = null
        decorView = null
        displayedFps = null
    }

    private fun updateLocation() {
        val currentPopup = popup ?: return
        if (currentPopup.isShowing) {
            val offsets = offsets()
            currentPopup.update(offsets.first, offsets.second, -1, -1)
        }
    }

    private fun offsets(): Pair<Int, Int> {
        val decor = decorView ?: activity.window.decorView
        val bottomInset: Int
        val rightInset: Int
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val insets = decor.rootWindowInsets?.getInsets(
                WindowInsets.Type.systemBars() or WindowInsets.Type.mandatorySystemGestures(),
            )
            bottomInset = insets?.bottom ?: 0
            rightInset = insets?.right ?: 0
        } else {
            @Suppress("DEPRECATION")
            val insets = decor.rootWindowInsets
            @Suppress("DEPRECATION")
            run {
                bottomInset = insets?.systemWindowInsetBottom ?: 0
                rightInset = insets?.systemWindowInsetRight ?: 0
            }
        }
        return Pair(
            dp(persistentFpsOverlayPolicy.edgeOffsetDp) + rightInset,
            dp(persistentFpsOverlayPolicy.edgeOffsetDp) + bottomInset,
        )
    }

    private fun dp(value: Int): Int =
        (value * activity.resources.displayMetrics.density).toInt().coerceAtLeast(1)
}
