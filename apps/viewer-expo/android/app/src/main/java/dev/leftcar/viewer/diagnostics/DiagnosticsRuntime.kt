package dev.leftcar.viewer.diagnostics

import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.os.PowerManager
import android.os.SystemClock

import dev.leftcar.viewer.BuildConfig
import dev.leftcar.viewer.shim.ViewerNative

import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * 진단 실행 시점 연결: 저장소 생성·세션 시작 기록·프로세스 수명 샘플러.
 * 판단 로직은 순수 함수로 분리해 테스트하고, 이 객체의 연결 자체는 얇게
 * 유지한다(실기 수동 검증 대상).
 */
object DiagnosticsRuntime {
  @Volatile private var storeRef: DiagnosticsStore? = null
  private var executorRef: ExecutorService? = null

  fun store(context: Context): DiagnosticsStore =
    storeRef ?: synchronized(this) {
      storeRef ?: create(context.applicationContext).also { storeRef = it }
    }

  fun exportExecutor(): ExecutorService =
    executorRef ?: synchronized(this) {
      executorRef ?: Executors.newSingleThreadExecutor().also { executorRef = it }
    }

  /** 부팅 직후면 기기를 다시 켠 직후다 — 재부팅 증명의 반쪽. */
  internal fun isFreshBoot(upMs: Long): Boolean = upMs in 0 until DiagnosticsLimits.BOOT_DETECTION_UPTIME_MS

  /** session_start 레코드. 문자열 필드는 TS 포맷터와 같은 허용 문자셋을 쓴다. */
  internal fun sessionStartLine(appVersion: String, osApi: Int, ts: Long, upMs: Long): String {
    val safe = if (SAFE_IDENTIFIER.matches(appVersion)) appVersion else "unknown"
    return "{\"ts\":$ts,\"up_ms\":$upMs,\"kind\":\"session\"," +
      "\"event\":\"session_start\",\"app_version\":\"$safe\",\"os_api\":$osApi," +
      "\"boot_detected\":${isFreshBoot(upMs)}}"
  }

  private fun create(context: Context): DiagnosticsStore {
    val store = DiagnosticsStore(
      baseDir = context.filesDir,
      nowMs = { System.currentTimeMillis() },
      upMs = { SystemClock.elapsedRealtime() },
    )
    store.append(
      sessionStartLine(
        appVersion = BuildConfig.VERSION_NAME,
        osApi = Build.VERSION.SDK_INT,
        ts = System.currentTimeMillis(),
        upMs = SystemClock.elapsedRealtime(),
      ),
    )
    startSampler(context, store)
    return store
  }

  private fun startSampler(context: Context, store: DiagnosticsStore) {
    val thread = HandlerThread("leftcar-diagnostics").apply { start() }
    val handler = Handler(thread.looper)
    val power = context.getSystemService(Context.POWER_SERVICE) as PowerManager
    val sampler = DiagnosticsSampler(
      readSnapshot = {
        try {
          context.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))?.extras
        } catch (_: Exception) {
          null
        }
      },
      thermalStatus = {
        if (Build.VERSION.SDK_INT >= 29) {
          try {
            power.currentThermalStatus
          } catch (_: Exception) {
            null
          }
        } else {
          null
        }
      },
      thermalHeadroom = {
        if (Build.VERSION.SDK_INT >= 30) {
          try {
            power.getThermalHeadroom(PowerManager.THERMAL_STATUS_SEVERE).toDouble()
          } catch (_: Exception) {
            null
          }
        } else {
          null
        }
      },
      streamCadence = {
        // 전 창 집계 표면 릴리스 간격(pack, 전부 0=데이터 없음). 네이티브
        // 미적재 환경은 데이터 없음으로 취급한다 — 판단 없이 전달만 한다.
        runCatching { ViewerNative.streamCadence() }.getOrNull()
      },
      nowMs = { System.currentTimeMillis() },
      upMs = { SystemClock.elapsedRealtime() },
      append = { line -> store.append(line) },
      schedule = { task, delayMs -> handler.postDelayed(task, delayMs) },
    )
    sampler.start()
  }

  private val SAFE_IDENTIFIER = Regex("^[\\w.\\-+]{1,40}$")
}
