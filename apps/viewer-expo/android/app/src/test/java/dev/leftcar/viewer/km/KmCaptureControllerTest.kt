package dev.leftcar.viewer.km

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** 캡처 게이트: 기본 OFF, 다섯 게이트 동시 충족, fail-closed 해제 규율. */
class KmCaptureControllerTest {
    private fun readyController(api: Int = 35): KmCaptureController {
        val controller = KmCaptureController(apiLevel = api)
        controller.onConnectionAuthenticated(true)
        controller.onOwnershipActive(true)
        controller.onSessionConsent(true)
        return controller
    }

    @Test
    fun `default state is off and emits nothing`() {
        val controller = KmCaptureController(apiLevel = 35)
        assertFalse(controller.captureActive)
        assertEquals(0, controller.desiredMask())
        assertEquals(KmServiceCapability.NOT_ATTACHED, controller.serviceCapability)
    }

    @Test
    fun `all gates open applies mouse family mask once`() {
        val controller = readyController()
        val effects = controller.onServiceCapability(KmServiceCapability.ATTACHED_READY)
        assertEquals(
            listOf(KmCaptureEffect.ApplySources(KmMotionSources.MOUSE_FAMILY)),
            effects,
        )
        assertTrue(controller.captureActive)
        assertEquals(KmMotionSources.MOUSE_FAMILY, controller.desiredMask())

        // 재통지는 멱등: 변화 없으면 효과 없음.
        assertEquals(
            emptyList<KmCaptureEffect>(),
            controller.onServiceCapability(KmServiceCapability.ATTACHED_READY),
        )
    }

    @Test
    fun `each missing gate keeps mask zero`() {
        fun maskFor(build: (KmCaptureController) -> Unit): Int {
            val controller = KmCaptureController(apiLevel = 35)
            build(controller)
            return controller.desiredMask()
        }
        assertEquals(0, maskFor { it.onOwnershipActive(true) })
        assertEquals(0, maskFor { it.onSessionConsent(true) })
        assertEquals(0, maskFor { it.onConnectionAuthenticated(true) })
        assertEquals(
            0,
            maskFor {
                it.onConnectionAuthenticated(true)
                it.onOwnershipActive(true)
                it.onSessionConsent(true)
            },
        )
        val belowApi = KmCaptureController(apiLevel = 33)
        belowApi.onConnectionAuthenticated(true)
        belowApi.onOwnershipActive(true)
        belowApi.onSessionConsent(true)
        belowApi.onServiceCapability(KmServiceCapability.ATTACHED_READY)
        assertEquals(0, belowApi.desiredMask())
        assertFalse(belowApi.captureActive)
    }

    @Test
    fun `below api 34 never activates even with every gate satisfied`() {
        val controller = KmCaptureController(apiLevel = 33)
        controller.onConnectionAuthenticated(true)
        controller.onOwnershipActive(true)
        controller.onSessionConsent(true)
        val effects = controller.onServiceCapability(KmServiceCapability.ATTACHED_READY)
        assertEquals(emptyList<KmCaptureEffect>(), effects)
        assertFalse(controller.captureActive)
    }

    @Test
    fun `consent revoke releases sources and fresh consent re-arms`() {
        val controller = KmCaptureController(apiLevel = 35)
        controller.onConnectionAuthenticated(true)
        controller.onOwnershipActive(true)
        controller.onSessionConsent(true)
        controller.onServiceCapability(KmServiceCapability.ATTACHED_READY)

        assertEquals(
            listOf(KmCaptureEffect.ReleaseSources),
            controller.onSessionConsent(false),
        )
        assertFalse(controller.captureActive)
        assertEquals(0, controller.desiredMask())

        assertEquals(
            listOf(KmCaptureEffect.ApplySources(KmMotionSources.MOUSE_FAMILY)),
            controller.onSessionConsent(true),
        )
        assertTrue(controller.captureActive)
    }

