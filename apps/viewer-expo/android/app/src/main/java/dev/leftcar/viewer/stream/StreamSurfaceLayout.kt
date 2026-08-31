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
) {
    val holders: List<SurfaceHolder>
        get() = listOfNotNull(left.holder, right?.holder)

    fun allValid(): Boolean = holders.all { it.surface.isValid }

    fun requestFocus() = left.requestFocus()

    fun normalizedX(eventX: Float, source: View): Float {
        val local = eventX / source.width.coerceAtLeast(1).toFloat()
        return if (right == null) {
            local.coerceIn(0f, 1f)
        } else {
            ((if (source === right) 0.5f else 0f) + local * 0.5f).coerceIn(0f, 1f)
        }
    }
}

internal fun createStreamSurfaces(
    activity: Activity,
    sourceWidth: Int,
    sourceHeight: Int,
    splitVertical: Boolean,
    callback: SurfaceHolder.Callback,
    pointerHandler: (View, android.view.MotionEvent) -> Boolean,
): StreamSurfaces {
    fun surface(): SurfaceView = SurfaceView(activity).apply {
        setBackgroundColor(Color.BLACK)
        isFocusable = true
        isFocusableInTouchMode = true
        setZOrderOnTop(true)
        pointerIcon = PointerIcon.getSystemIcon(activity, PointerIcon.TYPE_NULL)
        setOnGenericMotionListener(pointerHandler)
        setOnTouchListener { view, event ->
            if (event.actionMasked == android.view.MotionEvent.ACTION_DOWN) view.requestFocus()
            pointerHandler(view, event)
        }
        holder.setSizeFromLayout()
        holder.setFormat(PixelFormat.OPAQUE)
        holder.addCallback(callback)
        holder.setKeepScreenOn(true)
    }

    if (!splitVertical) {
        val left = AspectRatioSurfaceView(activity).apply {
            setVideoSize(sourceWidth, sourceHeight)
        }
        configureSurface(left, activity, callback, pointerHandler)
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
        return StreamSurfaces(root, left, null)
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
    return StreamSurfaces(root, left, right)
}

private fun configureSurface(
    surface: SurfaceView,
    activity: Activity,
    callback: SurfaceHolder.Callback,
    pointerHandler: (View, android.view.MotionEvent) -> Boolean,
) {
    surface.setBackgroundColor(Color.BLACK)
    surface.isFocusable = true
    surface.isFocusableInTouchMode = true
    surface.setZOrderOnTop(true)
    surface.pointerIcon = PointerIcon.getSystemIcon(activity, PointerIcon.TYPE_NULL)
    surface.setOnGenericMotionListener(pointerHandler)
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
