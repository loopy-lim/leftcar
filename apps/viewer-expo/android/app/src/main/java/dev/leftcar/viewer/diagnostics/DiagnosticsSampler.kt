package dev.leftcar.viewer.diagnostics

import android.os.BatteryManager
import android.os.Bundle

/** 한 틱의 플랫폼 스냅샷. 값은 숫자·불리언뿐이다(§16 허용 필드). */
data class DiagnosticsSample(
  val batteryPercent: Int?,
  val charging: Boolean,
  val plugged: Int,
  val voltageMv: Int?,
  val tempDeciC: Int?,
  val thermalStatus: Int?,
  val thermalHeadroom: Double?,
  val releaseGapP50Ms: Int? = null,
  val releaseGapP95Ms: Int? = null,
  val releaseGapMaxMs: Int? = null,
  val releaseGapCount: Int? = null,
)

/** ViewerNative.streamCadence pack 해체 결과. 값 단위는 ms다. */
data class CadenceWindowValues(
  val count: Int,
  val p50Ms: Int,
  val p95Ms: Int,
  val maxMs: Int,
)

/**
 * 고정 주기 전원·thermal 샘플러. JS 수명과 무관하게 동작해야 하므로 판단 없는
 * 상수 필드만 남긴다(docs/03 — Kotlin shim은 정책을 갖지 않는다). 매 틱 기록이
 * 원칙이다 — 기록 공백 자체가 사망 증거가 된다.
 */
class DiagnosticsSampler(
  val intervalMs: Long = DiagnosticsLimits.SAMPLE_INTERVAL_MS,
  private val readSnapshot: () -> Bundle?,
  private val thermalStatus: () -> Int?,
  private val thermalHeadroom: () -> Double?,
  private val streamCadence: () -> Long? = { null },
  private val nowMs: () -> Long,
  private val upMs: () -> Long,
  private val append: (String) -> Boolean,
  private val schedule: (Runnable, Long) -> Unit,
  private val onSample: (DiagnosticsSample) -> Unit = {},
) {
  private var running = false

  fun start() {
    if (running) return
    running = true
    tick()
  }

  fun stop() {
    running = false
  }

  private fun tick() {
    if (!running) return
    val cadence = streamCadence()?.let { unpackCadence(it) }
    val sample = sampleFromExtras(readSnapshot(), thermalStatus(), thermalHeadroom())
      .copy(
        releaseGapP50Ms = cadence?.p50Ms,
        releaseGapP95Ms = cadence?.p95Ms,
        releaseGapMaxMs = cadence?.maxMs,
        releaseGapCount = cadence?.count,
      )
    append(formatSampleValues(sample, nowMs = nowMs(), upMs = upMs()))
    // 판단 없는 전달 — 최신 스냅숏 보관(DiagnosticsRuntime)은 이 훅뿐이다.
    onSample(sample)
    schedule({ tick() }, intervalMs)
  }

  companion object {
    /**
     * ACTION_BATTERY_CHANGED extras → 스냅샷. 값이 없으면 0이 아니라 null로
     * 남긴다 — 방전·과열 판독이 0으로 오염되지 않게 하기 위해서다. 충전 판정은
     * CHARGING(2)·FULL(5)뿐이다(DISCHARGING 3 포함 금지).
     */
    fun sampleFromExtras(extras: Bundle?, thermalStatus: Int?, thermalHeadroom: Double?): DiagnosticsSample {
      if (extras == null) {
        return DiagnosticsSample(null, false, 0, null, null, thermalStatus, finite(thermalHeadroom))
      }
      val scale = extras.getInt(BatteryManager.EXTRA_SCALE, 0)
      val level = extras.getInt(BatteryManager.EXTRA_LEVEL, -1)
      val percent = if (scale > 0 && level >= 0) Math.round(level * 100.0 / scale).toInt() else null
      val status = extras.getInt(BatteryManager.EXTRA_STATUS, -1)
      val charging =
        status == BatteryManager.BATTERY_STATUS_CHARGING || status == BatteryManager.BATTERY_STATUS_FULL
      return DiagnosticsSample(
        batteryPercent = percent,
        charging = charging,
        plugged = extras.getInt(BatteryManager.EXTRA_PLUGGED, 0),
        voltageMv = extras.intIfPresent(BatteryManager.EXTRA_VOLTAGE),
        tempDeciC = extras.intIfPresent(BatteryManager.EXTRA_TEMPERATURE),
        thermalStatus = thermalStatus,
        thermalHeadroom = finite(thermalHeadroom),
      )
    }

    fun formatSample(snapshot: Bundle?, thermalStatus: Int?, thermalHeadroom: Double?, nowMs: Long, upMs: Long): String =
      formatSampleValues(
        sampleFromExtras(snapshot, thermalStatus, thermalHeadroom),
        nowMs,
        upMs,
      )

    /** sample 레코드 JSON 직렬화. JVM/Robolectric 테스트가 형식을 고정한다. */
    fun formatSampleValues(sample: DiagnosticsSample, nowMs: Long, upMs: Long): String =
      "{\"ts\":$nowMs,\"up_ms\":$upMs,\"kind\":\"sample\"," +
        "\"battery_percent\":${num(sample.batteryPercent)},\"charging\":${sample.charging}," +
        "\"plugged\":${sample.plugged},\"voltage_mv\":${num(sample.voltageMv)}," +
        "\"temp_deci_c\":${num(sample.tempDeciC)}," +
        "\"thermal_status\":${num(sample.thermalStatus)}," +
        "\"thermal_headroom\":${num(finite(sample.thermalHeadroom))}," +
        "\"release_gap_p50_ms\":${num(sample.releaseGapP50Ms)}," +
        "\"release_gap_p95_ms\":${num(sample.releaseGapP95Ms)}," +
        "\"release_gap_max_ms\":${num(sample.releaseGapMaxMs)}," +
        "\"release_gap_count\":${num(sample.releaseGapCount)}}"

    /**
     * ViewerNative.streamCadence pack 해체. 전부 0은 데이터 없음이다
     * (input_rtt_ms 센티널 선례) — 0으로 오염시키지 않고 null로 남긴다.
     */
    fun unpackCadence(packed: Long): CadenceWindowValues? {
      if (packed <= 0L) return null
      val count = ((packed ushr 48) and 0xFFFF).toInt()
      if (count == 0) return null
      return CadenceWindowValues(
        count = count,
        p50Ms = ((packed ushr 32) and 0xFFFF).toInt(),
        p95Ms = ((packed ushr 16) and 0xFFFF).toInt(),
        maxMs = (packed and 0xFFFF).toInt(),
      )
    }

    private fun num(value: Int?): String = value?.toString() ?: "null"

    private fun num(value: Double?): String = value?.toString() ?: "null"

    private fun finite(value: Double?): Double? = if (value != null && value.isFinite()) value else null

    private fun Bundle.intIfPresent(key: String): Int? =
      if (containsKey(key)) getInt(key) else null
  }
}
