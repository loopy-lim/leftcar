package dev.leftcar.viewer.km

import android.accessibilityservice.AccessibilityServiceInfo
import android.content.pm.ApplicationInfo
import android.content.pm.ResolveInfo
import android.content.pm.ServiceInfo
import android.os.Binder
import java.lang.reflect.Proxy
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

internal class KmOwnObservationFixture : KmOwnObservationPlatform {
    var local = 0
    var id = 7
    var unsupported = false
    var observedReadback: Int? = null
    var failInitialization = false
    var observedUnsupported = false
    var onLocalRead: (() -> Unit)? = null
    val activeWrites = mutableListOf<Int>()
    private val binder = Binder()
    override fun connection(service: KmAccessibilityService): KmOwnConnection {
        if (failInitialization) throw ExceptionInInitializerError("unsupported ART")
        check(!unsupported)
        return KmOwnConnection(id, binder)
    }
    override fun localMask(service: KmAccessibilityService): Int {
        onLocalRead?.let { onLocalRead = null; it() }
        return local
    }
    override fun localMask(service: KmAccessibilityService, mask: Int) { local = mask; if (mask != 0) activeWrites += mask }
    override fun observed(info: AccessibilityServiceInfo): Int {
        check(!observedUnsupported) { "Only app observed getter unavailable" }
        return observedReadback ?: info.motionEventSources
    }
    fun install(service: KmAccessibilityService) {
        service.observationPlatform = this
        service.observationMain = object : KmObservationMain { override fun run(action: () -> Unit) = runCatching(action).isSuccess }
    }
}

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE, shadows = [KmRuntimeCompletionTest.ServiceInfoShadow::class])
class KmOwnObservationTest {
    class Endpoint : KmObservationEndpoint {
        var value = ownInfo()
        var observed = 0
        var readbackWrong = false
        var failSet = false
        var corruptFlags = false
        var sets = 0
        override var descriptor = KmBorrowedObservationEndpoint.CONNECTION
        override fun get() = value
        override fun set(info: AccessibilityServiceInfo) { sets++; value = info; if (corruptFlags) value.flags = 0; if (failSet) error("uncertain setter") }
        override fun observed(info: AccessibilityServiceInfo) = if (readbackWrong) 0 else observed
        override fun observe(info: AccessibilityServiceInfo, mask: Int) { observed = mask }
    }
    companion object {
        fun ownInfo() = AccessibilityServiceInfo().apply {
            org.robolectric.util.ReflectionHelpers.setField(this, "mResolveInfo", ResolveInfo().apply { serviceInfo = ServiceInfo().apply {
                name = KmAccessibilityService::class.java.name
                packageName = "leftcar.ll3.kr"
                applicationInfo = ApplicationInfo().apply { uid = 10042; packageName = "leftcar.ll3.kr" }
            } })
        }
    }
    private val authority = KmOwnObservationAuthority(10042, "leftcar.ll3.kr")
    @Test fun `real helper seam rejects wrong UID mode resources descriptor metadata and IME before mutation`() {
        for (kind in listOf("uid", "mode", "resources", "descriptor", "component", "package", "targetUid", "ime", "noInfo")) {
            val e = Endpoint()
            when (kind) {
                "descriptor" -> e.descriptor = "foreign"
                "component" -> e.value.resolveInfo.serviceInfo.name = "OtherService"
                "package" -> e.value.resolveInfo.serviceInfo.packageName = "other"
                "targetUid" -> e.value.resolveInfo.serviceInfo.applicationInfo.uid = 10043
                "ime" -> e.value.flags = AccessibilityServiceInfo.FLAG_INPUT_METHOD_EDITOR
                "noInfo" -> org.robolectric.util.ReflectionHelpers.setField(e.value, "mResolveInfo", null)
            }
            assertTrue(kind, runCatching { authority.apply(if (kind == "uid") 10043 else 10042,
                if (kind == "mode") 9 else 1, { kind != "resources" }) { e } }.isFailure)
            assertEquals(kind, 0, e.sets)
        }
    }
    @Test fun `real helper mode table keeps observe and consume identical and OFF works after expiration`() {
        val e = Endpoint()
        val preserved = AccessibilityServiceInfo.FLAG_REPORT_VIEW_IDS
        for (mode in listOf(1, 2, 0)) {
            e.value.flags = preserved
            assertTrue(authority.apply(10042, mode, { mode != 0 }) { e })
            assertEquals(if (mode == 0) 0 else KmMotionSources.MOUSE_FAMILY, e.value.motionEventSources)
            assertEquals(e.value.motionEventSources, e.observed)
            assertEquals(preserved or if (mode == 0) 0 else AccessibilityServiceInfo.FLAG_REQUEST_FILTER_KEY_EVENTS, e.value.flags)
        }
    }
    @Test fun `helper verifies an immutable full flags tuple even when endpoint mutates its argument`() {
        val e = Endpoint(); e.corruptFlags = true
        assertFalse(authority.apply(10042, 1, { true }) { e })
    }
    @Test fun `helper expiry during borrowed RPC rejects success without holding its resource monitor`() {
        var live = true
        val e = object : KmObservationEndpoint by Endpoint() {
            override fun set(info: AccessibilityServiceInfo) { live = false }
        }
        assertTrue(runCatching { authority.apply(10042, 1, { live }) { e } }.isFailure)
    }
    @Test fun `real bounded main dispatcher timeout cannot run an invalidated queued active attempt`() {
        val attempt = KmObservationAttempt(); val token = attempt.begin()
        var writes = 0; var completed: Boolean? = null
        val main = KmBoundedObservationMain(timeoutMs = 10)
        val worker = Thread { completed = main.run { attempt.write(token, { true }) { writes++ } } }
        worker.start(); worker.join(1000)
        assertFalse(worker.isAlive); assertEquals(false, completed)
        attempt.cancel()
        org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
        assertEquals(0, writes)
    }
    @Test fun `foreign Binder descriptor rejected by actual borrowed endpoint before any invocation`() {
        val foreign = Binder().apply { attachInterface(null, "foreign.connection") }
        assertTrue(runCatching { KmBorrowedObservationEndpoint(foreign) }.isFailure)
    }
    @Test fun `hidden class initialization failure stays fixed capability failure`() {
        val (service, platform) = service(); platform.failInitialization = true
        val error = runCatching { service.configure(helper { true }, KmCaptureMode.OBSERVE) }.exceptionOrNull()
        assertTrue(error is KmObservationUnavailable); assertEquals(KmObservationUnavailable.REASON, error?.message)
    }
    @Test fun `actual service rejects unsupported member before helper set`() {
        val (service, platform) = service()
        platform.unsupported = true
        var calls = 0
        assertTrue(runCatching { service.configure(helper { calls++; true }, KmCaptureMode.OBSERVE) }.isFailure)
        assertEquals(0, calls); assertEquals(KmCaptureMode.OFF, service.mode)
    }
    @Test fun `failed and uncertain readback perform confirmed local and system OFF rollback`() {
        for (uncertain in listOf(false, true)) {
            val (service, platform) = service()
            val e = Endpoint(); service.serviceInfo = e.value
            val h = helper {
                e.failSet = uncertain; e.readbackWrong = !uncertain
                val okay = authority.apply(10042, 1, { true }) { e }
                service.serviceInfo = e.value
                okay
            }
            assertTrue(runCatching { service.configure(h, KmCaptureMode.OBSERVE) }.isFailure)
            assertEquals(0, platform.local); assertEquals(0, service.serviceInfo.motionEventSources)
            assertEquals(0, service.serviceInfo.flags and AccessibilityServiceInfo.FLAG_REQUEST_FILTER_KEY_EVENTS)
            assertEquals(KmCaptureMode.OFF, service.mode); assertTrue(service.restoreLocal())
        }
    }
    @Test fun `queued active write timeout and cancellation never rearm later`() {
        val (service, platform) = service()
        val queued = mutableListOf<() -> Unit>()
        service.observationMain = object : KmObservationMain { override fun run(action: () -> Unit): Boolean { queued += action; return false } }
        assertTrue(runCatching { service.configure(helper { true }, KmCaptureMode.OBSERVE) }.isFailure)
        queued.forEach { runCatching(it) }
        assertEquals(0, platform.local); assertEquals(KmCaptureMode.OFF, service.mode)
    }
    @Test fun `only app observed getter unsupported rejects before any active local helper or owner write`() {
        val (service, platform) = service(); platform.observedUnsupported = true
        var helperCalls = 0; var committed = false
        val error = runCatching { service.configure(helper { helperCalls++; true }, KmCaptureMode.OBSERVE) { committed = true } }.exceptionOrNull()
        assertTrue(error is KmObservationUnavailable)
        assertEquals(KmObservationUnavailable.REASON, error?.message)
        assertTrue(platform.activeWrites.isEmpty()); assertEquals(0, helperCalls)
        assertFalse(committed); assertEquals(KmCaptureMode.OFF, service.mode)
    }
    @Test fun `cancel after final freshness during local readback prevents either routing or owner commitment`() {
        for (mode in listOf(KmCaptureMode.OBSERVE, KmCaptureMode.CONSUME)) {
            val (service, platform) = service(); var committed = false
            val ticket = if (mode == KmCaptureMode.CONSUME) service.beginCapture() else null
            platform.onLocalRead = { service.invalidateObservation() }
            assertTrue(mode.toString(), runCatching { service.configure(helper { true }, mode, ticket) { committed = true } }.isFailure)
            assertFalse(committed); assertEquals(KmCaptureMode.OFF, service.mode); assertEquals(0, platform.local)
        }
    }
    @Test fun `capture hold invalidated before transaction rejects before helper write`() {
        val (service, platform) = service(); val ticket = service.beginCapture()
        val f = KmAccessibilityService::class.java.getDeclaredField("routing").apply { isAccessible = true }
        (f.get(service) as KmPhysicalTransition).pointer(1, 0)
        var calls = 0
        assertTrue(runCatching { service.configure(helper { calls++; true }, KmCaptureMode.CONSUME, ticket) }.isFailure)
        assertEquals(0, calls); assertEquals(0, platform.local); assertEquals(KmCaptureMode.OFF, service.mode)
    }
    @Test fun `fresh generation connection and capture gates prevent ownership commit and restore OFF`() {
        for (kind in listOf("generation", "connection", "cancel", "capture")) {
            val (service, platform) = service()
            var fresh = true; var committed = false
            val ticket = service.beginCapture()
            val h = helper {
                when (kind) {
                    "generation" -> fresh = false
                    "connection" -> platform.id++
                    "cancel" -> service.invalidateObservation()
                    "capture" -> {
                        val f = KmAccessibilityService::class.java.getDeclaredField("routing").apply { isAccessible = true }
                        (f.get(service) as KmPhysicalTransition).pointer(1, 0)
                    }
                }
                true
            }
            assertTrue(kind, runCatching { service.configure(h, KmCaptureMode.CONSUME, ticket, { fresh }) { committed = true } }.isFailure)
            assertFalse(kind, committed); assertEquals(0, platform.local); assertEquals(KmCaptureMode.OFF, service.mode)
        }
    }
    private fun service(): Pair<KmAccessibilityService, KmOwnObservationFixture> {
        val s = Robolectric.buildService(KmAccessibilityService::class.java).get()
        KmAccessibilityService.instance = s
        val p = KmOwnObservationFixture(); p.install(s)
        s.serviceInfo = AccessibilityServiceInfo()
        return s to p
    }
    private fun helper(action: () -> Boolean): IKmInputAuthority = Proxy.newProxyInstance(
        IKmInputAuthority::class.java.classLoader, arrayOf(IKmInputAuthority::class.java)) { _, method, _ ->
        if (method.name == "configureOwnObservation") action() else null
    } as IKmInputAuthority
}
