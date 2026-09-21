package dev.leftcar.viewer.stream

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import dev.loopy.keybridge.remote.IRemoteInputOwnership
import java.util.concurrent.Executor
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

internal enum class KeyBridgeAvailability {
    CHECKING,
    BINDING,
    READY,
    AVAILABLE_WITHOUT_KEYBRIDGE,
    UPDATE_REQUIRED,
    UNAVAILABLE,
}

internal fun resolveKeyBridgeAvailability(
    packageInstalled: Boolean,
    serviceResolvable: Boolean,
): KeyBridgeAvailability = when {
    !packageInstalled -> KeyBridgeAvailability.AVAILABLE_WITHOUT_KEYBRIDGE
    serviceResolvable -> KeyBridgeAvailability.READY
    else -> KeyBridgeAvailability.UPDATE_REQUIRED
}

internal interface KeyBridgeOwnershipRemote {
    fun acquire(): Boolean
    fun release()
}

internal interface KeyBridgeBindingBackend {
    fun packageInstalled(): Boolean
    fun serviceResolvable(): Boolean
    fun bind(
        onConnected: (KeyBridgeOwnershipRemote) -> Unit,
        onDisconnected: () -> Unit,
    ): Boolean
    fun unbind()
}

internal class KeyBridgeInputAdapter(
    private val backend: KeyBridgeBindingBackend,
    private val worker: Executor,
    private val main: Executor,
    private val closeWorker: () -> Unit = {},
) {
    var availability: KeyBridgeAvailability = KeyBridgeAvailability.CHECKING
        private set

    private var remote: KeyBridgeOwnershipRemote? = null
    private var bound = false
    private var closed = false
    private val pendingAcquire = ArrayList<(Boolean) -> Unit>()

    fun prepare() {
        if (closed || bound) return
        availability = resolveKeyBridgeAvailability(
            backend.packageInstalled(),
            backend.serviceResolvable(),
        )
        if (availability != KeyBridgeAvailability.READY) return

        availability = KeyBridgeAvailability.BINDING
        bound = backend.bind(
            onConnected = { connected ->
                main.execute {
                    if (closed) {
                        worker.execute { runCatching { connected.release() } }
                        return@execute
                    }
                    remote = connected
                    availability = KeyBridgeAvailability.READY
                    val callbacks = pendingAcquire.toList()
                    pendingAcquire.clear()
                    callbacks.forEach(::acquire)
                }
            },
            onDisconnected = {
                main.execute {
                    remote = null
                    availability = KeyBridgeAvailability.UNAVAILABLE
                    val callbacks = pendingAcquire.toList()
                    pendingAcquire.clear()
                    callbacks.forEach { it(false) }
                }
            },
        )
        if (!bound && remote == null) availability = KeyBridgeAvailability.UNAVAILABLE
    }

    fun acquire(onResult: (Boolean) -> Unit) {
        if (closed) {
            onResult(false)
            return
        }
        when (availability) {
            KeyBridgeAvailability.AVAILABLE_WITHOUT_KEYBRIDGE -> onResult(true)
            KeyBridgeAvailability.BINDING, KeyBridgeAvailability.CHECKING -> {
                pendingAcquire += onResult
                if (availability == KeyBridgeAvailability.CHECKING) prepare()
            }
            KeyBridgeAvailability.READY -> {
                val connected = remote
                if (connected == null) {
                    onResult(false)
                    return
                }
                worker.execute {
                    val acquired = runCatching { connected.acquire() }.getOrDefault(false)
                    main.execute { onResult(acquired && !closed) }
                }
            }
            KeyBridgeAvailability.UPDATE_REQUIRED,
            KeyBridgeAvailability.UNAVAILABLE,
            -> onResult(false)
        }
    }

    fun release() {
        val connected = remote ?: return
        worker.execute { runCatching { connected.release() } }
    }

    fun close() {
        if (closed) return
        closed = true
        val callbacks = pendingAcquire.toList()
        pendingAcquire.clear()
        callbacks.forEach { it(false) }
        val connected = remote
        remote = null
        worker.execute {
            if (connected != null) runCatching { connected.release() }
            main.execute {
                if (bound) {
                    backend.unbind()
                    bound = false
                }
                closeWorker()
            }
        }
    }

    companion object {
        fun create(context: Context): KeyBridgeInputAdapter {
            val executor: ExecutorService = Executors.newSingleThreadExecutor { runnable ->
                Thread(runnable, "leftcar-keybridge").apply { isDaemon = true }
            }
            val handler = Handler(Looper.getMainLooper())
            return KeyBridgeInputAdapter(
                AndroidKeyBridgeBindingBackend(context.applicationContext),
                executor,
                Executor(handler::post),
                executor::shutdown,
            )
        }
    }
}

private class AndroidKeyBridgeBindingBackend(
    private val context: Context,
) : KeyBridgeBindingBackend {
    private var connection: ServiceConnection? = null

    override fun packageInstalled(): Boolean = runCatching {
        if (Build.VERSION.SDK_INT >= 33) {
            context.packageManager.getPackageInfo(
                KEYBRIDGE_PACKAGE,
                PackageManager.PackageInfoFlags.of(0),
            )
        } else {
            @Suppress("DEPRECATION")
            context.packageManager.getPackageInfo(KEYBRIDGE_PACKAGE, 0)
        }
    }.isSuccess

    override fun serviceResolvable(): Boolean {
        val intent = Intent().setComponent(SERVICE_COMPONENT)
        return if (Build.VERSION.SDK_INT >= 33) {
            context.packageManager.resolveService(intent, PackageManager.ResolveInfoFlags.of(0)) != null
        } else {
            @Suppress("DEPRECATION")
            context.packageManager.resolveService(intent, 0) != null
        }
    }

    override fun bind(
        onConnected: (KeyBridgeOwnershipRemote) -> Unit,
        onDisconnected: () -> Unit,
    ): Boolean {
        if (connection != null) return true
        val next = object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
                val service = IRemoteInputOwnership.Stub.asInterface(binder) ?: run {
                    onDisconnected()
                    return
                }
                onConnected(object : KeyBridgeOwnershipRemote {
                    override fun acquire(): Boolean = service.acquire()
                    override fun release() = service.release()
                })
            }

            override fun onServiceDisconnected(name: ComponentName?) = onDisconnected()
            override fun onBindingDied(name: ComponentName?) = onDisconnected()
            override fun onNullBinding(name: ComponentName?) = onDisconnected()
        }
        connection = next
        val bound = runCatching {
            context.bindService(
                Intent(KEYBRIDGE_ACTION).setComponent(SERVICE_COMPONENT),
                next,
                Context.BIND_AUTO_CREATE,
            )
        }.getOrDefault(false)
        if (!bound) connection = null
        return bound
    }

    override fun unbind() {
        val current = connection ?: return
        connection = null
        runCatching { context.unbindService(current) }
    }

    private companion object {
        const val KEYBRIDGE_PACKAGE = "dev.loopy.keybridge"
        const val KEYBRIDGE_ACTION = "dev.loopy.keybridge.REMOTE_INPUT_OWNERSHIP"
        val SERVICE_COMPONENT = ComponentName(
            KEYBRIDGE_PACKAGE,
            "dev.loopy.keybridge.RemoteInputOwnershipService",
        )
    }
}
