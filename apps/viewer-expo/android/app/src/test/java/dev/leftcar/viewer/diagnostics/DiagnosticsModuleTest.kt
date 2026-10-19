package dev.leftcar.viewer.diagnostics

import android.content.Intent
import android.os.BatteryManager
import android.os.Bundle

import com.facebook.react.bridge.Callback
import com.facebook.react.bridge.CatalystInstance
import com.facebook.react.bridge.JavaScriptContextHolder
import com.facebook.react.bridge.JavaScriptModule
import com.facebook.react.bridge.NativeModule
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.UIManager
import com.facebook.react.bridge.WritableMap
import com.facebook.react.turbomodule.core.interfaces.CallInvokerHolder

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

import java.util.concurrent.Executor

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE)
class DiagnosticsModuleTest {
    @get:Rule
    val tmp = TemporaryFolder()

    private fun storeWith(maxFileBytes: Long = DiagnosticsLimits.MAX_FILE_BYTES) =
        DiagnosticsStore(tmp.root, nowMs = { 1_760_000_000_000 }, upMs = { 5_000 }, maxFileBytes = maxFileBytes)

    private fun activeFile() = java.io.File(tmp.root, "leftcar-diagnostics.ndjson")

    private fun module(store: DiagnosticsStore, sink: ExportSink? = null): DiagnosticsModule {
        val context = TestReactContext(RuntimeEnvironment.getApplication())
        return DiagnosticsModule(
            context,
            storeOf = { store },
            sinkOf = { sink ?: error("sink 미주입") },
            exportExecutor = Executor { it.run() },
        )
    }

    private fun batteryExtras(status: Int = BatteryManager.BATTERY_STATUS_CHARGING): Bundle = Bundle().apply {
        putInt(BatteryManager.EXTRA_LEVEL, 87)
        putInt(BatteryManager.EXTRA_SCALE, 100)
        putInt(BatteryManager.EXTRA_STATUS, status)
        putInt(BatteryManager.EXTRA_PLUGGED, 4)
        putInt(BatteryManager.EXTRA_VOLTAGE, 4123)
        putInt(BatteryManager.EXTRA_TEMPERATURE, 284)
    }

    @Test
    fun `module is named Diagnostics`() {
        assertEquals("Diagnostics", module(storeWith()).name)
    }

    @Test
    fun `appendRecord writes valid lines and refuses oversized ones`() {
        val store = storeWith()
        val m = module(store)

        val ok = RecordingPromise()
        m.appendRecord("{\"kind\":\"session\",\"event\":\"foreground\"}", ok)
        assertEquals(true, ok.resolved)

        val tooBig = RecordingPromise()
        m.appendRecord("x".repeat(2000), tooBig)
        assertEquals(false, tooBig.resolved)
        assertEquals(
            "{\"kind\":\"session\",\"event\":\"foreground\"}\n",
            activeFile().readText(),
        )
    }

    @Test
    fun `getSummary reports per-kind counts`() {
        val store = storeWith()
        store.append("{\"ts\":1,\"kind\":\"sample\"}")
        store.append("{\"ts\":2,\"kind\":\"sample\"}")
        store.appendCrash("java.lang.RuntimeException")

        val promise = RecordingPromise()
        module(store).getSummary(promise)

        val map = promise.resolved as WritableMap
        assertEquals(2, map.getInt("sample"))
        assertEquals(1, map.getInt("crash"))
    }

    @Test
    fun `exportToDownloads resolves the display name through the injected sink`() {
        val store = storeWith()
        store.append("{\"ts\":1,\"kind\":\"sample\"}")
        val sink = CapturingExportSink()
        val promise = RecordingPromise()
        module(store, sink).exportToDownloads(promise)

        assertFalse(promise.rejected)
        assertTrue(
            "이름은 접두어·시각·확장자 형식을 따른다: ${promise.resolved}",
            (promise.resolved as String).matches(Regex("leftcar-diagnostics-\\d{8}-\\d{6}\\.ndjson")),
        )
        assertEquals(promise.resolved, sink.openedName)
        assertEquals("{\"ts\":1,\"kind\":\"sample\"}\n", sink.out.toString("UTF-8"))
    }

