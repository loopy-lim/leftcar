package dev.leftcar.viewer.stream

enum class InputOwner {
    LOCAL_ANDROID,
    ACQUIRING_REMOTE,
    REMOTE_MAC,
}

enum class InputOwnershipEffect {
    ACQUIRE_KEYBRIDGE,
    REQUEST_POINTER_CAPTURE,
    HIDE_LOCAL_CURSOR,
    SHOW_REMOTE_CURSOR,
    FORWARD_POINTER,
    RELEASE_REMOTE_INPUT,
    RELEASE_POINTER_CAPTURE,
    RELEASE_KEYBRIDGE,
    HIDE_REMOTE_CURSOR,
    SHOW_LOCAL_CURSOR,
}

enum class KeyRoute {
    LOCAL_ANDROID,
    REMOTE_MAC,
    RELEASE_REMOTE,
}

data class PointerPosition(val x: Float, val y: Float)

sealed interface InputOwnershipEvent {
    data class MouseActivation(val x: Float, val y: Float) : InputOwnershipEvent

    data class PointerCaptureChanged(val captured: Boolean) : InputOwnershipEvent

    data class CapturedMove(
        val dx: Float,
        val dy: Float,
        val width: Int,
        val height: Int,
    ) : InputOwnershipEvent

    data object RemoteAcquireFailed : InputOwnershipEvent
    data object Escape : InputOwnershipEvent
    data object FocusLost : InputOwnershipEvent
    data object HostInputDisabled : InputOwnershipEvent
    data object SurfaceLost : InputOwnershipEvent
    data object Disconnected : InputOwnershipEvent
}

/**
 * Owns the Android ↔ remote input handoff independently from Activity and Binder
 * lifecycles. The first physical mouse click only starts acquisition; input is
 * remote only after Android confirms Pointer Capture.
 */
class InputOwnershipController {
    var owner: InputOwner = InputOwner.LOCAL_ANDROID
        private set

    var pointerPosition: PointerPosition = PointerPosition(0.5f, 0.5f)
        private set

    private val localKeySequences = HashSet<Int>()
    private val remoteKeySequences = HashSet<Int>()

    fun on(event: InputOwnershipEvent): List<InputOwnershipEffect> = when (event) {
        is InputOwnershipEvent.MouseActivation -> activate(event)
        is InputOwnershipEvent.PointerCaptureChanged -> pointerCaptureChanged(event.captured)
        is InputOwnershipEvent.CapturedMove -> moveCapturedPointer(event)
        InputOwnershipEvent.RemoteAcquireFailed,
        InputOwnershipEvent.Escape,
        InputOwnershipEvent.FocusLost,
        InputOwnershipEvent.HostInputDisabled,
        InputOwnershipEvent.SurfaceLost,
        InputOwnershipEvent.Disconnected,
        -> releaseEffects()
    }

    fun routePhysicalKey(keyCode: Int, down: Boolean): KeyRoute {
        if (down) {
            if (keyCode in localKeySequences) return KeyRoute.LOCAL_ANDROID
            if (keyCode in remoteKeySequences) return KeyRoute.REMOTE_MAC
            if (owner == InputOwner.REMOTE_MAC && keyCode == ESCAPE_KEY_CODE) {
                return KeyRoute.RELEASE_REMOTE
            }
            return if (owner == InputOwner.REMOTE_MAC) {
                remoteKeySequences.add(keyCode)
                KeyRoute.REMOTE_MAC
            } else {
                localKeySequences.add(keyCode)
                KeyRoute.LOCAL_ANDROID
            }
        }
        if (localKeySequences.remove(keyCode)) return KeyRoute.LOCAL_ANDROID
        if (remoteKeySequences.remove(keyCode)) return KeyRoute.REMOTE_MAC
        return if (owner == InputOwner.REMOTE_MAC) KeyRoute.REMOTE_MAC else KeyRoute.LOCAL_ANDROID
    }

    private fun activate(event: InputOwnershipEvent.MouseActivation): List<InputOwnershipEffect> {
        if (owner != InputOwner.LOCAL_ANDROID) return emptyList()
        pointerPosition = PointerPosition(event.x.coerceIn(0f, 1f), event.y.coerceIn(0f, 1f))
        owner = InputOwner.ACQUIRING_REMOTE
        return listOf(
            InputOwnershipEffect.ACQUIRE_KEYBRIDGE,
            InputOwnershipEffect.REQUEST_POINTER_CAPTURE,
        )
    }

    private fun pointerCaptureChanged(captured: Boolean): List<InputOwnershipEffect> {
        if (captured && owner == InputOwner.ACQUIRING_REMOTE) {
            owner = InputOwner.REMOTE_MAC
            return listOf(
                InputOwnershipEffect.HIDE_LOCAL_CURSOR,
                InputOwnershipEffect.SHOW_REMOTE_CURSOR,
            )
        }
        if (!captured && owner != InputOwner.LOCAL_ANDROID) return releaseEffects()
        return emptyList()
    }

    private fun moveCapturedPointer(event: InputOwnershipEvent.CapturedMove): List<InputOwnershipEffect> {
        if (owner != InputOwner.REMOTE_MAC || event.width <= 0 || event.height <= 0) return emptyList()
        val current = pointerPosition
        val leavesAtEdge =
            (current.x <= 0f && event.dx < 0f) ||
                (current.x >= 1f && event.dx > 0f) ||
                (current.y <= 0f && event.dy < 0f) ||
                (current.y >= 1f && event.dy > 0f)
        if (leavesAtEdge) return releaseEffects()

        pointerPosition = PointerPosition(
            (current.x + event.dx / event.width).coerceIn(0f, 1f),
            (current.y + event.dy / event.height).coerceIn(0f, 1f),
        )
        return listOf(InputOwnershipEffect.FORWARD_POINTER)
    }

    private fun releaseEffects(): List<InputOwnershipEffect> {
        if (owner == InputOwner.LOCAL_ANDROID) return emptyList()
        owner = InputOwner.LOCAL_ANDROID
        return listOf(
            InputOwnershipEffect.RELEASE_REMOTE_INPUT,
            InputOwnershipEffect.RELEASE_POINTER_CAPTURE,
            InputOwnershipEffect.RELEASE_KEYBRIDGE,
            InputOwnershipEffect.HIDE_REMOTE_CURSOR,
            InputOwnershipEffect.SHOW_LOCAL_CURSOR,
        )
    }

    private companion object {
        const val ESCAPE_KEY_CODE = 111
    }
}
