package dev.leftcar.viewer.diagnostics

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

/** §16 저장 규격 상수가 TS 측(src/diagnostics.ts)과 같은 값을 가리키는지 고정한다. */
class DiagnosticsLimitsTest {
  @Test
  fun pins_the_doc07_storage_numbers() {
    assertEquals(15_000L, DiagnosticsLimits.SAMPLE_INTERVAL_MS)
    assertEquals(1024, DiagnosticsLimits.MAX_RECORD_BYTES)
    assertEquals(5L * 1024 * 1024, DiagnosticsLimits.MAX_FILE_BYTES)
    assertEquals(1, DiagnosticsLimits.BACKUP_GENERATIONS)
    assertEquals(120_000L, DiagnosticsLimits.BOOT_DETECTION_UPTIME_MS)
  }
}

class DiagnosticsStoreTest {
  @get:Rule
  val tmp = TemporaryFolder()

  private fun activeFile() = java.io.File(tmp.root, "leftcar-diagnostics.ndjson")
  private fun backupFile() = java.io.File(tmp.root, "leftcar-diagnostics.ndjson.1")

  private fun store(maxFileBytes: Long = DiagnosticsLimits.MAX_FILE_BYTES) =
    DiagnosticsStore(tmp.root, nowMs = { 1_760_000_000_000 }, upMs = { 5_000 }, maxFileBytes = maxFileBytes)

  @Test
  fun appends_records_as_ndjson_lines() {
    val store = store()
    assertTrue(store.append("{\"kind\":\"sample\",\"battery_percent\":87}"))
    assertTrue(store.append("{\"kind\":\"sample\",\"battery_percent\":86}"))
    assertEquals(
      "{\"kind\":\"sample\",\"battery_percent\":87}\n{\"kind\":\"sample\",\"battery_percent\":86}\n",
      activeFile().readText(),
    )
  }

  @Test
  fun rejects_records_over_the_1024_byte_limit_excluding_newline() {
    val store = store()
    assertTrue(store.append("a".repeat(1024)))
    assertFalse(store.append("a".repeat(1025)))
    assertEquals(1025, activeFile().length())
  }

  @Test
  fun measures_the_limit_in_utf8_bytes_not_characters() {
    val store = store()
    // 3바이트 문자 341자 = 1023바이트는 통과, 342자 = 1026바이트는 거절.
    assertTrue(store.append("가".repeat(341)))
    assertFalse(store.append("가".repeat(342)))
  }

  @Test
  fun rejects_lines_containing_newlines() {
    val store = store()
    assertFalse(store.append("line1\nline2"))
    assertFalse(activeFile().exists())
  }

  @Test
  fun rotates_at_the_size_limit_and_keeps_one_backup_generation() {
    val store = store(maxFileBytes = 1_000)
    assertTrue(store.append("L1".repeat(200))) // 401바이트
    assertTrue(store.append("L2".repeat(200))) // 누적 802바이트
    // 세 번째 기록이 상한을 넘기 직전에 회전한다.
    assertTrue(store.append("L3".repeat(200)))
    assertEquals("L1".repeat(200) + "\n" + "L2".repeat(200) + "\n", backupFile().readText())
    assertEquals("L3".repeat(200) + "\n", activeFile().readText())

    // 두 번째 회전 시 이전 백업 세대는 사라진다(한 세대만 유지).
    assertTrue(store.append("L4".repeat(200)))
    assertTrue(store.append("L5".repeat(200)))
    assertTrue(store.append("L6".repeat(200)))
    assertEquals("L3".repeat(200) + "\n" + "L4".repeat(200) + "\n", backupFile().readText())
    assertFalse(backupFile().readText().contains("L1"))
  }

  @Test
  fun latches_closed_after_a_filesystem_error_and_keeps_failing_silently() {
    // 활성 파일 경로가 디렉터리면 FileOutputStream 열기가 확정적으로 실패한다.
    activeFile().mkdir()
    val store = store()
    assertFalse(store.append("hello"))
    assertFalse(store.append("again"))
  }

  @Test
  fun manual_stop_writing_latches_the_same_way() {
    val store = store()
    assertTrue(store.append("before"))
    store.stopWriting()
    assertFalse(store.append("after"))
    assertEquals("before\n", activeFile().readText())
  }

  @Test
  fun crash_record_carries_the_class_name_only() {
    val store = store()
    assertTrue(store.appendCrash("java.lang.NullPointerException"))
    assertEquals(
      "{\"ts\":1760000000000,\"up_ms\":5000,\"kind\":\"crash\",\"exception\":\"java.lang.NullPointerException\"}\n",
      activeFile().readText(),
    )
  }

  @Test
  fun crash_record_sanitizes_quote_and_backslash_and_truncates() {
    val store = store()
    assertTrue(store.appendCrash("Weird\"Class\\Path " + "x".repeat(400)))
    val line = activeFile().readText().trim()
    assertTrue(line.startsWith("{\"ts\":1760000000000,\"up_ms\":5000,\"kind\":\"crash\",\"exception\":\""))
    assertFalse(line.contains("\n"))
    // 400자 꼬리는 잘린다 — 클래스명 이상의 임의 텍스트가 늘어나지 않는다.
    assertTrue(line.length < 400)
  }

  @Test
  fun export_writes_backup_then_active_in_time_order() {
    val store = store(maxFileBytes = 1_000)
    store.append("L1")
    store.append("L2")
    store.append("L3") // 회전 발생: .1=L1,L2 / 활성=L3
    val sink = CapturingSink()
    val name = store.exportTo(sink)
    assertTrue(name.startsWith("leftcar-diagnostics-"))
    assertTrue(name.endsWith(".ndjson"))
    assertEquals("L1\nL2\nL3\n", sink.out.toString("UTF-8"))
  }

  @Test
  fun export_survives_missing_files() {
    val store = store()
    val sink = CapturingSink()
    val name = store.exportTo(sink)
    assertTrue(name.startsWith("leftcar-diagnostics-"))
    assertEquals("", sink.out.toString("UTF-8"))
  }

  @Test
  fun counts_track_successful_appends_per_kind() {
    val store = store()
    store.append("{\"ts\":1,\"kind\":\"sample\"}")
    store.append("{\"ts\":2,\"kind\":\"sample\"}")
    store.append("{\"ts\":3,\"kind\":\"session\"}")
    store.appendCrash("java.lang.IllegalStateException")
    assertEquals(mapOf("sample" to 2, "session" to 1, "crash" to 1), store.counts())
    // 거절된 기록은 세지 않는다.
    store.append("a".repeat(2000))
    assertEquals(mapOf("sample" to 2, "session" to 1, "crash" to 1), store.counts())
  }
}

private class CapturingSink : ExportSink {
  val out = java.io.ByteArrayOutputStream()
  var openedName: String? = null
  override fun open(name: String): java.io.OutputStream {
    openedName = name
    return out
  }
}