    @Test
    fun `sampler writes one sample per tick with the pinned schema`() {
        val store = storeWith()
        val scheduled = mutableListOf<Runnable>()
        val sampler = DiagnosticsSampler(
            intervalMs = DiagnosticsLimits.SAMPLE_INTERVAL_MS,
            readSnapshot = { batteryExtras() },
            thermalStatus = { 0 },
            thermalHeadroom = { 1.5 },
            nowMs = { 1_760_000_000_000 },
            upMs = { 123_456 },
            append = { line -> store.append(line) },
            schedule = { task, _ -> scheduled.add(task) },
        )
        sampler.start()

        assertEquals(1, scheduled.size)
        assertEquals(
            "{\"ts\":1760000000000,\"up_ms\":123456,\"kind\":\"sample\"," +
                "\"battery_percent\":87,\"charging\":true,\"plugged\":4," +
                "\"voltage_mv\":4123,\"temp_deci_c\":284," +
                "\"thermal_status\":0,\"thermal_headroom\":1.5}",
            activeFile().readText().trim(),
        )
        sampler.stop()
    }

    @Test
    fun `sampler tolerates a null battery snapshot`() {
        val store = storeWith()
        val sampler = DiagnosticsSampler(
            intervalMs = 1L,
            readSnapshot = { null },
            thermalStatus = { null },
            thermalHeadroom = { null },
            nowMs = { 1 },
            upMs = { 2 },
            append = { line -> store.append(line) },
            schedule = { _, _ -> },
        )
        sampler.start()
        assertTrue(activeFile().readText().contains("\"battery_percent\":null"))
    }

    @Test
    fun `battery extras map to every field`() {
        val sample = DiagnosticsSampler.sampleFromExtras(batteryExtras(), 0, 1.5)
        assertEquals(87, sample.batteryPercent)
        assertEquals(true, sample.charging)
        assertEquals(4, sample.plugged)
        assertEquals(4123, sample.voltageMv)
        assertEquals(284, sample.tempDeciC)
        assertEquals(0, sample.thermalStatus)
        assertEquals(1.5, sample.thermalHeadroom!!, 0.0)
    }

    @Test
    fun `discharging is not charging and full is charging`() {
        // 안전 수정 고정 — STATUS >= 2 판정은 DISCHARGING(3)을 잘못 포함한다.
        assertFalse(DiagnosticsSampler.sampleFromExtras(batteryExtras(BatteryManager.BATTERY_STATUS_DISCHARGING), null, null).charging)
        assertTrue(DiagnosticsSampler.sampleFromExtras(batteryExtras(BatteryManager.BATTERY_STATUS_FULL), null, null).charging)
        assertFalse(DiagnosticsSampler.sampleFromExtras(batteryExtras(BatteryManager.BATTERY_STATUS_NOT_CHARGING), null, null).charging)
    }

    @Test
    fun `missing or zero scale leaves the percent unavailable`() {
        val noScale = Bundle().apply { putInt(BatteryManager.EXTRA_LEVEL, 50) }
        assertNull(DiagnosticsSampler.sampleFromExtras(noScale, null, null).batteryPercent)
        assertNull(DiagnosticsSampler.sampleFromExtras(null, null, null).batteryPercent)
        val empty = DiagnosticsSampler.sampleFromExtras(Bundle(), null, null)
        assertNull(empty.batteryPercent)
        assertEquals(0, empty.plugged)
        assertFalse(empty.charging)
    }

    @Test
    fun `boot detection flags a fresh elapsedRealtime`() {
        assertTrue(DiagnosticsRuntime.isFreshBoot(upMs = 0))
        assertTrue(DiagnosticsRuntime.isFreshBoot(upMs = DiagnosticsLimits.BOOT_DETECTION_UPTIME_MS - 1))
        assertFalse(DiagnosticsRuntime.isFreshBoot(upMs = DiagnosticsLimits.BOOT_DETECTION_UPTIME_MS))
        assertFalse(DiagnosticsRuntime.isFreshBoot(upMs = 3_600_000))
    }

