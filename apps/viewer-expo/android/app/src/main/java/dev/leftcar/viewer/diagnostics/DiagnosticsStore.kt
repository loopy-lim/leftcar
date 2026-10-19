package dev.leftcar.viewer.diagnostics

import java.util.Calendar
import java.util.Locale

/**
 * 진단 저장 규격(docs/07 §16). TS 측 상수(apps/viewer-expo/src/diagnostics.ts)와
 * 같은 값을 가리키며 양쪽 테스트가 고정한다.
 */
object DiagnosticsLimits {
  const val SAMPLE_INTERVAL_MS = 15_000L
  const val MAX_RECORD_BYTES = 1024
  const val MAX_FILE_BYTES = 5L * 1024 * 1024
  const val BACKUP_GENERATIONS = 1
  const val BOOT_DETECTION_UPTIME_MS = 120_000L
}

/** 내보내기 대상 열기. 실제 구현은 MediaStore.Downloads, 테스트는 메모리 sink다. */
interface ExportSink {
  fun open(name: String): java.io.OutputStream
}

/**
 * 진단 ndjson 저장소. 같은 프로세스의 append/회전을 직렬화하고, 레코드는
 * 줄바꿈 제외 1,024바이트, 활성 파일은 5 MiB 기준으로 한 세대 백업에
 * 회전한다(§16). 파일 시스템 오류로 규격을 지킬 수 없으면 새 기록을
 * 중단하지만 호출자(스트림)는 계속된다.
 */
class DiagnosticsStore(
  private val baseDir: java.io.File,
  private val nowMs: () -> Long,
  private val upMs: () -> Long,
  private val maxFileBytes: Long = DiagnosticsLimits.MAX_FILE_BYTES,
) {
  private val lock = Any()
  private var stopped = false
  private val kindCounts = LinkedHashMap<String, Int>()

  fun append(line: String): Boolean = synchronized(lock) { appendLocked(line) }

  /**
   * 크래시 레코드. §16 denylist에 따라 메시지 텍스트는 애초에 받지 않고
   * 예외 클래스명만 남긴다 — 잘린 문자열을 이스케이프해 ndjson 한 줄을
   * 보존한다.
   */
  fun appendCrash(exceptionClass: String): Boolean = synchronized(lock) {
    val escaped = exceptionClass.take(200)
      .replace("\\", "\\\\")
      .replace("\"", "\\\"")
    appendLocked("{\"ts\":${nowMs()},\"up_ms\":${upMs()},\"kind\":\"crash\",\"exception\":\"$escaped\"}")
  }

  /** 백업(.1) 먼저, 활성 나중 — 시간순 병합. 파일이 없어도 이름은 돌려준다. */
  fun exportTo(sink: ExportSink): String = synchronized(lock) {
    val name = exportName()
    sink.open(name).use { out ->
      copyInto(out, backupFile())
      copyInto(out, activeFile())
    }
    name
  }

  /** 이번 실행에서 성공한 기록 개수(종류별). 프로세스 수명 동안만 유효하다. */
  fun counts(): Map<String, Int> = synchronized(lock) { kindCounts.toMap() }

  fun stopWriting() {
    synchronized(lock) { stopped = true }
  }

  private fun activeFile() = java.io.File(baseDir, "leftcar-diagnostics.ndjson")

  private fun backupFile() = java.io.File(baseDir, "leftcar-diagnostics.ndjson.1")

  private fun appendLocked(line: String): Boolean {
    if (stopped) return false
    // 레코드 상한은 줄바꿈을 제외한 본문 기준(§16)이고, 개행 포함 입력은
    // ndjson 한 줄 가정을 깨므로 거절한다.
    if (line.toByteArray(Charsets.UTF_8).size > DiagnosticsLimits.MAX_RECORD_BYTES) return false
    if (line.contains('\n')) return false
    val bytes = (line + "\n").toByteArray(Charsets.UTF_8)
    try {
      val active = activeFile()
      if (active.length() + bytes.size > maxFileBytes) rotateLocked(active)
      java.io.FileOutputStream(active, true).use { it.write(bytes) }
      enforceOwnerOnly(active)
      countLocked(line)
      return true
    } catch (_: java.io.IOException) {
      stopped = true
      return false
    }
  }

  private fun rotateLocked(active: java.io.File) {
    val backup = backupFile()
    backup.delete()
    if (active.exists()) active.renameTo(backup)
  }

  private fun countLocked(line: String) {
    val match = KIND_REGEX.find(line) ?: return
    val kind = match.groupValues[1]
    kindCounts[kind] = (kindCounts[kind] ?: 0) + 1
  }

  private fun copyInto(out: java.io.OutputStream, file: java.io.File) {
    if (!file.exists()) return
    java.io.FileInputStream(file).use { input -> input.copyTo(out) }
  }

  private fun exportName(): String {
    val cal = Calendar.getInstance()
    cal.timeInMillis = nowMs()
    return String.format(
      Locale.US,
      "leftcar-diagnostics-%04d%02d%02d-%02d%02d%02d.ndjson",
      cal.get(Calendar.YEAR),
      cal.get(Calendar.MONTH) + 1,
      cal.get(Calendar.DAY_OF_MONTH),
      cal.get(Calendar.HOUR_OF_DAY),
      cal.get(Calendar.MINUTE),
      cal.get(Calendar.SECOND),
    )
  }

  companion object {
    private val KIND_REGEX = Regex("\"kind\":\"([a-z_]{1,16})\"")

    /** 최선 노력 0600. 내부 저장소 디렉터리(0700)에 더해 파일 자체도 소유자 전용으로 강제한다. */
    fun enforceOwnerOnly(file: java.io.File) {
      file.setReadable(false, false)
      file.setReadable(true, true)
      file.setWritable(false, false)
      file.setWritable(true, true)
      file.setExecutable(false, false)
    }
  }
}
