package dev.leftcar.viewer.km

import android.accessibilityservice.AccessibilityServiceInfo
import android.content.Context
import android.os.Binder
import android.os.Build
import android.os.IBinder
import android.os.Process
import android.os.SystemClock
import android.system.Os
import android.system.OsConstants
import java.io.FileDescriptor
import java.io.File
import java.math.BigInteger
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** Runs only as Shizuku's shell UserService. The application UID is resolved from its Context. */
class KmInputAuthority(context: Context) : IKmInputAuthority.Stub() {
    private val authorizedUid = context.applicationInfo.uid
    private val ownObservation = KmOwnObservationAuthority(authorizedUid, context.packageName)
    private val hid = KmHidReports()
    private var mouse: FileDescriptor? = null
    private var keyboard: FileDescriptor? = null
    private var owner: IBinder? = null
    private var physicalMouse: FileDescriptor? = null
    private var mouseCallback: IKmMouseCallback? = null
    private var physicalButtons = 0
    private var residualX = 0.0
    private var residualY = 0.0
    private var residualWheelX = 0.0
    private var residualWheelY = 0.0
    private val lifetime = KmAuthorityLifetime(authorizedUid, SystemClock::elapsedRealtime, ::neutralClose)
    private val death = IBinder.DeathRecipient { synchronized(this) { lifetime.disconnected() } }
    private val timer = Executors.newSingleThreadScheduledExecutor { r -> Thread(r, "km-authority-watchdog").apply { isDaemon = true } }

    init {
        require(authorizedUid >= 10000 && Process.myUid() == 2000) { "KM requires app-bound shell authority" }
        timer.scheduleAtFixedRate({ synchronized(this) {
            lifetime.tick()
            mouse?.let(::drain); keyboard?.let(::drain); drainPhysicalMouse()
        } }, 8, 8, TimeUnit.MILLISECONDS)
    }

    private fun authorize() { lifetime.authorize(Binder.getCallingUid()) }

    @Synchronized override fun prepare(owner: IBinder): Boolean {
        authorize()
        neutralClose()
        return try {
            check(observationSupported()) { "Android observed motion API unavailable" }
            this.owner = owner
            owner.linkToDeath(death, 0)
            mouse = createDevice("Leftcar KM Mouse", KmHidReports.MOUSE_DESCRIPTOR)
            keyboard = createDevice("Leftcar KM Keyboard", KmHidReports.KEYBOARD_DESCRIPTOR)
            lifetime.prepared()
            true
        } catch (_: Exception) { neutralClose(); false }
    }

    @Synchronized override fun heartbeat() {
        authorize(); check(mouse != null && keyboard != null) { "KM authority expired" }
        lifetime.heartbeat()
    }

    @Synchronized override fun inject(kind: Int, code: Int, down: Boolean, x: Double, y: Double) {
        authorize()
        check(mouse != null && keyboard != null) { "KM authority unavailable" }
        require(x.isFinite() && y.isFinite() && kotlin.math.abs(x) <= 65536 && kotlin.math.abs(y) <= 65536)
        when (kind) {
            MOVE -> {
                residualX += x; residualY += y
                while (kotlin.math.abs(residualX) >= 1 || kotlin.math.abs(residualY) >= 1) {
                    val dx = residualX.toInt().coerceIn(-32767, 32767)
                    val dy = residualY.toInt().coerceIn(-32767, 32767)
                    writeReport(mouse!!, hid.mouse(dx, dy, 0, 0))
                    residualX -= dx; residualY -= dy
                }
            }
            BUTTON -> { require(code in 0..4); hid.button(1 shl code, down); writeReport(mouse!!, hid.mouse(0, 0, 0, 0)) }
            WHEEL -> {
                residualWheelX += x; residualWheelY += y
                while (kotlin.math.abs(residualWheelX) >= 1 || kotlin.math.abs(residualWheelY) >= 1) {
                    val dx = residualWheelX.toInt().coerceIn(-127, 127)
                    val dy = residualWheelY.toInt().coerceIn(-127, 127)
                    writeReport(mouse!!, hid.mouse(0, 0, dy, dx))
                    residualWheelX -= dx; residualWheelY -= dy
                }
            }
            KEY -> { hid.key(code, down); writeReport(keyboard!!, hid.keyboard()) }
            else -> error("Unknown KM input")
        }
    }

    @Synchronized override fun releaseAll() {
        authorize()
        try { neutral(true) } catch (e: Exception) { neutralClose(); throw e }
    }
    @Synchronized override fun close() { authorize(); neutralClose() }
    @Synchronized override fun destroy() {
        check(Binder.getCallingUid() == authorizedUid || Binder.getCallingUid() == 2000)
        neutralClose(); timer.shutdownNow(); kotlin.system.exitProcess(0)
    }