    @Test
    fun `session start line carries version api level and boot flag`() {
        val line = DiagnosticsRuntime.sessionStartLine(
            appVersion = "0.1.10",
            osApi = 36,
            ts = 1_760_000_000_000,
            upMs = 45_000,
        )
        assertEquals(
            "{\"ts\":1760000000000,\"up_ms\":45000,\"kind\":\"session\"," +
                "\"event\":\"session_start\",\"app_version\":\"0.1.10\",\"os_api\":36," +
                "\"boot_detected\":true}",
            line,
        )
        val stale = DiagnosticsRuntime.sessionStartLine("0.1.10", 36, 1, 1_000_000)
        assertTrue(stale.contains("\"boot_detected\":false"))
    }

    @Test
    fun `session start line replaces hostile version strings with unknown`() {
        val line = DiagnosticsRuntime.sessionStartLine("1.0 192.168.0.1 \"quoted\"", 36, 1, 9_000)
        assertTrue(line.contains("\"app_version\":\"unknown\""))
        assertFalse(line.contains("192.168"))
        assertFalse(line.contains("quoted"))
    }
}

/** React 인스턴스 접근은 이 테스트 범위가 아니다 — 논리 없는 기계적 스텁. */
private class TestReactContext(context: android.content.Context) : ReactApplicationContext(context) {
    override fun <T : JavaScriptModule> getJSModule(type: Class<T>): T = error("unused in test")
    override fun <T : NativeModule> hasNativeModule(type: Class<T>): Boolean = false
    override fun getNativeModules(): Collection<NativeModule> = emptyList()
    override fun <T : NativeModule> getNativeModule(type: Class<T>): T? = null
    override fun getNativeModule(name: String): NativeModule? = null
    override fun getCatalystInstance(): CatalystInstance = error("unused in test")
    override fun hasActiveCatalystInstance(): Boolean = false
    override fun hasActiveReactInstance(): Boolean = false
    override fun hasCatalystInstance(): Boolean = false
    override fun hasReactInstance(): Boolean = false
    override fun destroy() {}
    override fun handleException(e: Exception) {}
    override fun isBridgeless(): Boolean = false
    override fun getJavaScriptContextHolder(): JavaScriptContextHolder? = null
    override fun getJSCallInvokerHolder(): CallInvokerHolder? = null
    override fun getFabricUIManager(): UIManager? = null
    override fun getSourceURL(): String? = null
    override fun registerSegment(segmentId: Int, path: String, callback: Callback) {}
}

private class RecordingPromise : Promise {
    var resolved: Any? = null
    var rejected = false
        private set

    override fun resolve(value: Any?) {
        if (resolved == null) resolved = value
    }

    override fun reject(code: String?, message: String?) {
        rejected = true
    }

    override fun reject(code: String?, throwable: Throwable?) {
        rejected = true
    }

    override fun reject(code: String?, message: String?, throwable: Throwable?) {
        rejected = true
    }

    override fun reject(throwable: Throwable) {
        rejected = true
    }

    override fun reject(throwable: Throwable, userInfo: WritableMap) {
        rejected = true
    }

    override fun reject(code: String?, userInfo: WritableMap) {
        rejected = true
    }

    override fun reject(code: String?, throwable: Throwable?, userInfo: WritableMap) {
        rejected = true
    }

    override fun reject(code: String?, message: String?, userInfo: WritableMap) {
        rejected = true
    }

    override fun reject(code: String?, message: String?, throwable: Throwable?, userInfo: WritableMap?) {
        rejected = true
    }

    override fun reject(message: String) {
        rejected = true
    }
}

private class CapturingExportSink : ExportSink {
    val out = java.io.ByteArrayOutputStream()
    var openedName: String? = null
    override fun open(name: String): java.io.OutputStream {
        openedName = name
        return out
    }
}
