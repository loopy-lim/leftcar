package dev.leftcar.viewer.km

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.AccessibilityServiceInfo
import android.os.Handler
import android.os.IBinder
import android.os.IInterface
import android.os.Looper
import org.lsposed.hiddenapibypass.HiddenApiBypass
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

internal data class KmOwnConnection(val id: Int, val binder: IBinder)

/** Own live instance only. No caller, React, network or service-id discovery input. */
internal interface KmOwnObservationPlatform {
    fun connection(service: KmAccessibilityService): KmOwnConnection
    fun localMask(service: KmAccessibilityService): Int
    fun localMask(service: KmAccessibilityService, mask: Int)
    fun observed(info: AccessibilityServiceInfo): Int
}

internal object KmTargetedOwnObservation : KmOwnObservationPlatform {
    private fun field() = HiddenApiBypass.getInstanceFields(AccessibilityService::class.java)
        .single { it.name == "mMotionEventSources" && it.type == Int::class.javaPrimitiveType }
        .apply { isAccessible = true }
    override fun connection(service: KmAccessibilityService): KmOwnConnection {
        val id = HiddenApiBypass.invoke(AccessibilityService::class.java, service, "getConnectionId") as Int
        check(id >= 0)
        val client = Class.forName("android.view.accessibility.AccessibilityInteractionClient")
        val connection = HiddenApiBypass.invoke(client, null, "getConnection", id) as? IInterface
            ?: throw KmObservationUnavailable()
        field() // Resolve exact required local layout before any nonzero write.
        return KmOwnConnection(id, connection.asBinder().also { check(it.isBinderAlive) })
    }
    override fun localMask(service: KmAccessibilityService): Int = field().getInt(service)
    override fun localMask(service: KmAccessibilityService, mask: Int) { field().setInt(service, mask) }
    override fun observed(info: AccessibilityServiceInfo): Int =
        HiddenApiBypass.invoke(AccessibilityServiceInfo::class.java, info, "getObservedMotionEventSources") as Int
}

/** Stop advances a session revision synchronously; configure cannot reopen that session.
 * Reads are nonblocking even inside the attempt gate. Commit actions contain local state only. */
internal class KmObservationSession {
    @Volatile private var revision = 0L
    fun ticket(): Long = revision
    fun current(ticket: Long): Boolean = revision == ticket
    @Synchronized fun cancel() { revision++ }
    @Synchronized fun commit(ticket: Long, action: () -> Unit): Boolean {
        if (!current(ticket)) return false
        action()
        return true
    }
}

/** Short gate covers attempt invalidation and own main-thread writes, never Binder calls. */
internal class KmObservationAttempt {
    private var serial = 0L
    @Synchronized fun begin(): Long = ++serial
    @Synchronized fun cancel() { serial++ }
    @Synchronized fun current(token: Long): Boolean = serial == token
    @Synchronized fun write(token: Long, fresh: () -> Boolean, action: () -> Unit): Boolean {
        if (serial != token || !fresh()) return false
        action()
        return true
    }
}

internal interface KmObservationMain {
    fun run(action: () -> Unit): Boolean
}

internal class KmBoundedObservationMain(
    private val main: Handler = Handler(Looper.getMainLooper()),
    private val timeoutMs: Long = 500,
) : KmObservationMain {
    override fun run(action: () -> Unit): Boolean {
        if (Looper.myLooper() == main.looper) return runCatching(action).isSuccess
        val completed = CountDownLatch(1)
        var success = false
        if (!main.post { try { success = runCatching(action).isSuccess } finally { completed.countDown() } }) return false
        return completed.await(timeoutMs, TimeUnit.MILLISECONDS) && success
    }
}

/** Shell-side borrowed endpoint. The authorized original app is trusted to supply its own
 * framework Binder; descriptor/metadata checks are error guards, not proof against that app. */
internal interface KmObservationEndpoint {
    val descriptor: String
    fun get(): AccessibilityServiceInfo
    fun set(info: AccessibilityServiceInfo)
    fun observed(info: AccessibilityServiceInfo): Int
    fun observe(info: AccessibilityServiceInfo, mask: Int)
}

internal class KmBorrowedObservationEndpoint(binder: IBinder) : KmObservationEndpoint {
    override val descriptor = binder.interfaceDescriptor ?: ""
    init { check(descriptor == CONNECTION) { "Unexpected accessibility connection" } }
    private val type = Class.forName(CONNECTION)
    private val endpoint = Class.forName("$CONNECTION\$Stub").getMethod("asInterface", IBinder::class.java).invoke(null, binder)
    override fun get() = type.getMethod("getServiceInfo").invoke(endpoint) as AccessibilityServiceInfo
    override fun set(info: AccessibilityServiceInfo) { type.getMethod("setServiceInfo", AccessibilityServiceInfo::class.java).invoke(endpoint, info) }
    override fun observed(info: AccessibilityServiceInfo) = AccessibilityServiceInfo::class.java.getMethod("getObservedMotionEventSources").invoke(info) as Int
    override fun observe(info: AccessibilityServiceInfo, mask: Int) { AccessibilityServiceInfo::class.java.getMethod("setObservedMotionEventSources", Int::class.javaPrimitiveType).invoke(info, mask) }
    companion object { const val CONNECTION = "android.accessibilityservice.IAccessibilityServiceConnection" }
}

/** Production helper seam. All masks and flag changes derive from the closed mode table. */
internal class KmOwnObservationAuthority(private val uid: Int, private val packageName: String) {
    fun apply(caller: Int, mode: Int, prepared: () -> Boolean, endpoint: () -> KmObservationEndpoint): Boolean {
        check(caller == uid) { "KM caller denied" }
        require(mode in 0..2)
        check(mode == 0 || prepared()) { "KM authority expired" }
        val connection = endpoint()
        check(connection.descriptor == KmBorrowedObservationEndpoint.CONNECTION)
        val info = connection.get()
        val target = info.resolveInfo?.serviceInfo ?: error("Missing own accessibility target")
        check(target.name == KmAccessibilityService::class.java.name && target.packageName == packageName &&
            target.applicationInfo?.uid == uid && target.applicationInfo?.packageName == packageName)
        check(info.flags and AccessibilityServiceInfo.FLAG_INPUT_METHOD_EDITOR == 0) { "Unexpected input-method behavior" }
        val mask = if (mode == 0) 0 else KmMotionSources.MOUSE_FAMILY
        val otherFlags = info.flags and AccessibilityServiceInfo.FLAG_REQUEST_FILTER_KEY_EVENTS.inv()
        info.motionEventSources = mask
        connection.observe(info, mask)
        val expectedFlags = otherFlags or if (mode == 0) 0 else AccessibilityServiceInfo.FLAG_REQUEST_FILTER_KEY_EVENTS
        info.flags = expectedFlags
        check(mode == 0 || prepared())
        connection.set(info)
        val applied = connection.get()
        check(mode == 0 || prepared())
        return applied.motionEventSources == mask && connection.observed(applied) == mask && applied.flags == expectedFlags
    }
}
