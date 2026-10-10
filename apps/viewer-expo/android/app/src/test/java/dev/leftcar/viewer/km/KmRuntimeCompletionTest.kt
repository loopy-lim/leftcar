package dev.leftcar.viewer.km

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.AccessibilityServiceInfo
import java.io.DataOutputStream
import java.time.Duration
import org.robolectric.Robolectric
import org.robolectric.RuntimeEnvironment
import android.os.Handler
import android.os.Looper
import java.lang.reflect.Proxy
import java.net.ServerSocket
import org.junit.Assert.*
import org.junit.Test
import org.junit.Before
import org.junit.After
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implements
import org.robolectric.annotation.Implementation
import org.robolectric.shadows.ShadowAccessibilityService

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE, shadows = [KmRuntimeCompletionTest.ServiceInfoShadow::class])
class KmRuntimeCompletionTest {
    // Robolectric has no system accessibility connection. Model only that OS getter/setter,
    // leaving the real service, runtime, encrypted socket, State and callback paths intact.
    @Implements(AccessibilityService::class)
    class ServiceInfoShadow : ShadowAccessibilityService() {
        private var info = AccessibilityServiceInfo()
        @Implementation fun getServiceInfo(): AccessibilityServiceInfo = info
        @Implementation fun setServiceInfo(value: AccessibilityServiceInfo) { info = value }
    }
    @Before fun isolateFixture() {
        KmAccessibilityService.instance = null
        field("observationService", null)
        field("authority", null)
        field("authorityBinder", null)
        field("deadAuthorityBinder", null)
        KmRuntime.stop(); drain()
    }
    @After fun disposeFixture() { isolateFixture() }
    private fun field(name: String, value: Any?) {
        KmRuntime::class.java.getDeclaredField(name).apply { isAccessible = true }.set(KmRuntime, value)
    }
    private fun worker(): Handler =
         KmRuntime::class.java.getDeclaredField("worker").apply { isAccessible = true }.get(KmRuntime) as Handler
    private fun drain() {
        shadowOf(worker().looper).idle()
        shadowOf(Looper.getMainLooper()).idle()
    }
    private fun authority(failRelease: () -> Boolean = { false }, verify: Boolean = true, calls: MutableList<String> = mutableListOf()): IKmInputAuthority = Proxy.newProxyInstance(
        IKmInputAuthority::class.java.classLoader, arrayOf(IKmInputAuthority::class.java),
    ) { _, method, args ->
        calls += method.name
        if (method.name == "configureOwnObservation") return@newProxyInstance verify
        if (failRelease() && method.name == "releaseAll") error("release unavailable")
        when (method.returnType) { Boolean::class.javaPrimitiveType -> true; Int::class.javaPrimitiveType -> 0; else -> null }
    } as IKmInputAuthority

