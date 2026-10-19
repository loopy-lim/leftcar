package dev.leftcar.viewer.diagnostics

import android.content.ContentValues
import android.content.Context
import android.provider.MediaStore

import com.facebook.react.bridge.JavaOnlyMap
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod

import java.util.concurrent.Executor

/**
 * 진단 네이티브 모듈. TS가 세션 레코드를 붙이고 요약·내보내기를 요청한다.
 * 저장·회전 정책은 [DiagnosticsStore], 주기 기록은 [DiagnosticsSampler]가
 * 담당하며 이 모듈은 연결과 Downloads 내보내기 접착만 한다.
 */
class DiagnosticsModule(
  reactContext: ReactApplicationContext,
  private val storeOf: (Context) -> DiagnosticsStore,
  private val sinkOf: (Context) -> ExportSink,
  private val exportExecutor: Executor,
) : ReactContextBaseJavaModule(reactContext) {
  constructor(reactContext: ReactApplicationContext) : this(
    reactContext,
    { DiagnosticsRuntime.store(it) },
    { MediaStoreExportSink(it) },
    DiagnosticsRuntime.exportExecutor(),
  )

  override fun getName() = "Diagnostics"

  @ReactMethod
  fun appendRecord(line: String, promise: Promise) {
    promise.resolve(storeOf(reactApplicationContext).append(line))
  }

  /** 종류별 기록 개수 — 내보내기 전 사용자 확인용 요약(§16 "공유 전 확인"). */
  @ReactMethod
  fun getSummary(promise: Promise) {
    // JavaOnlyMap은 JNI 없이 만들어진다 — Robolectric에서도 같은 경로가 검증된다.
    val map = JavaOnlyMap()
    storeOf(reactApplicationContext).counts().forEach { (kind, count) -> map.putInt(kind, count) }
    promise.resolve(map)
  }

  @ReactMethod
  fun exportToDownloads(promise: Promise) {
    exportExecutor.execute {
      val context = reactApplicationContext.applicationContext
      val sink = sinkOf(context)
      try {
        val name = storeOf(context).exportTo(sink)
        (sink as? MediaStoreExportSink)?.finish()
        promise.resolve(name)
      } catch (e: Exception) {
        (sink as? MediaStoreExportSink)?.abort()
        promise.reject("export_failed", e.message ?: "export failed", e)
      }
    }
  }
}

/** MediaStore.Downloads 대상. IS_PENDING 플립은 쓰기가 닫힌 뒤에야 한다. */
private class MediaStoreExportSink(private val context: Context) : ExportSink {
  private var pendingUri: android.net.Uri? = null

  override fun open(name: String): java.io.OutputStream {
    val resolver = context.contentResolver
    val values = ContentValues().apply {
      put(MediaStore.Downloads.DISPLAY_NAME, name)
      put(MediaStore.Downloads.MIME_TYPE, "application/x-ndjson")
      put(MediaStore.Downloads.IS_PENDING, 1)
    }
    val inserted = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
      ?: throw java.io.IOException("downloads insert refused")
    pendingUri = inserted
    return resolver.openOutputStream(inserted)
      ?: throw java.io.IOException("downloads stream unavailable")
  }

  fun finish() {
    val target = pendingUri ?: return
    val values = ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }
    context.contentResolver.update(target, values, null, null)
  }

  fun abort() {
    val target = pendingUri ?: return
    context.contentResolver.delete(target, null, null)
  }
}
