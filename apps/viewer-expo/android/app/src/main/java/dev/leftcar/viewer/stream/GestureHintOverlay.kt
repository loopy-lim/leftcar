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
 * 스트림 창 터치 제스처 안내 행. 순수 데이터로만 구성해 JVM 단위 테스트가
 * Android 프레임워크 없이 내용을 검증할 수 있게 한다.
 */
internal object GestureHintRows {
    val rows: List<Pair<String, String>> = listOf(
        "탭" to "클릭",
        "끌기" to "드래그",
        "두 손가락으로 밀기" to "스크롤",
        "길게 누르기" to "오른쪽 클릭",
    )
}

/**
 * 첫 스트림 창에서 한 번만 보여 주는 제스처 안내. 영상 SurfaceView가
 * setZOrderOnTop이라 Activity 뷰 계층 위로 그릴 수 없어 HUD 배지와 같은
 * PopupWindow로 띄운다. 닫힐 때 [onDismissed]가 호출되므로 호출자가
 * "다시 보지 않기" 플래그를 저장하면 된다.
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
        val confirmButton = TextView(activity).apply {
            text = "확인"
            setTextColor(Color.argb(255, 140, 188, 255))
            textSize = 13f
            typeface = Typeface.DEFAULT_BOLD
            setPadding(0, dp(10), 0, 0)
        }
        val content = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(16), dp(20), dp(14))
            background = cardBackground()
        }
        TextView(activity).apply {
            text = "터치 제스처"
            setTextColor(Color.WHITE)
            textSize = 15f
            typeface = Typeface.DEFAULT_BOLD
            setPadding(0, 0, 0, dp(6))
        }.also(content::addView)
        GestureHintRows.rows.forEach { (gesture, action) ->
            TextView(activity).apply {
                text = "$gesture — $action"
                setTextColor(Color.argb(224, 255, 255, 255))
                textSize = 12f
                setPadding(0, dp(3), 0, dp(3))
            }.also(content::addView)
        }
        content.addView(confirmButton)

        val window = PopupWindow(
            content,
            ViewGroup.LayoutParams.WRAP_CONTENT,
            ViewGroup.LayoutParams.WRAP_CONTENT,
            false,
        ).apply {
            isFocusable = false
            isOutsideTouchable = true
            setBackgroundDrawable(ColorDrawable(Color.TRANSPARENT))
            elevation = dp(6).toFloat()
            setOnDismissListener {
                popup = null
                onDismissed()
            }
        }
        confirmButton.setOnClickListener { window.dismiss() }
        popup = window
        activity.window.decorView.post {
            if (!activity.isFinishing && !activity.isDestroyed && !window.isShowing) {
                window.showAtLocation(activity.window.decorView, Gravity.CENTER, 0, 0)
            }
        }
    }

    fun dismiss() {
        popup?.dismiss()
    }

    private fun dp(value: Int): Int =
        (value * activity.resources.displayMetrics.density).toInt().coerceAtLeast(1)

    private fun cardBackground() = GradientDrawable().apply {
        shape = GradientDrawable.RECTANGLE
        cornerRadius = dp(14).toFloat()
        setColor(Color.argb(236, 15, 23, 42))
        setStroke(dp(1), Color.argb(46, 255, 255, 255))
    }
}
