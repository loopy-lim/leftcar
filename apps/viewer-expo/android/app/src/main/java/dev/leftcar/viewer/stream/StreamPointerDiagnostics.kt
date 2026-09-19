package dev.leftcar.viewer.stream

import android.os.Build
import android.util.Log
import android.view.MotionEvent
import android.view.KeyEvent
import org.json.JSONObject

/** Opt-in, bounded metadata for device input diagnosis; no key text or coordinates. */
internal object StreamPointerDiagnostics {
    private const val TAG = "LeftcarPointer"
    private const val MAX_EVENTS = 128
    private var remaining = MAX_EVENTS
    private var previousGesturePosition: Pair<Float, Float>? = null
    private var previousDeviceId: Int? = null
    private var remainingLanguageKeys = 32

    fun recordLanguageKey(event: KeyEvent) {
        if (!Log.isLoggable("LeftcarLanguageKey", Log.DEBUG)) {
            remainingLanguageKeys = 32
            return
        }
        val relevant = event.keyCode == KeyEvent.KEYCODE_LANGUAGE_SWITCH ||
            event.keyCode == KeyEvent.KEYCODE_CAPS_LOCK ||
            (event.keyCode == KeyEvent.KEYCODE_SPACE && (event.isShiftPressed || event.isCtrlPressed))
        if (!relevant || remainingLanguageKeys == 0) return
        remainingLanguageKeys--
        Log.i("LeftcarLanguageKey", JSONObject().put("keyCode", event.keyCode)
            .put("scanCode", event.scanCode).put("action", event.action)
            .put("metaState", event.metaState).put("repeat", event.repeatCount)
            .put("deviceId", event.deviceId).toString())
    }

    fun record(event: MotionEvent) {
        if (!Log.isLoggable(TAG, Log.DEBUG)) {
            remaining = MAX_EVENTS
            previousGesturePosition = null
            previousDeviceId = null
            return
        }
        if (remaining == 0) return
        val classifiedScroll = Build.VERSION.SDK_INT >= 34 &&
            event.classification == MotionEvent.CLASSIFICATION_TWO_FINGER_SWIPE
        if (!classifiedScroll && event.actionMasked !in intArrayOf(
                MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP,
                MotionEvent.ACTION_BUTTON_PRESS, MotionEvent.ACTION_BUTTON_RELEASE,
                MotionEvent.ACTION_SCROLL,
            )) return
        remaining--
        val data = JSONObject()
            .put("schema", 1).put("deviceId", event.deviceId)
            .put("source", event.source).put("action", event.actionMasked)
            .put("button", event.actionButton).put("buttons", event.buttonState)
            .put("classification", if (Build.VERSION.SDK_INT >= 29) event.classification else 0)
            .put("historySize", event.historySize)
            .put("pointerCount", event.pointerCount)
            .put("toolType", event.getToolType(0))
            .put("horizontalWheel", event.getAxisValue(MotionEvent.AXIS_HSCROLL).toDouble())
            .put("verticalWheel", event.getAxisValue(MotionEvent.AXIS_VSCROLL).toDouble())
        if (Build.VERSION.SDK_INT >= 34) {
            data.put("horizontalGesture", event.getAxisValue(MotionEvent.AXIS_GESTURE_SCROLL_X_DISTANCE).toDouble())
                .put("verticalGesture", event.getAxisValue(MotionEvent.AXIS_GESTURE_SCROLL_Y_DISTANCE).toDouble())
        }
        if (classifiedScroll) {
            val previous = previousGesturePosition.takeIf { previousDeviceId == event.deviceId }
            if (event.actionMasked == MotionEvent.ACTION_MOVE && previous != null) {
                data.put("gesturePositionDeltaX", (event.x - previous.first).toDouble())
                    .put("gesturePositionDeltaY", (event.y - previous.second).toDouble())
            }
            previousGesturePosition = event.x to event.y
            previousDeviceId = event.deviceId
        } else {
            previousGesturePosition = null
            previousDeviceId = null
        }
        Log.i(TAG, data.toString())
    }
}
