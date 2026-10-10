package dev.leftcar.viewer.km

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.AccessibilityServiceInfo
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.os.DeadObjectException
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import java.lang.reflect.Proxy
import org.junit.Assert.*
import org.junit.Test
import org.junit.After
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.shadows.ShadowAccessibilityService
import rikka.shizuku.Shizuku

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE, shadows = [KmRuntimeRecoveryTest.ServiceInfoShadow::class, KmRuntimeRecoveryTest.ShizukuShadow::class])
class KmRuntimeRecoveryTest {
    @Implements(AccessibilityService::class)
    class ServiceInfoShadow : ShadowAccessibilityService() {
        var denyRestore = false
        private var info = AccessibilityServiceInfo()
        @Implementation fun getServiceInfo(): AccessibilityServiceInfo = info
        @Implementation fun setServiceInfo(value: AccessibilityServiceInfo) {
            if (denyRestore) error("local restoration denied")
            info = value
        }
    }
    @Implements(Shizuku::class)
    class ShizukuShadow {
        companion object {
            lateinit var next: Helper
            val connections = mutableListOf<ServiceConnection>()
            val retired = mutableListOf<ServiceConnection>()
            @JvmStatic @Implementation fun pingBinder() = true
            @JvmStatic @Implementation fun getUid() = 2000
            @JvmStatic @Implementation fun getVersion() = 13
            @JvmStatic @Implementation fun checkSelfPermission() = PackageManager.PERMISSION_GRANTED
            @JvmStatic @Implementation fun bindUserService(options: Shizuku.UserServiceArgs, connection: ServiceConnection) {
                connections += connection
                connection.onServiceConnected(null, next.binder)
            }
            @JvmStatic @Implementation fun unbindUserService(options: Shizuku.UserServiceArgs, connection: ServiceConnection, remove: Boolean) {
                retired += connection
            }
        }
    }
    /** The same permanent proxy survives death. Only the next explicit bind creates another helper. */
    class Helper {
        var alive = true
        var failCleanup = false
        var beforePrepare: (() -> Unit)? = null
        val activeConfigurations = mutableListOf<Int>()
        val deaths = mutableListOf<IBinder.DeathRecipient>()
        val cleanup = mutableListOf<String>()
        val binder: IBinder = Proxy.newProxyInstance(IBinder::class.java.classLoader, arrayOf(IBinder::class.java)) { _, method, args ->
            when (method.name) {
                "queryLocalInterface" -> api
                "isBinderAlive", "pingBinder" -> alive
                "linkToDeath" -> { deaths += args!![0] as IBinder.DeathRecipient; null }
                "unlinkToDeath" -> true
                else -> null
            }
        } as IBinder
        val api: IKmInputAuthority = Proxy.newProxyInstance(IKmInputAuthority::class.java.classLoader, arrayOf(IKmInputAuthority::class.java)) { _, method, args ->
            if (method.name == "asBinder") return@newProxyInstance binder
            if (method.name in setOf("releaseAll", "releaseMouse", "close")) {
                cleanup += method.name
                if (!alive) throw DeadObjectException()
                if (failCleanup) error("live helper cleanup denied")
            }
            when (method.name) {
                "prepare" -> { beforePrepare?.invoke(); true }
                "configureOwnObservation" -> { if (args!![1] as Int != 0) activeConfigurations += args[1] as Int; true }
                else -> when (method.returnType) { Boolean::class.javaPrimitiveType -> true; Int::class.javaPrimitiveType -> 0; else -> null }
            }
        } as IKmInputAuthority
        fun die(notify: Boolean = true) { alive = false; if (notify) deaths.forEach { it.binderDied() } }
    }
    @After fun disposeProcessFixture() {
        // Test isolation only, after all recovery assertions. Never replace a dead proxy
        // to make the tested production retry pass.
        KmAccessibilityService.instance = null
        KmRuntime::class.java.getDeclaredField("authority").apply { isAccessible = true }.set(KmRuntime, null)
        KmRuntime::class.java.getDeclaredField("observationService").apply { isAccessible = true }.set(KmRuntime, null)
        KmRuntime.stop(); drain()
    }
    private fun drain() {
        val worker = KmRuntime::class.java.getDeclaredField("worker").apply { isAccessible = true }.get(KmRuntime) as Handler
        repeat(4) { shadowOf(worker.looper).idle(); shadowOf(Looper.getMainLooper()).idle() }
    }
    private fun setup(): Pair<Helper, KmAccessibilityService> {
        ShizukuShadow.connections.clear(); ShizukuShadow.retired.clear()
        val service = Robolectric.buildService(KmAccessibilityService::class.java).get()
        KmAccessibilityService.instance = service
        KmOwnObservationFixture().install(service)
        KmRuntime.initialize(RuntimeEnvironment.getApplication())
        val helper = Helper(); ShizukuShadow.next = helper
        val results = mutableListOf<Result<KmNativeStatus>>()
        KmRuntime.prepare { results += it }; drain()
        assertEquals("preparing", results.single().getOrThrow().state)
        assertEquals(1, ShizukuShadow.connections.size)
        return helper to service
    }
    @Test fun `Stop returned while worker prepare paused forbids later active admission and success publication`() {
        ShizukuShadow.connections.clear(); ShizukuShadow.retired.clear()
        val service = Robolectric.buildService(KmAccessibilityService::class.java).get()
        KmAccessibilityService.instance = service
        val platform = KmOwnObservationFixture(); platform.install(service)
        KmRuntime.initialize(RuntimeEnvironment.getApplication())
        val helper = Helper(); ShizukuShadow.next = helper
        helper.beforePrepare = {
            // This worker remains inside prepare until a separate Stop caller returns.
            val stopCaller = Thread { KmRuntime.stop() }
            stopCaller.start(); stopCaller.join(1000)
            check(!stopCaller.isAlive) { "Stop must not wait for paused worker" }
        }
        val results = mutableListOf<Result<KmNativeStatus>>()
        KmRuntime.prepare { results += it }; drain()
        assertTrue(platform.activeWrites.isEmpty())
        assertTrue(helper.activeConfigurations.isEmpty())
        assertEquals(1, results.size); assertTrue(results.single().isFailure)
        assertEquals(KmCaptureMode.OFF, service.mode); assertEquals("off", KmRuntime.status.state)
    }
    @Test fun `confirmed dead helper retires after local restore and explicit prepare binds a new unarmed generation`() {
        val (dead, service) = setup()
        val oldConnection = ShizukuShadow.connections.single()
        dead.die(); drain()
        assertEquals("off", KmRuntime.status.state)
        assertEquals(KmCaptureMode.OFF, service.mode)
        assertEquals(listOf(oldConnection), ShizukuShadow.retired)
        assertEquals(listOf("releaseAll", "releaseMouse", "close"), dead.cleanup)
        assertEquals(1, ShizukuShadow.connections.size) // no automatic rebind/rearm
        val next = Helper(); ShizukuShadow.next = next
        val results = mutableListOf<Result<KmNativeStatus>>()
        KmRuntime.prepare { results += it }; drain()
        assertEquals("preparing", results.single().getOrThrow().state)
        assertEquals(2, ShizukuShadow.connections.size)
        assertEquals(KmCaptureMode.OBSERVE, service.mode)
        for (name in listOf("config", "transport", "gate")) {
            assertNull(KmRuntime::class.java.getDeclaredField(name).apply { isAccessible = true }.get(KmRuntime))
        } // Neither direction has session authority before explicit authenticated Start.
        oldConnection.onServiceConnected(null, dead.binder)
        dead.deaths.forEach { it.binderDied() }; drain()
        assertEquals("preparing", KmRuntime.status.state) // stale death must not stop replacement
        assertTrue(next.cleanup.isEmpty())
        KmRuntime.stop(); drain()
    }
    @Test fun `dead liveness allows explicit retry only after local restoration succeeds`() {
        val (dead, service) = setup()
        val system = org.robolectric.shadow.api.Shadow.extract<ServiceInfoShadow>(service)
        system.denyRestore = true
        dead.die(notify = false) // liveness, not arbitrary RPC error, proves death
        val results = mutableListOf<Result<Unit>>()
        KmRuntime.stop(completion = { results += it }); drain()
        assertTrue(results.single().isFailure)
        assertEquals("cleanupFailed", KmRuntime.status.state)
        assertTrue(ShizukuShadow.retired.isEmpty())
        system.denyRestore = false
        KmRuntime.stop(completion = { results += it }); drain()
        assertTrue(results.last().isSuccess)
        assertEquals("off", KmRuntime.status.state)
        assertEquals(1, ShizukuShadow.retired.size)
    }
    @Test fun `dead helper never retires when actual observed cleanup readback remains nonzero`() {
        val (helper, service) = setup()
        val platform = service.observationPlatform as KmOwnObservationFixture
        platform.observedReadback = KmMotionSources.MOUSE_FAMILY
        helper.die(); drain()
        assertEquals("cleanupFailed", KmRuntime.status.state)
        assertTrue(ShizukuShadow.retired.isEmpty())
        platform.observedReadback = 0
        KmRuntime.stop(); drain()
        assertEquals("off", KmRuntime.status.state)
        assertEquals(1, ShizukuShadow.retired.size)
    }
    @Test fun `retained helper death after failed Stop still recovers despite the advanced generation`() {
        val (helper, _) = setup()
        helper.failCleanup = true
        KmRuntime.stop(); drain()
        assertEquals("cleanupFailed", KmRuntime.status.state)
        helper.die(); drain()
        assertEquals("off", KmRuntime.status.state)
        assertEquals(1, ShizukuShadow.retired.size)
        assertEquals(1, ShizukuShadow.connections.size)
    }
    @Test fun `live helper RPC failures stay unconfirmed until same proxy cleanup succeeds`() {
        val (live, _) = setup()
        live.failCleanup = true
        val results = mutableListOf<Result<Unit>>()
        repeat(2) { KmRuntime.stop(completion = { results += it }); drain() }
        assertTrue(results.all { it.isFailure })
        assertEquals("cleanupFailed", KmRuntime.status.state)
        assertTrue(ShizukuShadow.retired.isEmpty())
        live.failCleanup = false
        KmRuntime.stop(completion = { results += it }); drain()
        assertTrue(results.last().isSuccess)
        assertEquals("off", KmRuntime.status.state)
        assertEquals(1, ShizukuShadow.retired.size)
    }
}
