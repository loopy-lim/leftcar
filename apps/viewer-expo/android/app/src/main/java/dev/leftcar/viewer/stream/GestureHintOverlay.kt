package dev.leftcar.viewer.stream

import android.app.Activity
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.ColorDrawable
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.PopupWindow
import android.widget.TextView

/**
 * 첫 스트림 창에서 한 번만 보여 주는 제스처 안내. 영상 SurfaceView가
 * setZOrderOnTop이라 Activity 뷰 계층 위로 그릴 수 없어 HUD 배지와 같은
 * PopupWindow로 띄운다. 팝업은 입력을 전혀 받지 않고 8초 뒤 스스로 닫힌다 —
 * 원격 클릭을 삼키는 안내 창은 스트림이 먹통인 것처럼 보이므로(2026-09-17
 * XR 사고). 닫힐 때 [onDismissed]가 호출된다. 문구는 [ViewerStrings] 언어를 따른다.
 */
internal class GestureHintOverlay(
    private val activity: Activity,
    private val onDismissed: () -> Unit,
) {
    companion object {
        const val PREF_SHOWN = "gesture_hint_shown"
    }

    private var popup: PopupWindow? = null

    fun show() {
        if (popup != null || activity.isFinishing || activity.isDestroyed) return
        val panelScale = StreamPanelDensity.scaleOf(activity)
        val content = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(16), dp(20), dp(16))
            background = cardBackground()
        }
        TextView(activity).apply {
            text = ViewerStrings.gestureHintTitle
            setTextColor(Color.WHITE)
            textSize = 15f * panelScale
            typeface = Typeface.DEFAULT_BOLD
            setPadding(0, 0, 0, dp(6))
        }.also(content::addView)
        GestureHintRows.rows(ViewerStrings.language).forEach { (gesture, action) ->
            TextView(activity).apply {
                text = "$gesture — $action"
                setTextColor(Color.argb(224, 255, 255, 255))
                textSize = 12f * panelScale
                setPadding(0, dp(3), 0, dp(3))
            }.also(content::addView)
        }

        val window = PopupWindow(
            content,
            ViewGroup.LayoutParams.WRAP_CONTENT,
            ViewGroup.LayoutParams.WRAP_CONTENT,
            false,
        ).apply {
            isFocusable = false
            // 안내가 떠 있는 동안 원격 클릭을 팝업이 삼키면 스트림이 먹통처럼
            // 보인다(2026-09-17 XR 사고). 외부 터치를 받지 않게 두고, 아래
            // 자동 닫힘으로만 정리한다.
            isOutsideTouchable = false
            isTouchable = false
            setBackgroundDrawable(ColorDrawable(Color.TRANSPARENT))
            elevation = dp(6).toFloat()
            setOnDismissListener {
                popup = null
                autoDismissHandler?.removeCallbacksAndMessages(null)
                onDismissed()
            }
        }
        popup = window
        activity.window.decorView.post {
            if (!activity.isFinishing && !activity.isDestroyed && !window.isShowing) {
                window.showAtLocation(activity.window.decorView, Gravity.CENTER, 0, 0)
                // 입력을 받지 않는 안내는 스스로 닫혀야 한다. 8초 뒤 사라진다.
                autoDismissHandler = android.os.Handler(android.os.Looper.getMainLooper()).also { handler ->
                    handler.postDelayed({ window.dismiss() }, 8_000L)
                }
            }
        }
    }

    private var autoDismissHandler: android.os.Handler? = null

    fun dismiss() {
        popup?.dismiss()
    }

    private fun dp(value: Int): Int =
        StreamPanelDensity.dp(
            value.toFloat(),
            activity.resources.displayMetrics.density,
            StreamPanelDensity.scaleOf(activity),
        )

    private fun cardBackground() = GradientDrawable().apply {
        shape = GradientDrawable.RECTANGLE
        cornerRadius = dp(14).toFloat()
        setColor(Color.argb(236, 15, 23, 42))
        setStroke(dp(1), Color.argb(46, 255, 255, 255))
    }
}
