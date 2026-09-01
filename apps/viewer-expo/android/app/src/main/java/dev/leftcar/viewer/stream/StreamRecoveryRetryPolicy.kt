package dev.leftcar.viewer.stream

internal data class RebindRetryAttempt(
    val number: Int,
    val delayMs: Long,
)

/**
 * Local failures are recoverable without destroying the visible stream window.
 * Host-unreachable is recovered by React/Host, while render-stalled is first
 * retried directly by the native renderer.
 */
internal fun isSameWindowRecoveryReason(reason: Int): Boolean = reason == 4 || reason == 5

/**
 * Short, bounded same-window recovery attempts. A successful rebind resets
 * this budget; if all local attempts fail, the React/Host recovery path gets
 * a chance to recreate the session.
 */
internal class StreamRecoveryRetryPolicy(
    private val delaysMs: LongArray = DEFAULT_DELAYS_MS,
) {
    private var nextIndex = 0

    fun nextAttempt(): RebindRetryAttempt? {
        val delayMs = delaysMs.getOrNull(nextIndex) ?: return null
        nextIndex += 1
        return RebindRetryAttempt(nextIndex, delayMs)
    }

    fun reset() {
        nextIndex = 0
    }

    private companion object {
        val DEFAULT_DELAYS_MS = longArrayOf(0L, 250L, 500L, 1_000L, 2_000L, 4_000L)
    }
}