    @Synchronized override fun probeMouse(name: String): Boolean {
        authorize()
        return runCatching {
            mouseIoctl()
            val fd = Os.open(physicalMousePath(name), OsConstants.O_RDONLY or OsConstants.O_NONBLOCK, 0)
            Os.close(fd)
            true
        }.getOrDefault(false)
    }

    @Synchronized override fun captureMouse(name: String, callback: IKmMouseCallback): Boolean {
        authorize(); check(mouse != null)
        closePhysicalMouse()
        return try {
            val fd = Os.open(physicalMousePath(name), OsConstants.O_RDONLY or OsConstants.O_NONBLOCK, 0)
            physicalMouse = fd
            // libcore ioctlInt supplies a non-null int pointer; EVIOCGRAB tests non-null.
            // close(fd), never another ioctlInt(0), releases this exclusive ownership.
            mouseIoctl().invoke(null, fd, 0x40044590)
            physicalButtons = 0
            mouseCallback = callback
            true
        } catch (_: Exception) { closePhysicalMouse(); false }
    }

    @Synchronized override fun releaseMouse() { authorize(); closePhysicalMouse() }

    override fun observedSources(info: AccessibilityServiceInfo): Int {
        authorize(); check(observationSupported())
        return AccessibilityServiceInfo::class.java.getMethod("getObservedMotionEventSources").invoke(info) as Int
    }
    override fun mouseCaptureSupported(): Boolean { authorize(); return runCatching { mouseIoctl(); Process.is64Bit() }.getOrDefault(false) }

    private fun mouseIoctl() = Os::class.java.getMethod("ioctlInt", FileDescriptor::class.java, Int::class.javaPrimitiveType)

    private fun physicalMousePath(name: String): String {
        require(name.isNotBlank() && name.length <= 256 && !name.startsWith("Leftcar KM "))
        val candidates = File("/dev/input").listFiles().orEmpty().filter { node ->
            if (!node.name.matches(Regex("event[0-9]+"))) return@filter false
            val sys = File("/sys/class/input/${node.name}/device")
            runCatching {
                fun bits(part: String) = File(sys, "capabilities/$part").readText().trim().split(Regex("\\s+"))
                    .fold(BigInteger.ZERO) { value, word -> value.shiftLeft(64).or(BigInteger(word, 16)) }
                File(sys, "name").readText().trim() == name &&
                    bits("rel").testBit(0) && bits("rel").testBit(1) &&
                    bits("key").testBit(272) && !bits("key").testBit(330) && bits("abs") == BigInteger.ZERO &&
                    OsConstants.S_ISCHR(Os.stat(node.path).st_mode)
            }.getOrDefault(false)
        }
        // Ambiguous same-name devices must not grab an unrelated input node.
        require(candidates.size == 1) { "Physical mouse identity unavailable" }
        return candidates.single().path
    }

    private fun drainPhysicalMouse() {
        val fd = physicalMouse ?: return
        try {
            val bytes = ByteArray(24 * 64) // Supported APK architecture is arm64: timeval is 16 bytes.
            repeat(8) {
                val count = Os.read(fd, bytes, 0, bytes.size)
                check(count > 0 && count % 24 == 0)
                val data = ByteBuffer.wrap(bytes, 0, count).order(ByteOrder.nativeOrder())
                while (data.remaining() >= 24) {
                    data.position(data.position() + 16)
                    val type = data.short.toInt() and 65535
                    val code = data.short.toInt() and 65535
                    val value = data.int
                    when {
                        type == 2 && code in setOf(0, 1, 6, 8) -> mouseCallback?.onMouse(
                            if (code == 0) value.toDouble() else 0.0,
                            if (code == 1) value.toDouble() else 0.0,
                            if (code == 6) value.toDouble() else 0.0,
                            if (code == 8) value.toDouble() else 0.0, physicalButtons)
                        type == 1 && code in 272..276 -> {
                            val bit = 1 shl (code - 272)
                            physicalButtons = if (value == 0) physicalButtons and bit.inv() else physicalButtons or bit
                            mouseCallback?.onMouse(0.0, 0.0, 0.0, 0.0, physicalButtons)
                        }
                        type == 0 && code == 3 -> error("Physical input overflow")
                    }
                }
            }
        } catch (e: android.system.ErrnoException) {
            if (e.errno != OsConstants.EAGAIN && e.errno != OsConstants.EINTR) physicalLost()
        } catch (_: Exception) { physicalLost() }
    }

