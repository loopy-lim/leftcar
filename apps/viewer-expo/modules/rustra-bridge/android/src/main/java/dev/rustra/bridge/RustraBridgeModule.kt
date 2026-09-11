// ── rustra generated ────────────────────────────────────────
// File:   android/src/main/java/dev/rustra/bridge/RustraBridgeModule.kt
// Source: schema.json (single source of truth for this file)
// Regen:  rustra codegen --config rustra.json
// Stage:  rust-probe schema → ts renderer
// DO NOT EDIT — changes will be overwritten and fail codegen --check.
// ────────────────────────────────────────────────────────────

package dev.rustra.bridge

import android.content.pm.ApplicationInfo
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.turbomodule.core.interfaces.CallInvokerHolder

class RustraBridgeModule(context: ReactApplicationContext) : ReactContextBaseJavaModule(context) {
  companion object { init { System.loadLibrary("rustra_bridge") } }
  override fun getName(): String = "RustraBridge"
  override fun invalidate() { nativeInvalidate(); super.invalidate() }
  @ReactMethod
  fun install(promise: Promise) {
    val pointer = reactApplicationContext.javaScriptContextHolder?.get()
    if (pointer == null || pointer == 0L) { promise.reject("ERR_NO_RUNTIME", "JavaScript context pointer is null"); return }
    if (!nativeInstall(pointer, reactApplicationContext.jsCallInvokerHolder)) {
      promise.reject("ERR_INSTALL", "Failed to install Rustra onto the JSI runtime")
      return
    }
    // Android 핫 디렉터 관례: <filesDir>/rustra/hot — dev 흐름에서 CLI 가 발행하는
    // *-hot-live.so 를 코어가 감시한다. debuggable 빌드에서만 활성화한다 —
    // 릴리스는 감시 스레드 자체가 생기지 않는다(appdata dlopen 표면 차단,
    // iOS 글루의 RUSTRA_HOT_CORE_DIR env 게이트와 대칭). 파일이 없으면 정적
    // 코어로 머무르며, 설치 실패로 install 을 거절하지 않는다(dev 전용 표면).
    if (reactApplicationContext.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0) {
      nativeConfigureHotCore(reactApplicationContext.filesDir.resolve("rustra/hot").toString())
    }
    promise.resolve(true)
  }
  private external fun nativeInstall(pointer: Long, holder: CallInvokerHolder?): Boolean
  private external fun nativeConfigureHotCore(path: String?)
  private external fun nativeInvalidate()
}
