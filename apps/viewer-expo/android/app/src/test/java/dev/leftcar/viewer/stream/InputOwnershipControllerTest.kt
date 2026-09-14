package dev.leftcar.viewer.stream

import org.junit.Assert.assertEquals
import org.junit.Test

class InputOwnershipControllerTest {
    @Test
    fun `first click acquires without forwarding and capture confirms remote`() {
        val controller = InputOwnershipController()

        assertEquals(
            listOf(
                InputOwnershipEffect.ACQUIRE_KEYBRIDGE,
                InputOwnershipEffect.REQUEST_POINTER_CAPTURE,
            ),
            controller.on(InputOwnershipEvent.MouseActivation(0.5f, 0.5f)),
        )
        assertEquals(InputOwner.ACQUIRING_REMOTE, controller.owner)

        assertEquals(
            listOf(
                InputOwnershipEffect.HIDE_LOCAL_CURSOR,
                InputOwnershipEffect.SHOW_REMOTE_CURSOR,
            ),
            controller.on(InputOwnershipEvent.PointerCaptureChanged(true)),
        )
        assertEquals(InputOwner.REMOTE_MAC, controller.owner)
    }

    @Test
    fun `captured motion accumulates normalized absolute coordinates`() {
        val controller = remoteOwnerAt(x = 0.5f, y = 0.25f)

        assertEquals(
            listOf(InputOwnershipEffect.FORWARD_POINTER),
            controller.on(
                InputOwnershipEvent.CapturedMove(
                    dx = 100f,
                    dy = -50f,
                    width = 1_000,
                    height = 500,
                ),
            ),
        )
        assertEquals(PointerPosition(0.6f, 0.15f), controller.pointerPosition)
    }

    @Test
    fun `outward motion at host edge releases remote first`() {
        val controller = remoteOwnerAt(x = 1f, y = 0.5f)

        val effects = controller.on(
            InputOwnershipEvent.CapturedMove(
                dx = 3f,
                dy = 0f,
                width = 1_000,
                height = 1_000,
            ),
        )

        assertEquals(InputOwnershipEffect.RELEASE_REMOTE_INPUT, effects.first())
        assertEquals(InputOwner.LOCAL_ANDROID, controller.owner)
    }

    @Test
    fun `all terminal events release remote input before local resources`() {
        val events = listOf(
            InputOwnershipEvent.Escape,
            InputOwnershipEvent.FocusLost,
            InputOwnershipEvent.HostInputDisabled,
            InputOwnershipEvent.SurfaceLost,
            InputOwnershipEvent.Disconnected,
        )

        for (event in events) {
            val controller = remoteOwnerAt()
            val effects = controller.on(event)
            assertEquals(event.toString(), InputOwnershipEffect.RELEASE_REMOTE_INPUT, effects.first())
            assertEquals(event.toString(), InputOwner.LOCAL_ANDROID, controller.owner)
        }
    }

    @Test
    fun `physical keys stay local until pointer capture confirms remote`() {
        val controller = InputOwnershipController()
        assertEquals(KeyRoute.LOCAL_ANDROID, controller.routePhysicalKey(keyCode = 29, down = true))

        controller.on(InputOwnershipEvent.MouseActivation(0.5f, 0.5f))
        assertEquals(KeyRoute.LOCAL_ANDROID, controller.routePhysicalKey(keyCode = 30, down = true))

        controller.on(InputOwnershipEvent.PointerCaptureChanged(true))
        assertEquals(KeyRoute.REMOTE_MAC, controller.routePhysicalKey(keyCode = 31, down = true))
        assertEquals(KeyRoute.RELEASE_REMOTE, controller.routePhysicalKey(keyCode = 111, down = true))
    }

    @Test
    fun `key sequences remain with their down owner across handoff`() {
        val controller = InputOwnershipController()
        assertEquals(KeyRoute.LOCAL_ANDROID, controller.routePhysicalKey(keyCode = 29, down = true))
        controller.on(InputOwnershipEvent.MouseActivation(0.5f, 0.5f))
        controller.on(InputOwnershipEvent.PointerCaptureChanged(true))
        assertEquals(KeyRoute.LOCAL_ANDROID, controller.routePhysicalKey(keyCode = 29, down = false))

        assertEquals(KeyRoute.REMOTE_MAC, controller.routePhysicalKey(keyCode = 30, down = true))
        controller.on(InputOwnershipEvent.FocusLost)
        assertEquals(KeyRoute.REMOTE_MAC, controller.routePhysicalKey(keyCode = 30, down = false))
    }

    private fun remoteOwnerAt(x: Float = 0.5f, y: Float = 0.5f): InputOwnershipController {
        return InputOwnershipController().apply {
            on(InputOwnershipEvent.MouseActivation(x, y))
            on(InputOwnershipEvent.PointerCaptureChanged(true))
        }
    }
}
