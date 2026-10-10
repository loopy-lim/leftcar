package dev.leftcar.viewer.km

import android.accessibilityservice.AccessibilityServiceInfo
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.view.accessibility.AccessibilityManager
import dev.loopy.keybridge.remote.IRemoteInputOwnership

/** Separate KM lease; failed binding never silently disables the user's remapper. */
internal class KmKeyBridgeLease(private val context: Context, private val worker: Handler, private val lost: () -> Unit) {
    private val main = Handler(Looper.getMainLooper())
    private var connection: ServiceConnection? = null
    private var remote: IRemoteInputOwnership? = null
    private var closed = false
    @Volatile var acknowledged = false; private set
    private val death = IBinder.DeathRecipient { if (!closed) lost() }

    fun acquire(result: (Boolean) -> Unit) {
        if (!isEnabled(context) || !KmKeyboardInventory.anyPhysical()) { result(true); return }
        var complete = false
        fun finish(value: Boolean) { if (!complete) { complete = true; result(value) } }
        val next = object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
                worker.post {
                    if (closed) return@post
                    val api = IRemoteInputOwnership.Stub.asInterface(binder)
                    val okay = runCatching { binder!!.linkToDeath(death, 0); api.acquire() }.getOrDefault(false)
                    if (okay) { remote = api; acknowledged = true }
                    finish(okay)
                }
            }
            override fun onServiceDisconnected(name: ComponentName?) { if (!closed) lost() }
            override fun onBindingDied(name: ComponentName?) { if (!closed) lost() }
            override fun onNullBinding(name: ComponentName?) { worker.post { finish(false) } }
        }
        connection = next
        main.post {
            val bound = runCatching { context.bindService(Intent("$PACKAGE.REMOTE_INPUT_OWNERSHIP")
                .setComponent(ComponentName(PACKAGE, "$PACKAGE.RemoteInputOwnershipService")), next, Context.BIND_AUTO_CREATE) }.getOrDefault(false)
            if (!bound) worker.post { finish(false) }
        }
        worker.postDelayed({ finish(false) }, 2000)
    }

    fun close() {
        closed = true
        acknowledged = false
        remote?.let { api -> runCatching { api.release(); api.asBinder().unlinkToDeath(death, 0) } }
        remote = null
        connection?.let { value -> main.post { runCatching { context.unbindService(value) } } }
        connection = null
    }
    companion object {
        const val PACKAGE = "dev.loopy.keybridge"
        fun isEnabled(context: Context): Boolean = context.getSystemService(AccessibilityManager::class.java)
            .getEnabledAccessibilityServiceList(AccessibilityServiceInfo.FEEDBACK_ALL_MASK)
            .any { it.resolveInfo.serviceInfo.packageName == PACKAGE }
    }
}
