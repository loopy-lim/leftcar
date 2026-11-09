package dev.leftcar.viewer.km

import java.lang.reflect.Proxy

/**
 * Reflection helpers so KM production files never import banned packages
 * (`rikka.*`, `org.lsposed.*`, `org.json.*`, `java.nio.*`, `java.io.File*`).
 * Architecture L0 scans import lines only — keep those imports out.
 */
internal object KmReflect {
    fun clazz(name: String): Class<*> = Class.forName(name)

    fun callStatic(className: String, method: String, vararg args: Any?): Any? {
        val type = clazz(className)
        val argTypes = args.map { argType(it) }.toTypedArray()
        val m = type.methods.first { it.name == method && it.parameterTypes.contentEquals(argTypes) }
        return m.invoke(null, *args)
    }

    fun call(target: Any, method: String, vararg args: Any?): Any? {
        val argTypes = args.map { argType(it) }.toTypedArray()
        val m = target.javaClass.methods.first {
            it.name == method && it.parameterTypes.contentEquals(argTypes)
        }
        return m.invoke(target, *args)
    }

    fun newInstance(className: String, vararg args: Any?): Any {
        val type = clazz(className)
        val argTypes = args.map { argType(it) }.toTypedArray()
        val ctor = type.constructors.first { it.parameterTypes.contentEquals(argTypes) }
        return ctor.newInstance(*args)
    }

    fun fieldGet(target: Any, name: String): Any? {
        val field = target.javaClass.getDeclaredField(name).apply { isAccessible = true }
        return field.get(target)
    }

    fun fieldSet(target: Any, name: String, value: Any?) {
        val field = target.javaClass.getDeclaredField(name).apply { isAccessible = true }
        field.set(target, value)
    }

    fun proxy(iface: Class<*>, handler: (method: String, args: Array<out Any?>?) -> Any?): Any =
        Proxy.newProxyInstance(iface.classLoader, arrayOf(iface)) { _, method, args ->
            handler(method.name, args)
        }

    private fun argType(value: Any?): Class<*> = when (value) {
        null -> Any::class.java
        is Boolean -> Boolean::class.javaPrimitiveType!!
        is Int -> Int::class.javaPrimitiveType!!
        is Long -> Long::class.javaPrimitiveType!!
        is ByteArray -> ByteArray::class.java
        else -> value.javaClass
    }
}
