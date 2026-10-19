package dev.leftcar.viewer.diagnostics

/**
 * Java 계열 미처리 예외에 한 줄 크래시 마커를 남긴다(§16 — 클래스명만, 메시지
 * 텍스트 금지). 프로세스가 죽기 직전의 동기 기록이므로 마커 자체의 실패는
 * 삼키고 반드시 이전 핸들러로 위임한다. 네이티브(SIGSEGV) 크래시는 마커 없이
 * 샘플 시계열이 끊기는 형태로만 관측된다 — 문서화된 한계다.
 */
object CrashMarker {
  fun install(store: DiagnosticsStore) {
    val previous = Thread.getDefaultUncaughtExceptionHandler()
    Thread.setDefaultUncaughtExceptionHandler { thread, throwable ->
      try {
        store.appendCrash(throwable.javaClass.name)
      } catch (_: Throwable) {
        // 마커 실패가 원래 크래시 처리를 방해하지 않는다.
      }
      previous?.uncaughtException(thread, throwable)
    }
  }
}
