package dev.leftcar.viewer.km

import android.content.Intent
import android.content.pm.PackageManager
import android.provider.Settings
import com.facebook.react.ReactPackage
import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.NativeModule
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.bridge.ReadableMap
import com.facebook.react.bridge.WritableMap
import com.facebook.react.uimanager.ViewManager
import com.facebook.react.modules.core.DeviceEventManagerModule
import rikka.shizuku.Shizuku

class KmModule(private val reactContext: ReactApplicationContext) : ReactContextBaseJavaModule(reactContext) {
    private val statusListener: (KmNativeStatus) -> Unit = { status ->
        if (reactContext.hasActiveReactInstance()) reactContext.getJSModule(DeviceEventManagerModule.RCTDeviceEventEmitter::class.java)
            .emit("leftcarKmStatus", statusMap(status))
    }
    private val binderListener = Shizuku.OnBinderReceivedListener { KmRuntime.changed() }
    private val deadListener = Shizuku.OnBinderDeadListener { KmRuntime.stop("Shizuku가 종료되었습니다") }
    private val permissionListener = Shizuku.OnRequestPermissionResultListener { _, _ -> KmRuntime.changed() }

    init {
        KmRuntime.initialize(reactContext)
        KmRuntime.listeners.add(statusListener)
        Shizuku.addBinderReceivedListenerSticky(binderListener)
        Shizuku.addBinderDeadListener(deadListener)
        Shizuku.addRequestPermissionResultListener(permissionListener)
    }
    override fun getName() = "LeftcarKm"
    @ReactMethod fun getStatus(promise: Promise) { promise.resolve(statusMap(KmRuntime.status)) }
    @ReactMethod fun prepare(promise: Promise) { KmRuntime.prepare { it.fold({ ready -> promise.resolve(statusMap(ready)) }, { error -> promise.reject("km_prepare_failed", error.message) }) } }
    @ReactMethod fun requestShizukuPermission(promise: Promise) {
        try {
            check(Shizuku.pingBinder() && Shizuku.getUid() == 2000) { "USB로 Shizuku를 ADB 모드에서 시작하세요" }
            Shizuku.requestPermission(1441)
            promise.resolve(null)
        } catch (_: Exception) { promise.reject("km_shizuku_unavailable", "USB로 Shizuku를 ADB 모드에서 시작하세요") }
    }
    @ReactMethod fun openAccessibilitySettings(promise: Promise) {
        try {
            reactContext.startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            promise.resolve(null)
        } catch (_: Exception) { promise.reject("km_settings_unavailable", "시스템 접근성 설정을 열 수 없습니다") }
    }
    @ReactMethod fun start(config: ReadableMap, promise: Promise) {
        try {
            val epoch = config.getDouble("epoch")
            require(epoch.isFinite() && epoch > 0 && epoch <= 9007199254740991.0 && epoch == epoch.toLong().toDouble())
            val value = KmStartConfig(
                config.getString("host") ?: error("host"), config.getInt("port"),
                config.getString("sessionId") ?: error("session"), config.getString("sessionKey") ?: error("key"),
                epoch.toLong(), config.getString("arrangementSide") ?: error("side"),
                config.getBoolean("macToAndroid"), config.getBoolean("androidToMac"))
            KmRuntime.start(value) { it.fold({ status -> promise.resolve(statusMap(status)) }, { error -> promise.reject("km_start_failed", error.message) }) }
        } catch (_: Exception) { promise.reject("km_invalid_config", "인증된 입력 공유 설정이 올바르지 않습니다") }
    }
    @ReactMethod fun stop(promise: Promise) { KmRuntime.stop(completion = { result -> result.fold({ promise.resolve(null) }, { promise.reject("km_stop_failed", it.message) }) }) }
    @ReactMethod fun addListener(eventName: String) = Unit
    @ReactMethod fun removeListeners(count: Int) = Unit
    override fun invalidate() {
        KmRuntime.listeners.remove(statusListener)
        Shizuku.removeBinderReceivedListener(binderListener)
        Shizuku.removeBinderDeadListener(deadListener)
        Shizuku.removeRequestPermissionResultListener(permissionListener)
        KmRuntime.stop("앱 연결이 종료되어 입력 공유를 중지했습니다")
        super.invalidate()
    }

    private fun statusMap(status: KmNativeStatus): WritableMap = Arguments.createMap().apply {
        putString("state", status.state); putString("reason", status.reason)
        putBoolean("shizukuRunning", runCatching { Shizuku.pingBinder() && Shizuku.getUid() == 2000 }.getOrDefault(false))
        putBoolean("shizukuGranted", runCatching { Shizuku.checkSelfPermission() == PackageManager.PERMISSION_GRANTED }.getOrDefault(false))
        putBoolean("accessibilityEnabled", KmAccessibilityService.instance != null)
        val bounds = reactContext.getSystemService(android.view.WindowManager::class.java).maximumWindowMetrics.bounds
        putInt("displayWidth", bounds.width()); putInt("displayHeight", bounds.height())
        putMap("capabilities", Arguments.createMap().apply {
            putBoolean("cursorInjection", status.cursorInjection); putBoolean("mouseObservation", status.mouseObservation)
            putBoolean("mouseCapture", status.mouseCapture); putBoolean("keyboardCapture", status.keyboardCapture)
        })
    }
}

class KmPackage : ReactPackage {
    @Deprecated("Legacy React Native package registration")
    override fun createNativeModules(reactContext: ReactApplicationContext): List<NativeModule> = listOf(KmModule(reactContext))
    override fun createViewManagers(reactContext: ReactApplicationContext): List<ViewManager<*, *>> = emptyList()
}