    private fun physicalLost() {
        val callback = mouseCallback
        closePhysicalMouse()
        runCatching { callback?.onCaptureLost() }
    }

    private fun closePhysicalMouse() {
        physicalMouse?.let { runCatching { Os.close(it) } }
        physicalMouse = null; mouseCallback = null; physicalButtons = 0
    }

    private val observationCalls = Any()
    override fun configureOwnObservation(connection: IBinder, mode: Int): Boolean {
        val caller = Binder.getCallingUid()
        // Never hold the input/watchdog monitor across an external connection RPC.
        return synchronized(observationCalls) {
            ownObservation.apply(caller, mode, {
                synchronized(this) { mouse != null && keyboard != null && owner?.isBinderAlive == true }
            }) { KmBorrowedObservationEndpoint(connection) }
        }
    }

    private fun observedSetter() = AccessibilityServiceInfo::class.java.getMethod("setObservedMotionEventSources", Int::class.javaPrimitiveType)
    private fun observationSupported(): Boolean = runCatching {
        Build.VERSION.SDK_INT >= 36 &&
            Class.forName("android.view.accessibility.Flags").getMethod("motionEventObserving").invoke(null) == true &&
            observedSetter() != null
    }.getOrDefault(false)

    private fun neutral(strict: Boolean = false) {
        hid.release(); residualX = 0.0; residualY = 0.0; residualWheelX = 0.0; residualWheelY = 0.0
        var failure: Exception? = null
        mouse?.let { try { writeReport(it, hid.mouse(0, 0, 0, 0)) } catch (e: Exception) { failure = e } }
        keyboard?.let { try { writeReport(it, hid.keyboard()) } catch (e: Exception) { failure = e } }
        if (strict) failure?.let { throw it }
    }

    private fun neutralClose() {
        lifetime.closed()
        neutral()
        closePhysicalMouse()
        listOfNotNull(mouse, keyboard).forEach { runCatching { Os.close(it) } }
        mouse = null; keyboard = null
        owner?.let { runCatching { it.unlinkToDeath(death, 0) } }; owner = null
    }

    // Linux uapi/linux/uhid.h. Protocol reference: scrcpy UhidManager (Apache-2.0).
    // This independently written implementation uses fixed descriptors and one app-bound session.
    private fun createDevice(name: String, descriptor: ByteArray): FileDescriptor {
        val fd = Os.open("/dev/uhid", OsConstants.O_RDWR or OsConstants.O_NONBLOCK, 0)
        try {
            val data = ByteBuffer.allocate(280 + descriptor.size).order(ByteOrder.nativeOrder())
            data.putInt(11).put(name.toByteArray(Charsets.US_ASCII))
            data.position(260)
            data.putShort(descriptor.size.toShort()).putShort(0x06).putInt(0x1209).putInt(0x1eaf).putInt(1).putInt(0).put(descriptor)
            writeWhole(fd, data.array())
            return fd
        } catch (e: Exception) { Os.close(fd); throw e }
    }

    private fun writeReport(fd: FileDescriptor, report: ByteArray) {
        writeWhole(fd, ByteBuffer.allocate(6 + report.size).order(ByteOrder.nativeOrder())
            .putInt(12).putShort(report.size.toShort()).put(report).array())
    }
    private fun writeWhole(fd: FileDescriptor, bytes: ByteArray) { check(Os.write(fd, bytes, 0, bytes.size) == bytes.size) }
    private fun drain(fd: FileDescriptor) {
        try {
            repeat(16) {
                val data = ByteArray(4380)
                val count = Os.read(fd, data, 0, data.size)
                if (count < 4) return
                val buffer = ByteBuffer.wrap(data).order(ByteOrder.nativeOrder())
                val type = buffer.int
                // Reply unsupported control reports; output/LED reports contain no user key data.
                if ((type == 9 || type == 13) && count >= 8) {
                    val reply = ByteBuffer.allocate(if (type == 9) 12 else 10).order(ByteOrder.nativeOrder())
                    reply.putInt(if (type == 9) 10 else 14).putInt(buffer.int).putShort(OsConstants.EOPNOTSUPP.toShort())
                    if (type == 9) reply.putShort(0)
                    writeWhole(fd, reply.array())
                }
            }
        } catch (e: android.system.ErrnoException) {
            if (e.errno != OsConstants.EAGAIN && e.errno != OsConstants.EINTR) neutralClose()
        } catch (_: Exception) { neutralClose() }
    }

    companion object { const val MOVE = 0; const val BUTTON = 1; const val WHEEL = 2; const val KEY = 3 }
}
