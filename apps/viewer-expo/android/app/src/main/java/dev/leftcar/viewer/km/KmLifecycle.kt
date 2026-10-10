package dev.leftcar.viewer.km

import kotlin.math.abs

/** Used by the real accessibility adapter: keyboard filtering never widens mouse ownership. */
internal fun KmCaptureMode.observesMouse(): Boolean = this != KmCaptureMode.OFF

/** Actual runtime transition ordering, separated from Android Binder/Looper for deterministic tests. */
internal class KmOwnerTransition(
    private val neutral: () -> Unit,
    private val ungrab: () -> Unit,
    private val restore: (String) -> Unit,
    private val acknowledge: () -> Unit,
) {
    var waitingForPlacement = false; private set
    fun apply(owner: String, placementPending: Boolean) {
        check(!waitingForPlacement)
        if (owner == "local" && placementPending) { waitingForPlacement = true; return }
        commit(owner)
    }
    fun placementCompleted() {
        if (!waitingForPlacement) return
        commit("local")
        waitingForPlacement = false
    }
    private fun commit(owner: String) { neutral(); ungrab(); restore(owner); acknowledge() }
    fun cancel() { waitingForPlacement = false }
    fun disconnect(close: () -> Unit): Boolean {
        waitingForPlacement = false
        var okay = true
        for (action in listOf(neutral, ungrab, close, { restore("off") })) {
            try { action() } catch (_: Exception) { okay = false }
        }
        return okay
    }
}

/** Shared by the shell helper's real timer, app Binder death and caller guard. */
internal class KmAuthorityLifetime(private val appUid: Int, private val now: () -> Long, private val cleanup: () -> Unit) {
    private var alive = false
    private var heartbeatAt = 0L
    fun authorize(callerUid: Int) { check(callerUid == appUid) { "KM caller denied" } }
    fun prepared() { alive = true; heartbeatAt = now() }
    fun heartbeat() { check(alive) { "KM authority expired" }; heartbeatAt = now() }
    fun tick() { if (alive && now() - heartbeatAt > 1000) disconnected() }
    fun disconnected() { if (alive) { alive = false; cleanup() } }
    fun closed() { alive = false }
}

/** Coordinates are always supplied by observed native pointer events. */
internal class KmCursorPlacement(private val x: Double, private val y: Double, private val started: Long) {
    private var steps = 0
    fun observe(actualX: Double, actualY: Double, now: Long): Pair<Double, Double>? {
        check(now - started <= 1000 && ++steps <= 64) { "Native cursor placement timed out" }
        val dx = x - actualX; val dy = y - actualY
        if (abs(dx) <= 2 && abs(dy) <= 2) return null
        fun step(delta: Double): Double = if (abs(delta) <= 2) 0.0 else
            (delta / 3).coerceIn(-32.0, 32.0).let { if (abs(it) < 1) kotlin.math.sign(it) else it }
        return step(dx) to step(dy)
    }
}

/** Worker-owned, exactly-once completion. Only authenticated, successfully applied State settles success. */
internal class KmStartCompletion<T> {
    private var serial = 0L
    private var callback: ((Result<T>) -> Unit)? = null
    val pending: Boolean get() = callback != null
    fun begin(completion: (Result<T>) -> Unit): Long {
        check(!pending)
        callback = completion
        return ++serial
    }
    fun applied(ticket: Long, value: T) {
        if (ticket == serial) settle(Result.success(value))
    }
    fun failed(reason: String) { settle(Result.failure(IllegalStateException(reason))) }
    private fun settle(result: Result<T>) {
        val completion = callback ?: return
        callback = null
        completion(result)
    }
}

/** Callback routing and capture commitment share this short gate; OS/Binder calls never hold it. */
internal class KmPhysicalTransition {
    private val held = mutableSetOf<Int>()
    private var buttons = 0
    private var modifiers = 0
    private var revision = 0L
    @Volatile var mode = KmCaptureMode.OFF; private set
    @Synchronized fun hasLocalHolds(): Boolean = held.isNotEmpty() || buttons != 0 || modifiers != 0
    @Synchronized fun setMode(next: KmCaptureMode) { check(next != KmCaptureMode.CONSUME); revision++; mode = next }
    @Synchronized fun beginCapture(): Long { check(!hasLocalHolds()); return revision }
    @Synchronized fun commitCapture(ticket: Long, commitOwner: () -> Unit): Boolean {
        if (ticket != revision || hasLocalHolds()) return false
        mode = KmCaptureMode.CONSUME
        commitOwner()
        return true
    }
    @Synchronized fun key(code: Int, down: Boolean, meta: Int, allowed: Boolean = true): Boolean {
        if (mode == KmCaptureMode.CONSUME && allowed) return true
        if (!allowed) mode = KmCaptureMode.OFF
        revision++
        modifiers = meta
        if (down) held.add(code) else held.remove(code)
        return false
    }
    @Synchronized fun pointer(nextButtons: Int, meta: Int) {
        if (buttons != nextButtons || modifiers != meta) revision++
        buttons = nextButtons; modifiers = meta
    }
}
