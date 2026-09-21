package dev.leftcar.viewer.stream

import android.app.Activity
import android.graphics.Color
import android.graphics.PixelFormat
import android.view.Gravity
import android.view.PointerIcon
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.widget.FrameLayout
import android.widget.LinearLayout

internal data class StreamSurfaces(
    val root: View,
    val left: SurfaceView,
    val right: SurfaceView?,
    private val videoSizeUpdater: (Int, Int) -> Unit = { _, _ -> },
) {
    fun updateVideoSize(width: Int, height: Int) {
        videoSizeUpdater(width, height)
        root.requestLayout()
    }
    val holders: List<SurfaceHolder>
        get() = listOfNotNull(left.holder, right?.holder)

    fun allValid(): Boolean = holders.all { it.surface.isValid }

    fun requestFocus() = left.requestFocus()
}

internal fun createStreamSurfaces(
    activity: Activity,
    sourceWidth: Int,
    sourceHeight: Int,
    splitVertical: Boolean,
    callback: SurfaceHolder.Callback,
    pointerHandler: (View, android.view.MotionEvent) -> Boolean,
    capturedPointerHandler: (View, android.view.MotionEvent) -> Boolean,
): StreamSurfaces {
    fun surface(): SurfaceView = SurfaceView(activity).also {
        configureSurface(it, activity, callback, pointerHandler, capturedPointerHandler)
    }

    if (!splitVertical) {
        val left = AspectRatioSurfaceView(activity).apply {
            setVideoSize(sourceWidth, sourceHeight)
        }
        configureSurface(left, activity, callback, pointerHandler, capturedPointerHandler)
        val root = FrameLayout(activity).apply {
            setBackgroundColor(Color.BLACK)
            addView(
                left,
                FrameLayout.LayoutParams(
                    FrameLayout.LayoutParams.WRAP_CONTENT,
                    FrameLayout.LayoutParams.WRAP_CONTENT,
                    Gravity.CENTER,
                ),
            )
        }
        return StreamSurfaces(root, left, null) { width, height -> left.setVideoSize(width, height) }
    }

    val left = surface()
    val right = surface()
    val row = LinearLayout(activity).apply {
        orientation = LinearLayout.HORIZONTAL
        setBackgroundColor(Color.BLACK)
        addView(left, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.MATCH_PARENT, 1f))
        addView(right, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.MATCH_PARENT, 1f))
    }
    val content = AspectRatioFrameLayout(activity).apply {
        setVideoSize(sourceWidth, sourceHeight)
        setBackgroundColor(Color.BLACK)
        addView(
            row,
            FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.MATCH_PARENT,
                Gravity.CENTER,
            ),
        )
    }
    val root = FrameLayout(activity).apply {
        setBackgroundColor(Color.BLACK)
        addView(
            content,
            FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.WRAP_CONTENT,
                FrameLayout.LayoutParams.WRAP_CONTENT,
                Gravity.CENTER,
            ),
        )
    }
    return StreamSurfaces(root, left, right) { width, height -> content.setVideoSize(width, height) }
}

private fun configureSurface(
    surface: SurfaceView,
    activity: Activity,
    callback: SurfaceHolder.Callback,
    pointerHandler: (View, android.view.MotionEvent) -> Boolean,
    capturedPointerHandler: (View, android.view.MotionEvent) -> Boolean,
) {
    surface.setBackgroundColor(Color.BLACK)
    surface.isFocusable = true
    surface.isFocusableInTouchMode = true
    surface.setZOrderOnTop(true)
    surface.pointerIcon = PointerIcon.getSystemIcon(activity, PointerIcon.TYPE_ARROW)
    surface.setOnGenericMotionListener(pointerHandler)
    if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.O) {
        surface.setOnCapturedPointerListener(capturedPointerHandler)
    }
    surface.setOnTouchListener { view, event ->
        if (event.actionMasked == android.view.MotionEvent.ACTION_DOWN) view.requestFocus()
        pointerHandler(view, event)
    }
    surface.holder.setSizeFromLayout()
    surface.holder.setFormat(PixelFormat.OPAQUE)
    surface.holder.addCallback(callback)
    surface.holder.setKeepScreenOn(true)
}

private open class AspectRatioFrameLayout(context: android.content.Context) : FrameLayout(context) {
    private var videoWidth = 16
    private var videoHeight = 9

    fun setVideoSize(width: Int, height: Int) {
        if (width > 0 && height > 0) {
            videoWidth = width
            videoHeight = height
            requestLayout()
        }
    }

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val maxWidth = MeasureSpec.getSize(widthMeasureSpec)
        val maxHeight = MeasureSpec.getSize(heightMeasureSpec)
        if (maxWidth == 0 || maxHeight == 0) {
            super.onMeasure(widthMeasureSpec, heightMeasureSpec)
            return
        }
        val aspect = videoWidth.toDouble() / videoHeight.toDouble()
        var width = maxWidth
        var height = (width / aspect).toInt()
        if (height > maxHeight) {
            height = maxHeight
            width = (height * aspect).toInt()
        }
        super.onMeasure(
            MeasureSpec.makeMeasureSpec(width.coerceAtLeast(1), MeasureSpec.EXACTLY),
            MeasureSpec.makeMeasureSpec(height.coerceAtLeast(1), MeasureSpec.EXACTLY),
        )
    }
}

private class AspectRatioSurfaceView(context: android.content.Context) : SurfaceView(context) {
    private var videoWidth = 16
    private var videoHeight = 9

    fun setVideoSize(width: Int, height: Int) {
        if (width > 0 && height > 0) {
            videoWidth = width
            videoHeight = height
            requestLayout()
        }
    }

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val maxWidth = MeasureSpec.getSize(widthMeasureSpec)
        val maxHeight = MeasureSpec.getSize(heightMeasureSpec)
        if (maxWidth == 0 || maxHeight == 0) {
            super.onMeasure(widthMeasureSpec, heightMeasureSpec)
            return
        }
        val aspect = videoWidth.toDouble() / videoHeight.toDouble()
        var width = maxWidth
        var height = (width / aspect).toInt()
        if (height > maxHeight) {
            height = maxHeight
            width = (height * aspect).toInt()
        }
        setMeasuredDimension(width.coerceAtLeast(1), height.coerceAtLeast(1))
    }
}
