package dev.leftcar.viewer.diagnostics

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** sample 레코드 JSON 형식을 고정한다 — docs/06 필드명, TS 스키마와 정렬. */
class DiagnosticsFormatsTest {
  @Test
  fun formats_a_full_sample_record_in_schema_order() {
    val line = DiagnosticsSampler.formatSampleValues(
      DiagnosticsSample(
        batteryPercent = 87,
        charging = false,
        plugged = 0,
        voltageMv = 4123,
        tempDeciC = 284,
        thermalStatus = 0,
        thermalHeadroom = 1.5,
      ),
      nowMs = 1_760_000_000_000,
      upMs = 123_456,
    )
    assertEquals(
      "{\"ts\":1760000000000,\"up_ms\":123456,\"kind\":\"sample\"," +
        "\"battery_percent\":87,\"charging\":false,\"plugged\":0," +
        "\"voltage_mv\":4123,\"temp_deci_c\":284," +
        "\"thermal_status\":0,\"thermal_headroom\":1.5," +
        "\"release_gap_p50_ms\":null,\"release_gap_p95_ms\":null," +
        "\"release_gap_max_ms\":null,\"release_gap_count\":null}",
      line,
    )
  }

  @Test
  fun formats_cadence_lanes_when_a_window_is_present() {
    val line = DiagnosticsSampler.formatSampleValues(
      DiagnosticsSample(
        batteryPercent = 87,
        charging = false,
        plugged = 0,
        voltageMv = null,
        tempDeciC = null,
        thermalStatus = null,
        thermalHeadroom = null,
        releaseGapP50Ms = 16,
        releaseGapP95Ms = 33,
        releaseGapMaxMs = 120,
        releaseGapCount = 240,
      ),
      nowMs = 1,
      upMs = 2,
    )
    assertTrue(line.contains("\"release_gap_p50_ms\":16"))
    assertTrue(line.contains("\"release_gap_p95_ms\":33"))
    assertTrue(line.contains("\"release_gap_max_ms\":120"))
    assertTrue(line.contains("\"release_gap_count\":240"))
  }

  @Test
  fun unpacks_cadence_pack_lanes_and_treats_zero_as_absent() {
    val packed = (240L shl 48) or (16L shl 32) or (33L shl 16) or 120L
    val window = DiagnosticsSampler.unpackCadence(packed)
    assertEquals(240, window?.count)
    assertEquals(16, window?.p50Ms)
    assertEquals(33, window?.p95Ms)
    assertEquals(120, window?.maxMs)
    // 전부 0은 데이터 없음(input_rtt_ms 센티널 선례) — 0으로 오염시키지 않는다.
    assertNull(DiagnosticsSampler.unpackCadence(0L))
  }

  @Test
  fun renders_unavailable_values_as_null_not_zero() {
    // 값 없음을 0으로 쓰면 방전·과열 판독이 오염된다 — null로 남긴다.
    val line = DiagnosticsSampler.formatSampleValues(
      DiagnosticsSample(null, false, 0, null, null, null, null),
      nowMs = 1,
      upMs = 2,
    )
    assertEquals(
      "{\"ts\":1,\"up_ms\":2,\"kind\":\"sample\",\"battery_percent\":null," +
        "\"charging\":false,\"plugged\":0,\"voltage_mv\":null,\"temp_deci_c\":null," +
        "\"thermal_status\":null,\"thermal_headroom\":null," +
        "\"release_gap_p50_ms\":null,\"release_gap_p95_ms\":null," +
        "\"release_gap_max_ms\":null,\"release_gap_count\":null}",
      line,
    )
  }

  @Test
  fun treats_non_finite_headroom_as_unavailable() {
    val line = DiagnosticsSampler.formatSampleValues(
      DiagnosticsSample(50, true, 1, 3900, 300, 1, Double.NaN),
      nowMs = 1,
      upMs = 2,
    )
    assertTrue(line.contains("\"thermal_headroom\":null"))
  }

  @Test
  fun records_fit_the_1024_byte_limit_by_construction() {
    val line = DiagnosticsSampler.formatSampleValues(
      DiagnosticsSample(100, true, 4, 5000, 450, 7, 999.75),
      nowMs = Long.MAX_VALUE,
      upMs = Long.MAX_VALUE,
    )
    assertTrue(line.toByteArray(Charsets.UTF_8).size <= DiagnosticsLimits.MAX_RECORD_BYTES)
  }

  @Test
  fun crash_kind_regex_reads_kinds_from_records() {
    // 스토어 counts가 종류를 읽는 정규식 — sample·session·crash만 인정한다.
    assertEquals("sample", DiagnosticsStore.KIND_OF.find("{\"kind\":\"sample\"}")?.groupValues?.get(1))
    assertEquals("crash", DiagnosticsStore.KIND_OF.find("{\"kind\":\"crash\"}")?.groupValues?.get(1))
    assertNull(DiagnosticsStore.KIND_OF.find("{\"kind\":\"UPPER\"}"))
  }
}
