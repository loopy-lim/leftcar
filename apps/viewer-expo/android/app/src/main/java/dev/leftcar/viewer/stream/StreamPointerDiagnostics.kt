package dev.leftcar.viewer.stream

import android.os.Build
import android.util.Log
import android.view.KeyEvent
import android.view.MotionEvent

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
        Log.i("LeftcarLanguageKey", jsonObject(listOf(
            "keyCode" to event.keyCode,
            "scanCode" to event.scanCode,
            "action" to event.action,
            "metaState" to event.metaState,
            "repeat" to event.repeatCount,
            "deviceId" to event.deviceId,
        )))
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
        val data = mutableListOf<Pair<String, Number>>(
            "schema" to 1,
            "deviceId" to event.deviceId,
            "source" to event.source,
            "action" to event.actionMasked,
            "button" to event.actionButton,
            "buttons" to event.buttonState,
            "classification" to if (Build.VERSION.SDK_INT >= 29) event.classification else 0,
            "historySize" to event.historySize,
            "pointerCount" to event.pointerCount,
            "toolType" to event.getToolType(0),
            "horizontalWheel" to event.getAxisValue(MotionEvent.AXIS_HSCROLL).toDouble(),
            "verticalWheel" to event.getAxisValue(MotionEvent.AXIS_VSCROLL).toDouble(),
        )
        if (Build.VERSION.SDK_INT >= 34) {
            data += "horizontalGesture" to
                event.getAxisValue(MotionEvent.AXIS_GESTURE_SCROLL_X_DISTANCE).toDouble()
            data += "verticalGesture" to
                event.getAxisValue(MotionEvent.AXIS_GESTURE_SCROLL_Y_DISTANCE).toDouble()
        }
        if (classifiedScroll) {
            val previous = previousGesturePosition.takeIf { previousDeviceId == event.deviceId }
            if (event.actionMasked == MotionEvent.ACTION_MOVE && previous != null) {
                data += "gesturePositionDeltaX" to (event.x - previous.first).toDouble()
                data += "gesturePositionDeltaY" to (event.y - previous.second).toDouble()
            }
            previousGesturePosition = event.x to event.y
            previousDeviceId = event.deviceId
        } else {
            previousGesturePosition = null
            previousDeviceId = null
        }
        Log.i(TAG, jsonObject(data))
    }

    private fun jsonObject(fields: List<Pair<String, Number>>): String =
        fields.joinToString(separator = ",", prefix = "{", postfix = "}") { (key, value) ->
            "\"$key\":$value"
        }
}
