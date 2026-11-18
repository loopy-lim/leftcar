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

internal data class RenderedFpsPoint(
    val frames: Long,
    val sampledAtMs: Long,
)

internal data class RenderedFpsSample(
    val history: List<RenderedFpsPoint>,
    val displayedFps: Double?,
)

/** 윈도우 길이 상한(포인트 수). 250ms 폴링 기준 최대 ~1.75s 창. */
internal const val FPS_WINDOW_POINTS = 8
/** 이보다 짧은 창은 노이스라 값들을 갱신하지 않고 직전 표시를 유지한다. */
internal const val FPS_WINDOW_MIN_MS = 500L

/**
 * EMA(0.65/0.35)를 1.5초 창 적분 평균으로 교체했다. EMA는 프레임 카운터가
 * 한 틱 멈추면 0 샘플이 35% 가중으로 박혀 실측 p50(57) 대비 40까지 억제됐고
 * 회복에도 수 틱이 걸렸다. 창 적분은 창 안의 총 프레임 수를 그대로 나누므로
 * 죽은 틱이 창을 벗어나면 즉시 실제 속도로 돌아온다. 최소 창(500ms) 미만에서는
 * 직전 값을 유지해 인접 정수 경계에서의 흔들림(케이던스 플러터)을 없앤다.
 */
internal fun nextRenderedFpsSample(
    previous: RenderedFpsSample?,
    renderedFrames: Long,
    nowMs: Long,
): RenderedFpsSample {
    val newest = previous?.history?.lastOrNull()
    if (previous == null || newest == null || renderedFrames < newest.frames || nowMs <= newest.sampledAtMs) {
        return RenderedFpsSample(listOf(RenderedFpsPoint(renderedFrames, nowMs)), null)
    }
    val history = (previous.history + RenderedFpsPoint(renderedFrames, nowMs))
        .takeLast(FPS_WINDOW_POINTS)
    val anchor = history.first()
    val elapsedMs = (nowMs - anchor.sampledAtMs).coerceAtLeast(1L)
    val displayedFps = if (elapsedMs >= FPS_WINDOW_MIN_MS) {
        ((renderedFrames - anchor.frames) * 1_000.0 / elapsedMs).coerceIn(0.0, 240.0)
    } else {
        previous.displayedFps
    }
    return RenderedFpsSample(history, displayedFps)
}

/**
 * HUD fps 배지의 유휴 판별 임계(docs/2026-11-18-macos-promotion-idle-capture-cadence.md).
 * ProMotion 호스트는 정적 화면에서 컴포지터 cadence가 내려가 캡처 프레임 수
 * 자체가 줄어든다(실측 41~52fps). 손실·갭 없이 fps만 낮은 상태를 "유휴"로
 * 표시해 성능 저하와 구분한다. 진입 0.85/해제 0.93의 히스테리시스와 1.5케이
 * 지속 조건으로 모션 직후 램프다운·램프업 경계의 플러터를 막는다.
 */
internal const val HUD_FPS_IDLE_ENTER_RATIO = 0.85
internal const val HUD_FPS_IDLE_EXIT_RATIO = 0.93
internal const val HUD_FPS_IDLE_HOLD_MS = 1_500L

internal enum class HudFpsBadgeMode { ACTIVE, IDLE }

internal data class HudFpsBadgeState(
    val mode: HudFpsBadgeMode = HudFpsBadgeMode.ACTIVE,
    val lowFpsSinceMs: Long? = null,
)

/**
 * 순수 상태 전이: fps 저하가 손실(input drops + frame gaps)이나 스테일 프레임
 * 증가 없이 [HUD_FPS_IDLE_HOLD_MS] 이상 지속되면 유휴로 판정한다. 저하의
 * 증거가 하나라도 있으면 즉시 ACTIVE로 돌아간다 — 그 저하는 원본 cadence가
 * 아니라 경로의 결함이다.
 */
