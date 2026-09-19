package dev.leftcar.viewer.stream

import android.content.Context
import android.database.ContentObserver
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.view.inputmethod.InputMethodManager

/** Actual IME language wins over the physical keyboard's layout hint. */
internal fun streamInputLanguage(tag: String, locale: String, physicalHint: String): Int? {
    val selected = listOf(tag, locale, physicalHint).firstOrNull { it.isNotBlank() } ?: return null
    return when (selected.substringBefore('-').substringBefore('_').lowercase()) {
        "en" -> 1
        "ko" -> 2
        else -> null
    }
}

internal class StreamInputLanguageSync {
    private var sent: Int? = null
    fun reset() { sent = null }
    fun update(language: Int?, active: Boolean, send: (Int) -> Boolean): Boolean {
        if (!active || language == null) { reset(); return false }
        if (sent == language) return true
        if (!send(language)) { reset(); return false }
        sent = language
        return true
    }
}

/** Observe switches consumed by Android before Activity.dispatchKeyEvent. */
internal class StreamInputLanguageMonitor(
    private val context: Context,
    private val active: () -> Boolean,
    private val send: (Int) -> Boolean,
) {
    private val handler = Handler(Looper.getMainLooper())
    private val sync = StreamInputLanguageSync()
    private var started = false
    private val observer = object : ContentObserver(handler) {
        override fun onChange(selfChange: Boolean) { refresh() }
    }
    private val poll = object : Runnable {
        override fun run() {
            if (!started) return
            refresh()
            handler.postDelayed(this, 500)
        }
    }
    fun start() {
        if (started) return
        started = true
        for (key in listOf(Settings.Secure.DEFAULT_INPUT_METHOD, "selected_input_method_subtype")) {
            context.contentResolver.registerContentObserver(Settings.Secure.getUriFor(key), false, observer)
        }
        handler.post(poll)
    }
    fun stop() {
        started = false
        handler.removeCallbacks(poll)
        context.contentResolver.unregisterContentObserver(observer)
        sync.reset()
    }
    fun reset() = sync.reset()

    /**
     * 한/영 키를 Android가 소비한 직후에는 subtype 변경이 ContentObserver에
     * 도달하기 전일 수 있다 — 그 상태로 다음 글자가 큐에 들어가면 이전 언어로
     * 나가므로(토글 직후 첫 글자 레이스), 짧은 간격으로 몇 번 더 확인해
     * kind-7이 먼저 나가도록 만든다.
     */
    fun refreshSoon() {
        for (delayMs in longArrayOf(100, 250, 500, 1_000)) {
            handler.postDelayed({ refresh() }, delayMs)
        }
    }

    @Suppress("DEPRECATION")
    fun refresh(): Boolean {
        if (!started || !active()) { sync.reset(); return false }
        val subtype = (context.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager)
            .currentInputMethodSubtype ?: return false
        val hint = if (Build.VERSION.SDK_INT >= 34) subtype.physicalKeyboardHintLanguageTag?.toLanguageTag().orEmpty() else ""
        return sync.update(streamInputLanguage(subtype.languageTag, subtype.locale, hint), true, send)
    }
}
