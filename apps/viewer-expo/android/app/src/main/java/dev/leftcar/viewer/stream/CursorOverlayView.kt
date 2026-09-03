package dev.leftcar.viewer.stream

import android.app.Activity
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
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
    }

    private val choreographer = Choreographer.getInstance()
    private val host = FrameLayout(activity)
    private var popup: PopupWindow? = null
    private var running = false
    private var lastSequence = Long.MIN_VALUE
    private var lastVisible = false
    private var sourceWidth = 0
    private var sourceHeight = 0

    private val fillPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = 0xF0FFFFFF.toInt()
        style = Paint.Style.FILL
    }
    private val strokePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = 0xFF0F172A.toInt()
        style = Paint.Style.STROKE
        strokeWidth = resources.displayMetrics.density * 1.5f
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

    private val frameCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            if (!running) return
            if (activity.isFinishing || activity.isDestroyed) {
                stop()
                return
            }
            applyState(ViewerNative.cursorState(instanceId))
            choreographer.postFrameCallback(this)
        }
    }

    fun start() {
        showHostWindow()
        if (running) return
        running = true
        choreographer.postFrameCallback(frameCallback)
    }

    fun stop() {
        running = false
        choreographer.removeFrameCallback(frameCallback)
        popup?.dismiss()
        popup = null
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
        translationX = video.first + x / 65535f * (video.third - cursorWidthPx())
        translationY = video.second + y / 65535f * (video.fourth - cursorHeightPx())
    }

    /** Centered aspect-fit rect (offsetX, offsetY, width, height) in the host frame. */
    private fun videoRect(parent: ViewGroup): QuadF? {
        if (sourceWidth <= 0 || sourceHeight <= 0) return null
        val scale = min(
            parent.width / sourceWidth.toFloat(),
            parent.height / sourceHeight.toFloat(),
        )
        val width = sourceWidth * scale
        val height = sourceHeight * scale
        return QuadF(
            (parent.width - width) / 2f,
            (parent.height - height) / 2f,
            width,
            height,
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
        (value * resources.displayMetrics.density).toInt().coerceAtLeast(1)

    override fun onDraw(canvas: Canvas) {
        val density = resources.displayMetrics.density
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
}

private data class QuadF(val first: Float, val second: Float, val third: Float, val fourth: Float)
