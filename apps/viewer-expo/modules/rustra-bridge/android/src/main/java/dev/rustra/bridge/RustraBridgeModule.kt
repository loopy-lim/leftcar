// ── rustra generated ────────────────────────────────────────
// File:   android/src/main/java/dev/rustra/bridge/RustraBridgeModule.kt
// Source: schema.json (single source of truth for this file)
// Regen:  rustra codegen --config rustra.json
// Stage:  rust-probe schema → ts renderer
// DO NOT EDIT — changes will be overwritten and fail codegen --check.
// ────────────────────────────────────────────────────────────

package dev.rustra.bridge

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
    if (nativeInstall(pointer, reactApplicationContext.jsCallInvokerHolder)) promise.resolve(true)
    else promise.reject("ERR_INSTALL", "Failed to install Rustra onto the JSI runtime")
  }
  private external fun nativeInstall(pointer: Long, holder: CallInvokerHolder?): Boolean
  private external fun nativeInvalidate()
}
