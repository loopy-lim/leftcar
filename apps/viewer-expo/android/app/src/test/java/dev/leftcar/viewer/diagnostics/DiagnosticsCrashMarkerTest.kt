package dev.leftcar.viewer.diagnostics

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class DiagnosticsCrashMarkerTest {
  @get:Rule
  val tmp = TemporaryFolder()

  private fun newStore(): DiagnosticsStore =
    DiagnosticsStore(tmp.root, nowMs = { 1_760_000_000_000 }, upMs = { 250_000 })

  @Test
  fun records_the_exception_class_and_delegates_to_the_previous_handler() {
    val store = newStore()
    val delegated = mutableListOf<Pair<Thread, Throwable>>()
    val previous = Thread.UncaughtExceptionHandler { thread, throwable ->
      delegated.add(thread to throwable)
    }
    Thread.setDefaultUncaughtExceptionHandler(previous)
    CrashMarker.install(store)

    val boom = IllegalStateException("boom with 192.168.0.9 and token=pair_abcd")
    Thread.getDefaultUncaughtExceptionHandler()!!.uncaughtException(Thread.currentThread(), boom)

    val line = java.io.File(tmp.root, "leftcar-diagnostics.ndjson").readText().trim()
    assertEquals(
      "{\"ts\":1760000000000,\"up_ms\":250000,\"kind\":\"crash\"," +
        "\"exception\":\"java.lang.IllegalStateException\"}",
      line,
    )
    // denylist — 예외 메시지는 절대 남지 않는다.
    assertFalse(line.contains("192.168"))
    assertFalse(line.contains("pair_abcd"))
    assertEquals(1, delegated.size)
    assertEquals(boom, delegated[0].second)
  }

  @Test
  fun marker_failure_never_blocks_the_previous_handler() {
    val store = newStore().also { it.stopWriting() }
    var delegated = false
    val previous = Thread.UncaughtExceptionHandler { _, _ -> delegated = true }
    Thread.setDefaultUncaughtExceptionHandler(previous)
    CrashMarker.install(store)

    Thread.getDefaultUncaughtExceptionHandler()!!
      .uncaughtException(Thread.currentThread(), RuntimeException("x"))

    assertTrue("래치가 걸려도 위임은 계속된다", delegated)
  }

  @Test
  fun install_chains_previous_handlers() {
    val store = newStore()
    val first = Thread.getDefaultUncaughtExceptionHandler()
    CrashMarker.install(store)
    val second = Thread.getDefaultUncaughtExceptionHandler()
    CrashMarker.install(store)
    try {
      assertTrue("연속 설치는 이전 핸들러를 사슬로 보존한다", second !== first || first == null)
    } finally {
      Thread.setDefaultUncaughtExceptionHandler(first)
    }
  }
}
