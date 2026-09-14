package dev.leftcar.viewer.stream

import android.app.Activity
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.graphics.drawable.ColorDrawable
import android.view.Choreographer
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.PopupWindow
import dev.leftcar.viewer.shim.ViewerNative
import kotlin.math.min

/**
 * Local cursor overlay fed by the host LCD1 position stream. Polls the packed
 * native sample at frame rate and moves itself; a -1 state means the host
 * never opted in, so the overlay stays fully hidden (old-host fallback).
 *
 * The stream SurfaceView composites on top of the Activity window
 * (setZOrderOnTop), so no view inside that window can draw above the video.
 * The HUD badges already solve this with separate PopupWindows; the cursor
 * rides the same route and maps normalized coords onto the video rect inside
 * its full-window host frame — the same rect the pointer-forwarding path
 * derives normalized input from.
 */
internal class CursorOverlayView(
    private val activity: Activity,
    private val instanceId: String,
) : View(activity) {
    companion object {
        private const val CURSOR_WIDTH_DP = 18
        private const val CURSOR_HEIGHT_DP = 22

        /**
         * JNI 폴링 절전(Q9a): cursorState 하나가 전역 라이프사이클 뮤텍스와
         * Arc 클론을 태우므로 무조건 매 프레임 호출은 서멀/전력 낭비다.
         * 커서가 보이는 동안에는 매 디스플레이 프레임(60Hz 패널에서 60Hz)으로
         * 폴링해 커서가 패널 주사율의 절반으로 그려지는 "딱딱 끊김"을 없앤다
         * (033f1da의 프레임 2개당 1회 폴링이 이 회귀의 원인이었다).
         * 숨김 상태에서만 ~2Hz(프레임 30개당 1회)로 낮춘다. inactive→active
         * 전이는 [nudge]가 로컬 입력 직후 다음 프레임 폴링으로 즉시 잡는다.
         */
        private const val ACTIVE_POLL_EVERY_N_FRAMES = 1
        private const val IDLE_POLL_EVERY_N_FRAMES = 30
        private const val TAP_ECHO_SIZE_DP = 52
        private const val TAP_ECHO_DURATION_MS = 220L
    }

    private val choreographer = Choreographer.getInstance()
    /** 창 폭 기반 커서 배율 — XR 대형 패널에서 18dp 화살표가 너무 작아진다. */
    private val panelScale = StreamPanelDensity.scaleOf(activity)
    private val host = FrameLayout(activity)
    private var popup: PopupWindow? = null
    private var running = false
    private var lastSequence = Long.MIN_VALUE
    private var lastVisible = false
    private var sourceWidth = 0
    private var sourceHeight = 0
    private var framesUntilPoll = 0
    /** 마지막으로 관측한 커서 활성 상태 — 폴링 주기(60Hz/2Hz)를 정한다. */
    private var cursorActive = false
    private var tapEcho: TapEchoView? = null

    private val fillPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = 0xF0FFFFFF.toInt()
        style = Paint.Style.FILL
    }
    private val strokePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = 0xFF0F172A.toInt()
        style = Paint.Style.STROKE
        strokeWidth = resources.displayMetrics.density * panelScale * 1.5f
    }
    private val arrow = Path()

    init {
        isClickable = false
        isFocusable = false
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
        elevation = resources.displayMetrics.density * 4f
        visibility = GONE
        host.addView(this, FrameLayout.LayoutParams(cursorWidthPx(), cursorHeightPx()))
    }

    /** Mirrors the letterboxed video rect so the cursor tracks forwarded input. */
    fun setVideoSize(width: Int, height: Int) {
        sourceWidth = width
        sourceHeight = height
    }

    /**
     * 로컬 입력 직후 호출 — 다음 Choreographer 프레임에서 즉시 폴링하게 한다.
     * 유휴(2Hz) 주기 한 바퀴(최대 ~500ms)를 기다리지 않고 커서가 입력 반응을
     * 바로 따라붙게 하는 것이 목적이다. 메인 스레드 입력 경로에서만 부른다.
     */
    fun nudge() {
        if (running) framesUntilPoll = 0
    }

    /**
     * 터치/클릭 즉각 피드백 — 호스트 왕복(LCD1)을 기다리지 않고 터치 지점에
     * 짧게 퍼지는 물결을 그린다. 원격 커서 표시와 무관하게 입력이 살아 있음을
     * 첫 프레임 안에 보여 준다. 좌표는 창 원점 기준(rawX/rawY).
     */
    fun showTapEcho(rawX: Float, rawY: Float) {
        if (!running) return
        val view = tapEcho ?: TapEchoView(activity).also { created ->
            host.addView(
                created,
                FrameLayout.LayoutParams(dp(TAP_ECHO_SIZE_DP), dp(TAP_ECHO_SIZE_DP)),
            )
            tapEcho = created
        }
        view.reveal(rawX, rawY)
    }

    private val frameCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            if (!running) return
            if (activity.isFinishing || activity.isDestroyed) {
                stop()
                return
            }
            // The overlay itself keeps its last position every frame; only
            // the JNI sample poll is frame-skipped (see Q9a constants).
            if (framesUntilPoll-- <= 0) {
                framesUntilPoll =
                    if (cursorActive) ACTIVE_POLL_EVERY_N_FRAMES else IDLE_POLL_EVERY_N_FRAMES
                val packed = ViewerNative.cursorState(instanceId)
                cursorActive = packed != -1L && packed < 0
                applyState(packed)
            }
            choreographer.postFrameCallback(this)
        }
    }

    fun start() {
        showHostWindow()
        if (running) return
        running = true
        // Poll immediately on (re)start so a cursor that was already live is
        // not held back by a full idle skip window.
        framesUntilPoll = 0
        choreographer.postFrameCallback(frameCallback)
    }

    fun stop() {
        running = false
        choreographer.removeFrameCallback(frameCallback)
        popup?.dismiss()
        popup = null
        tapEcho = null
        hide()
    }

    /**
     * A dismissed PopupWindow cannot reshow, so each start wraps the reused
     * host frame in a fresh popup — same pattern as PersistentFpsOverlay.
     */
    private fun showHostWindow() {
        if (popup?.isShowing == true) return
        val nextPopup = PopupWindow(
            host,
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.MATCH_PARENT,
            false,
        ).apply {
            isTouchable = false
            isFocusable = false
            isOutsideTouchable = false
            setBackgroundDrawable(ColorDrawable(android.graphics.Color.TRANSPARENT))
            animationStyle = 0
        }
        popup = nextPopup
        val decor = activity.window.decorView
        decor.post {
            if (popup === nextPopup && !nextPopup.isShowing &&
                !activity.isFinishing && !activity.isDestroyed
            ) {
                nextPopup.showAtLocation(decor, Gravity.TOP or Gravity.START, 0, 0)
            }
        }
    }

    private fun applyState(packed: Long) {
        if (packed == -1L) {
            hide()
            return
        }
        val visible = packed < 0 // sign bit 63
        if (!visible) {
            hide()
            return
        }
        val x = (packed and 0xffffL).toInt()
        val y = ((packed ushr 16) and 0xffffL).toInt()
        val sequence = (packed ushr 32) and 0x3fff_ffffL
        if (lastVisible && sequence == lastSequence) return
        val parent = parent as? ViewGroup ?: return
        // The popup needs one traversal before its host frame has real
        // bounds; retry next frame instead of pinning the cursor at (0, 0).
        if (parent.width == 0 || parent.height == 0) return
        val video = videoRect(parent) ?: return
        lastSequence = sequence
        lastVisible = true
        visibility = VISIBLE
        // A video rect narrower than the cursor must not push the arrow
        // backwards past the rect's leading edge.
        translationX = video.left + x / 65535f *
            (video.width().coerceAtLeast(cursorWidthPx().toFloat()) - cursorWidthPx())
        translationY = video.top + y / 65535f *
            (video.height().coerceAtLeast(cursorHeightPx().toFloat()) - cursorHeightPx())
    }

    /** Centered aspect-fit rect of the source video inside the host frame. */
    private fun videoRect(parent: ViewGroup): RectF? {
        if (sourceWidth <= 0 || sourceHeight <= 0) return null
        val scale = min(
            parent.width / sourceWidth.toFloat(),
            parent.height / sourceHeight.toFloat(),
        )
        val width = sourceWidth * scale
        val height = sourceHeight * scale
        return RectF(
            (parent.width - width) / 2f,
            (parent.height - height) / 2f,
            (parent.width + width) / 2f,
            (parent.height + height) / 2f,
        )
    }

    private fun hide() {
        if (!lastVisible) return
        lastVisible = false
        visibility = GONE
    }

    private fun cursorWidthPx(): Int = dp(CURSOR_WIDTH_DP)

    private fun cursorHeightPx(): Int = dp(CURSOR_HEIGHT_DP)

    private fun dp(value: Int): Int =
        StreamPanelDensity.dp(value.toFloat(), resources.displayMetrics.density, panelScale)

    override fun onDraw(canvas: Canvas) {
        // 크기(dp)와 동일한 panelScale로 화살표 경로를 그려야 뷰에 맞는다.
        val density = resources.displayMetrics.density * panelScale
        arrow.rewind()
        arrow.moveTo(2f * density, 2f * density)
        arrow.lineTo(2f * density, 18f * density)
        arrow.lineTo(6.5f * density, 14f * density)
        arrow.lineTo(9.5f * density, 20.5f * density)
        arrow.lineTo(12f * density, 19.2f * density)
        arrow.lineTo(9f * density, 12.8f * density)
        arrow.lineTo(14.5f * density, 12.5f * density)
        arrow.close()
        canvas.drawPath(arrow, fillPaint)
        canvas.drawPath(arrow, strokePaint)
    }

    /**
     * 탭 에코 — 중심에서 퍼지며 사라지는 원. 팝업 호스트 프레임에 얹히는
     * 단독 뷰라 스트림 SurfaceView 위에도 그려진다(커서 오버레이와 같은 경로).
     */
    private class TapEchoView(context: android.content.Context) : View(context) {
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = 0xFFFFFFFF.toInt()
            style = Paint.Style.STROKE
            strokeWidth = resources.displayMetrics.density * 2.5f
        }
        private val fillPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = 0x33FFFFFF
            style = Paint.Style.FILL
        }
        private var progress = 1f
        private val animator = android.animation.ValueAnimator.ofFloat(0f, 1f).apply {
            duration = TAP_ECHO_DURATION_MS
            interpolator = android.view.animation.DecelerateInterpolator()
            addUpdateListener { animation ->
                progress = animation.animatedValue as Float
                invalidate()
            }
            addListener(object : android.animation.AnimatorListenerAdapter() {
                override fun onAnimationEnd(animation: android.animation.Animator) {
                    visibility = GONE
                }
            })
        }

        init {
            isClickable = false
            isFocusable = false
            importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
            visibility = GONE
        }

        fun reveal(rawX: Float, rawY: Float) {
            translationX = rawX - layoutParams.width / 2f
            translationY = rawY - layoutParams.height / 2f
            progress = 0f
            visibility = VISIBLE
            animator.cancel()
            animator.start()
        }

        override fun onDraw(canvas: Canvas) {
            if (progress >= 1f) return
            val cx = width / 2f
            val cy = height / 2f
            val radius = width * (0.18f + 0.32f * progress)
            paint.alpha = ((1f - progress) * 220).toInt()
            canvas.drawCircle(cx, cy, radius, fillPaint)
            canvas.drawCircle(cx, cy, radius, paint)
        }
    }
}