internal fun nextHudFpsBadgeState(
    previous: HudFpsBadgeState,
    displayedFps: Double?,
    sourceFps: Int,
    lossDelta: Int,
    staleDelta: Int,
    nowMs: Long,
): HudFpsBadgeState {
    if (lossDelta > 0 || staleDelta > 0) {
        return HudFpsBadgeState(HudFpsBadgeMode.ACTIVE)
    }
    if (displayedFps == null || sourceFps <= 0) {
        return HudFpsBadgeState(HudFpsBadgeMode.ACTIVE)
    }
    return when {
        displayedFps < sourceFps * HUD_FPS_IDLE_ENTER_RATIO -> {
            val since = previous.lowFpsSinceMs ?: nowMs
            val idle = previous.mode == HudFpsBadgeMode.IDLE || nowMs - since >= HUD_FPS_IDLE_HOLD_MS
            HudFpsBadgeState(if (idle) HudFpsBadgeMode.IDLE else HudFpsBadgeMode.ACTIVE, since)
        }
        displayedFps >= sourceFps * HUD_FPS_IDLE_EXIT_RATIO ->
            HudFpsBadgeState(HudFpsBadgeMode.ACTIVE)
        // 진입/해제 임계 사이는 직전 상태를 유지한다(히스테리시스).
        else -> previous
    }
}

internal data class PersistentFpsOverlayPolicy(
    val textSizeSp: Float = StreamUiTokens.BODY_SP,
    val textColorAlpha: Int = 255,
    val backgroundAlpha: Int = 255,
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

internal fun persistentFpsBadgeText(
    displayedFps: Double?,
    mode: HudFpsBadgeMode,
    language: String = ViewerStrings.language,
): String = when (mode) {
    HudFpsBadgeMode.ACTIVE -> persistentFpsText(displayedFps)
    HudFpsBadgeMode.IDLE ->
        if (language == "en") "IDLE(${persistentFpsText(displayedFps)})"
        else "유휴(${persistentFpsText(displayedFps)})"
}

internal fun persistentFpsContentDescription(
    displayedFps: Double?,
    mode: HudFpsBadgeMode = HudFpsBadgeMode.ACTIVE,
    language: String = ViewerStrings.language,
): String {
    val rate = persistentFpsText(displayedFps)
    return when (mode) {
        HudFpsBadgeMode.ACTIVE ->
            if (language == "en") "Actual render rate $rate" else "실제 렌더링 속도 $rate"
        HudFpsBadgeMode.IDLE ->
            if (language == "en") "Screen idle, render rate $rate" else "화면 유휴, 렌더링 속도 $rate"
    }
}

internal class PersistentFpsOverlay(private val activity: Activity) {
    private var popup: PopupWindow? = null
    private var textView: TextView? = null
    private var decorView: View? = null
    private var displayedFps: Double? = null
    private var badgeMode: HudFpsBadgeMode = HudFpsBadgeMode.ACTIVE
    private val layoutListener = ViewTreeObserver.OnGlobalLayoutListener { updateLocation() }

    fun show() {
        if (popup != null) return
        val policy = persistentFpsOverlayPolicy
        val view = TextView(activity).apply {
            setTextColor(StreamUiTokens.INK)
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
                setColor(StreamUiTokens.SURFACE)
                setStroke(dp(1), StreamUiTokens.OUTLINE)
            }
            elevation = 0f
            text = persistentFpsBadgeText(displayedFps, badgeMode)
            contentDescription = persistentFpsContentDescription(displayedFps, badgeMode)
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

    fun update(displayedFps: Double?, mode: HudFpsBadgeMode = HudFpsBadgeMode.ACTIVE) {
        this.displayedFps = displayedFps
        this.badgeMode = mode
        val view = textView ?: return
        val nextText = persistentFpsBadgeText(displayedFps, mode)
        // 표시 값이 실제로 바뀔 때만 다시 그린다 — 250ms 폴링마다 동일 문자열을
        // set하지 않아 배지 레이아웃 패스와 깜빡임을 줄인다.
        if (view.text.toString() == nextText) return
        view.text = nextText
        view.contentDescription = persistentFpsContentDescription(displayedFps, mode)
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
        badgeMode = HudFpsBadgeMode.ACTIVE
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
                WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout() or WindowInsets.Type.mandatorySystemGestures(),
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
