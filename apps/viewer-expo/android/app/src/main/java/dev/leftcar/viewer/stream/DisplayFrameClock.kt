package dev.leftcar.viewer.stream

import android.view.Choreographer

/** Main-thread Activity-owned callback. A retired callback never reschedules or publishes. */
internal class DisplayFrameClock(
    private val post: (Choreographer.FrameCallback) -> Unit,
    private val remove: (Choreographer.FrameCallback) -> Unit,
    private val display: () -> Pair<Int, Float>?,
    private val deliver: (Int, Long, Long) -> Unit,
) {
    private var generation = 0L
    private var callback: Choreographer.FrameCallback? = null

    fun start() {
        stop()
        val epoch = generation
        val next = object : Choreographer.FrameCallback {
            override fun doFrame(frameTimeNanos: Long) {
                if (generation != epoch || callback !== this) return
                val screen = display()
                val rate = screen?.second
                if (screen != null && rate != null && rate.isFinite() && rate in 20f..250f) {
                    deliver(screen.first, frameTimeNanos, (1_000_000_000.0 / rate).toLong())
                } else {
                    deliver(-1, 0, 0)
                }
                if (generation == epoch && callback === this) post(this)
            }
        }
        callback = next
        post(next)
    }

    fun stop() {
        generation++
        callback?.let(remove)
        callback = null
    }
}