    @Test
    fun `disconnect fails closed and reconnect alone never re-activates`() {
        val controller = KmCaptureController(apiLevel = 35)
        controller.onConnectionAuthenticated(true)
        controller.onOwnershipActive(true)
        controller.onSessionConsent(true)
        controller.onServiceCapability(KmServiceCapability.ATTACHED_READY)
        assertTrue(controller.captureActive)

        // 단절 = fail-closed: 해제가 먼저, 사유 보고가 그 뒤.
        assertEquals(
            listOf(
                KmCaptureEffect.ReleaseSources,
                KmCaptureEffect.FailClosed(KmFailClosedReason.DISCONNECTED),
            ),
            controller.onConnectionAuthenticated(false),
        )
        assertFalse(controller.captureActive)
        assertFalse(controller.sessionConsentGranted)

        // 재인증만으로는 재활성화되지 않는다 (자동 활성 금지).
        assertEquals(
            emptyList<KmCaptureEffect>(),
            controller.onConnectionAuthenticated(true),
        )
        assertFalse(controller.captureActive)

        // 새 명시 동의 후에만 재활성.
        assertEquals(
            listOf(KmCaptureEffect.ApplySources(KmMotionSources.MOUSE_FAMILY)),
            controller.onSessionConsent(true),
        )
        assertTrue(controller.captureActive)
    }

    @Test
    fun `timeout permission loss and teardown fail closed releasing first`() {
        for (trigger in listOf<KmCaptureController.() -> List<KmCaptureEffect>>(
            { onWatchdogTimeout() },
            { onPermissionLost() },
            { onTeardown() },
        )) {
            val controller = readyController()
            controller.onServiceCapability(KmServiceCapability.ATTACHED_READY)
            assertTrue(controller.captureActive)

            val effects = controller.trigger()
            assertEquals(
                "trigger 실패 시 해제가 사유 보고보다 먼저여야 한다",
                listOf(
                    KmCaptureEffect.ReleaseSources,
                    KmCaptureEffect.FailClosed(controller.failClosedReasonOf(effects)),
                ),
                effects,
            )
            assertFalse(controller.captureActive)
            assertEquals(0, controller.desiredMask())

            // fail-closed 후 게이트 입력은 모두 무효화됐다 — 개별 재설정으로는 재활성 없음.
            assertEquals(
                emptyList<KmCaptureEffect>(),
                controller.onConnectionAuthenticated(true),
            )
            assertEquals(emptyList<KmCaptureEffect>(), controller.onOwnershipActive(true))
            assertFalse(controller.captureActive)
        }
    }

    private fun KmCaptureController.failClosedReasonOf(
        effects: List<KmCaptureEffect>,
    ): KmFailClosedReason = (effects.last() as KmCaptureEffect.FailClosed).reason

    @Test
    fun `sink and write errors fail closed`() {
        for (trigger in listOf<KmCaptureController.() -> List<KmCaptureEffect>>(
            { onSinkError() },
            { onWriteFailed() },
        )) {
            val controller = readyController()
            controller.onServiceCapability(KmServiceCapability.ATTACHED_READY)
            assertTrue(controller.captureActive)

            val effects = controller.trigger()
            assertEquals(KmCaptureEffect.ReleaseSources, effects.first())
            assertTrue(effects.last() is KmCaptureEffect.FailClosed)
            assertFalse(controller.captureActive)
        }
    }

    @Test
    fun `ownership release is a normal disarm that keeps session consent`() {
        val controller = readyController()
        controller.onServiceCapability(KmServiceCapability.ATTACHED_READY)
        assertTrue(controller.captureActive)

        assertEquals(
            listOf(KmCaptureEffect.ReleaseSources),
            controller.onOwnershipActive(false),
        )
        assertFalse(controller.captureActive)
        assertTrue(controller.sessionConsentGranted)

        // 동의·인증이 유지된 상태에서 소유권 회복 → 게이트가 다시 열린다.
        assertEquals(
            listOf(KmCaptureEffect.ApplySources(KmMotionSources.MOUSE_FAMILY)),
            controller.onOwnershipActive(true),
        )
        assertTrue(controller.captureActive)
    }

    @Test
    fun `teardown is idempotent and never double releases`() {
        val controller = readyController()
        controller.onServiceCapability(KmServiceCapability.ATTACHED_READY)

        assertEquals(
            listOf(
                KmCaptureEffect.ReleaseSources,
                KmCaptureEffect.FailClosed(KmFailClosedReason.TEARDOWN),
            ),
            controller.onTeardown(),
        )

        // 두 번째 teardown: 이미 해제됨 — ReleaseSources 중복 없이 사유 보고만.
        assertEquals(
            listOf(KmCaptureEffect.FailClosed(KmFailClosedReason.TEARDOWN)),
            controller.onTeardown(),
        )
        assertFalse(controller.captureActive)
    }
}
