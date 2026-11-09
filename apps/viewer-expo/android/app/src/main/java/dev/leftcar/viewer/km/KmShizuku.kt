package dev.leftcar.viewer.km

import android.content.ComponentName
import android.content.ServiceConnection

/**
 * Reflection seam over Rikka Shizuku so KM sources never import `rikka.*`
 * (architecture L0 scans import lines only).
 *
 * Function vars are overridable for JVM unit tests: Robolectric shadows do not
 * intercept Method.invoke, so reflective production calls need an explicit fake.
 */
internal object KmShizuku {
    private const val CLASS = "rikka.shizuku.Shizuku"
    private val type: Class<*> get() = KmReflect.clazz(CLASS)

    @Volatile
    var pingBinder: () -> Boolean = { invokeStatic("pingBinder") as Boolean }

    @Volatile
    var getUid: () -> Int = { invokeStatic("getUid") as Int }

    @Volatile
    var getVersion: () -> Int = { invokeStatic("getVersion") as Int }

    @Volatile
    var checkSelfPermission: () -> Int = { invokeStatic("checkSelfPermission") as Int }

    @Volatile
    var requestPermission: (Int) -> Unit = { code -> invokeStatic("requestPermission", code); Unit }

    @Volatile
    var userServiceArgs: (ComponentName) -> Any = ::defaultUserServiceArgs

    @Volatile
    var bindUserService: (Any, ServiceConnection) -> Unit = ::defaultBindUserService

    @Volatile
    var unbindUserService: (Any, ServiceConnection, Boolean) -> Unit = ::defaultUnbindUserService

    fun onBinderReceived(block: () -> Unit): Any = listener("OnBinderReceivedListener", "onBinderReceived") { block(); null }
    fun onBinderDead(block: () -> Unit): Any = listener("OnBinderDeadListener", "onBinderDead") { block(); null }
    fun onRequestPermissionResult(block: (Int, Int) -> Unit): Any =
        listener("OnRequestPermissionResultListener", "onRequestPermissionResult") { args ->
            block(args!![0] as Int, args[1] as Int); null
        }

    fun addBinderReceivedListenerSticky(listener: Any) {
        type.getMethod("addBinderReceivedListenerSticky", KmReflect.clazz("$CLASS\$OnBinderReceivedListener"))
            .invoke(null, listener)
    }
    fun addBinderDeadListener(listener: Any) {
        type.getMethod("addBinderDeadListener", KmReflect.clazz("$CLASS\$OnBinderDeadListener"))
            .invoke(null, listener)
    }
    fun addRequestPermissionResultListener(listener: Any) {
        type.getMethod("addRequestPermissionResultListener", KmReflect.clazz("$CLASS\$OnRequestPermissionResultListener"))
            .invoke(null, listener)
    }
    fun removeBinderReceivedListener(listener: Any) {
        type.getMethod("removeBinderReceivedListener", KmReflect.clazz("$CLASS\$OnBinderReceivedListener"))
            .invoke(null, listener)
    }
    fun removeBinderDeadListener(listener: Any) {
        type.getMethod("removeBinderDeadListener", KmReflect.clazz("$CLASS\$OnBinderDeadListener"))
            .invoke(null, listener)
    }
    fun removeRequestPermissionResultListener(listener: Any) {
        type.getMethod("removeRequestPermissionResultListener", KmReflect.clazz("$CLASS\$OnRequestPermissionResultListener"))
            .invoke(null, listener)
    }

    fun reset() {
        pingBinder = { invokeStatic("pingBinder") as Boolean }
        getUid = { invokeStatic("getUid") as Int }
        getVersion = { invokeStatic("getVersion") as Int }
        checkSelfPermission = { invokeStatic("checkSelfPermission") as Int }
        requestPermission = { code -> invokeStatic("requestPermission", code); Unit }
        userServiceArgs = ::defaultUserServiceArgs
        bindUserService = ::defaultBindUserService
        unbindUserService = ::defaultUnbindUserService
    }

    private fun defaultUserServiceArgs(component: ComponentName): Any {
        var args = KmReflect.newInstance("$CLASS\$UserServiceArgs", component)
        args = KmReflect.call(args, "processNameSuffix", "km-authority")!!
        args = KmReflect.call(args, "tag", "leftcar-km-v1")!!
        args = KmReflect.call(args, "version", 2)!!
        args = KmReflect.call(args, "daemon", false)!!
        args = KmReflect.call(args, "debuggable", false)!!
        return args
    }

    private fun defaultBindUserService(args: Any, connection: ServiceConnection) {
        type.getMethod(
            "bindUserService",
            KmReflect.clazz("$CLASS\$UserServiceArgs"),
            ServiceConnection::class.java,
        ).invoke(null, args, connection)
    }

    private fun defaultUnbindUserService(args: Any, connection: ServiceConnection, remove: Boolean) {
        type.getMethod(
            "unbindUserService",
            KmReflect.clazz("$CLASS\$UserServiceArgs"),
            ServiceConnection::class.java,
            Boolean::class.javaPrimitiveType,
        ).invoke(null, args, connection, remove)
    }

    private fun invokeStatic(method: String, vararg args: Any?): Any? {
        val argTypes = args.map { arg ->
            when (arg) {
                is Boolean -> Boolean::class.javaPrimitiveType!!
                is Int -> Int::class.javaPrimitiveType!!
                is Long -> Long::class.javaPrimitiveType!!
                null -> Any::class.java
                else -> arg.javaClass
            }
        }.toTypedArray()
        return type.getMethod(method, *argTypes).invoke(null, *args)
    }

    private fun listener(simpleName: String, methodName: String, handle: (Array<out Any?>?) -> Any?): Any {
        val iface = KmReflect.clazz("$CLASS\$$simpleName")
        return KmReflect.proxy(iface) { name, args -> if (name == methodName) handle(args) else null }
    }
}