    @Test fun `real native start stays pending after Hello until State and stop settles cancellation once`() {
        field("authority", authority())
        field("status", KmNativeStatus("preparing", mouseObservation = true, mouseCapture = true))
        val results = mutableListOf<Result<KmNativeStatus>>()
        ServerSocket(0).use { server ->
            KmRuntime.start(KmStartConfig("127.0.0.1", server.localPort, "test", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", 1, "right", true, true)) { results += it }
            drain()
            assertTrue("Hello alone must not complete native Start", results.isEmpty())
            val duplex = KmRuntime::class.java.getDeclaredField("transport").apply { isAccessible = true }.get(KmRuntime)
            @Suppress("UNCHECKED_CAST")
            val lateState = KmDuplex::class.java.getDeclaredField("received").apply { isAccessible = true }.get(duplex) as (KmWire) -> Unit
            KmRuntime.stop(); drain()
            lateState(KmWire(1, 1, KmWire.payload("state", "owner" to "local"))); drain()
            assertEquals("off", KmRuntime.status.state)
            assertEquals(1, results.size)
            assertTrue(results.single().isFailure)
            KmRuntime.stop(); drain()
            assertEquals(1, results.size)
        }
    }
    @Test fun `actual failed stop retains actionable status and authority for retry`() {
        val calls = mutableListOf<String>()
        var failRelease = true
        field("authority", authority(failRelease = { failRelease }, calls = calls))
        val results = mutableListOf<Result<Unit>>()
        KmRuntime.stop(completion = { results += it }); drain()
        assertEquals("cleanupFailed", KmRuntime.status.state)
        assertTrue(results.single().isFailure)
        assertEquals(listOf("releaseAll", "releaseMouse", "close"), calls)
        assertNotNull(KmRuntime::class.java.getDeclaredField("authority").apply { isAccessible = true }.get(KmRuntime))
        failRelease = false
        KmRuntime.stop(completion = { results += it }); drain()
        assertEquals("off", KmRuntime.status.state)
        assertTrue(results.last().isSuccess)
    }
    @Test fun `real delayed authenticated State resolves Start only after observation readback`() {
        stateRoundTrip(verify = true)
    }
    @Test fun `failed State application rejects native start`() {
        stateRoundTrip(verify = false)
    }
    @Test fun `Runtime Stop at final local readback rejects State without owner success`() {
        stateRoundTrip(verify = true, cancelAtReadback = true)
    }
    private fun stateRoundTrip(verify: Boolean, cancelAtReadback: Boolean = false) {
        field("context", RuntimeEnvironment.getApplication())
        val service = Robolectric.buildService(KmAccessibilityService::class.java).get()
        service.serviceInfo = AccessibilityServiceInfo()
        KmAccessibilityService.instance = service
        val platform = KmOwnObservationFixture(); platform.install(service)
        if (cancelAtReadback) platform.onLocalRead = { KmRuntime.stop() }
        val session = KmRuntime::class.java.getDeclaredField("observationSession").apply { isAccessible = true }.get(KmRuntime) as KmObservationSession
        field("observationSessionTicket", session.ticket()) // This fixture models an already prepared session.
        field("authority", authority(verify = verify))
        field("status", KmNativeStatus("preparing", mouseObservation = true, mouseCapture = true))
        val results = mutableListOf<Result<KmNativeStatus>>()
        ServerSocket(0).use { server ->
            KmRuntime.start(KmStartConfig("127.0.0.1", server.localPort, "test", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", 1, "right", true, true)) { results += it }
            drain()
            assertTrue(results.isEmpty())
            server.accept().use { socket ->
                val wire = """{"version":1,"sessionId":"test","epoch":1,"seq":1,"origin":"mac","payload":{"kind":"state","owner":"local"}}"""
                val frame = KmCipher(KmCipher.keys(ByteArray(32), "test").second).seal(wire.toByteArray())
                DataOutputStream(socket.getOutputStream()).apply { writeInt(frame.size); write(frame); flush() }
                val deadline = System.nanoTime() + 2_000_000_000L
                while (results.isEmpty() && System.nanoTime() < deadline) { Thread.sleep(5); drain() }
                assertEquals(1, results.size)
                if (verify && !cancelAtReadback) assertEquals("armed", results.single().getOrThrow().state)
                else assertTrue(results.single().isFailure)
                if (cancelAtReadback) {
                    assertEquals(KmCaptureMode.OFF, service.mode)
                    assertEquals("off", KmRuntime.status.state)
                }
            }
        }
        KmRuntime.stop(); drain()
    }
    @Test fun `native disconnect before State rejects once`() {
        field("authority", authority())
        field("status", KmNativeStatus("preparing", mouseObservation = true, mouseCapture = true))
        val results = mutableListOf<Result<KmNativeStatus>>()
        ServerSocket(0).use { server ->
            KmRuntime.start(KmStartConfig("127.0.0.1", server.localPort, "test", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", 1, "right", true, true)) { results += it }
            drain()
            server.accept().close()
            val deadline = System.nanoTime() + 2_000_000_000L
            while (results.isEmpty() && System.nanoTime() < deadline) { Thread.sleep(5); drain() }
            assertEquals(1, results.size)
            assertTrue(results.single().isFailure)
            KmRuntime.stop(); drain()
            assertEquals(1, results.size)
        }
    }
    @Test fun `native start deadline rejects once without authenticated State`() {
        field("authority", authority())
        field("status", KmNativeStatus("preparing", mouseObservation = true, mouseCapture = true))
        val results = mutableListOf<Result<KmNativeStatus>>()
        ServerSocket(0).use { server ->
            KmRuntime.start(KmStartConfig("127.0.0.1", server.localPort, "test", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", 1, "right", true, true)) { results += it }
            drain()
            shadowOf(worker().looper).idleFor(Duration.ofMillis(1001)); drain()
            assertEquals(1, results.size)
            assertTrue(results.single().isFailure)
            KmRuntime.stop(); drain()
            assertEquals(1, results.size)
        }
    }

}
