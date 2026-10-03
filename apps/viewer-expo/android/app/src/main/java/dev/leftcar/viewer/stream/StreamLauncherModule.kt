package dev.leftcar.viewer.stream

import android.content.Intent
import android.content.Context
import android.app.ActivityOptions
import android.graphics.Rect
import android.hardware.display.DisplayManager
import android.os.Build
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.Uri
import android.util.DisplayMetrics
import android.view.Display
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.Arguments
import com.facebook.react.modules.core.DeviceEventManagerModule
import dev.leftcar.viewer.shim.ViewerNative
import java.lang.ref.WeakReference
import java.util.concurrent.ConcurrentHashMap

/**
     * Decode the viewer-generated media key (32 bytes of unpadded base64url,
     * produced by the JS `bytesToBase64Url`). Returns null for malformed
     * input so the prepare call fails closed instead of starting unsealed.
     */
internal fun decodeMediaKey(value: String): ByteArray? = try {
    val decoded = android.util.Base64.decode(
        value,
        android.util.Base64.URL_SAFE or android.util.Base64.NO_PADDING or android.util.Base64.NO_WRAP,
    )
    if (decoded.size == 32) decoded else null
} catch (_: IllegalArgumentException) {
    null
}

internal fun isCurrentActiveTerminationContext(
    registeredContext: Any?,
    queuedContext: Any?,
    active: Boolean,
): Boolean = active && registeredContext === queuedContext

/** 뷰어 패널의 물리 메트릭(getDisplayMetrics 응답 몸통). */
internal data class PhysicalDisplayMetrics(
    val physicalWidth: Int,
    val physicalHeight: Int,
    val densityDpi: Int,
)

/** 패널 디스플레이의 real metrics에서 메트릭을 뽑는다. 0 값은 무효로 취급해
 * 호출부가 필드를 생략할 수 있게 한다. real metrics는 회전에 따라 폭·높이가
 * 바뀐다 — 호스트 매칭은 장변을 폭으로 정규화하므로 그대로 보고한다. */
internal fun physicalDisplayMetricsFrom(metrics: DisplayMetrics): PhysicalDisplayMetrics? {
    if (metrics.widthPixels <= 0 || metrics.heightPixels <= 0 || metrics.densityDpi <= 0) {
        return null
    }
    return PhysicalDisplayMetrics(
        physicalWidth = metrics.widthPixels,
        physicalHeight = metrics.heightPixels,
        densityDpi = metrics.densityDpi,
    )
}

/**
 * Opens one OS window (task) per unique stream: RN calls
 * prepareStream binds the media listener before Host reachability proof;
 * openStream then displays the already-authorized stream. Reopening the same
 * host/port reuses its existing document task instead of adding another entry
 * to Recents.
 */
class StreamLauncherModule(reactContext: ReactApplicationContext) :
    ReactContextBaseJavaModule(reactContext) {

    companion object {
        private val splitDecoderByPort = ConcurrentHashMap<Int, String>()
        private val launchedInstances = ConcurrentHashMap.newKeySet<String>()
        private val liveStreams = ConcurrentHashMap<String, StreamOwnership>()
        // One pending render-recovery handoff per live generation. Closed or
        // superseded leases are removed; there is no independent event queue.
        private data class PendingRecovery(val port: Int, val generation: Long)
        private val pendingRecoveries = ConcurrentHashMap<String, PendingRecovery>()
        private val releaseExecutor = java.util.concurrent.Executors.newSingleThreadExecutor()
        private val mainHandler = android.os.Handler(android.os.Looper.getMainLooper())
        private val closeLeases = ConcurrentHashMap<String, StreamCloseLease>()
        private val activityRegistry = StreamActivityRegistry<StreamActivity>(
            closeLeases, ::executeNativeRelease,
            { instance, generation -> generation.toLongOrNull()?.let { forgetStream(instance, it) } },
        ) { it.finish() }

        private fun executeNativeRelease(release: () -> Boolean, complete: (Boolean) -> Unit) {
            releaseExecutor.execute {
                val success = runCatching(release).getOrDefault(false)
                mainHandler.post { complete(success) }
            }
        }

        fun registerStreamActivity(instanceId: String, generation: Long, activity: StreamActivity): Boolean =
            activityRegistry.register(instanceId, generation.toString(), activity)

        fun releaseStreamActivity(instanceId: String, generation: Long, activity: StreamActivity, release: () -> Boolean) {
            activityRegistry.release(instanceId, generation.toString(), activity, release)
            if (activityRegistry.isReleased(instanceId)) {
                forgetStream(instanceId, generation)
            }
        }
        private val reactContextLock = Any()
        private var reactContextReference = WeakReference<ReactApplicationContext>(null)
        private var terminationListenerCount = 0

        private fun registerReactContext(context: ReactApplicationContext) {
            synchronized(reactContextLock) {
                reactContextReference = WeakReference(context)
                terminationListenerCount = 0
            }
        }

        internal fun activeRegisteredReactContext(): ReactApplicationContext? =
            synchronized(reactContextLock) {
                reactContextReference.get()?.takeIf { it.hasActiveReactInstance() }
            }

        private fun isCurrentActiveReactContext(context: ReactApplicationContext): Boolean =
            synchronized(reactContextLock) {
                isCurrentActiveTerminationContext(
                    reactContextReference.get(),
                    context,
                    context.hasActiveReactInstance(),
                )
            }

        private fun clearReactContext(context: ReactApplicationContext) {
            synchronized(reactContextLock) {
                if (reactContextReference.get() === context) {
                    reactContextReference.clear()
                    terminationListenerCount = 0
                }
            }
        }

        fun emitTermination(port: Int, reason: Int) {
            emitTerminationEvent(port, reason, liveStreams["src-$port"]?.generation)
        }

        fun emitTermination(port: Int, reason: Int, generation: Long) {
            if (liveStreams["src-$port"]?.generation != generation) return
            emitTerminationEvent(port, reason, generation)
        }

        private fun isLiveRecovery(instanceId: String, recovery: PendingRecovery): Boolean {
            val lease = closeLeases[instanceId]
            return liveStreams[instanceId]?.generation == recovery.generation &&
                lease?.generation == recovery.generation.toString() && !lease.requested && !lease.released
        }

        private fun recoveryListenerReady(context: ReactApplicationContext): Boolean =
            synchronized(reactContextLock) {
                terminationListenerCount > 0 && reactContextReference.get() === context && context.hasActiveReactInstance()
            }

        fun cancelPendingRecovery(instanceId: String, generation: Long) {
            pendingRecoveries[instanceId]?.takeIf { it.generation == generation }?.let {
                pendingRecoveries.remove(instanceId, it)
            }
        }

        private fun flushPendingRecoveries() {
            val context = activeRegisteredReactContext()
            pendingRecoveries.forEach { (instanceId, recovery) ->
                if (!isLiveRecovery(instanceId, recovery)) {
                    pendingRecoveries.remove(instanceId, recovery)
                } else if (context != null && recoveryListenerReady(context)) {
                    try {
                        context.runOnNativeModulesQueueThread {
                            if (pendingRecoveries[instanceId] !== recovery) return@runOnNativeModulesQueueThread
                            if (!isLiveRecovery(instanceId, recovery)) {
                                pendingRecoveries.remove(instanceId, recovery)
                                return@runOnNativeModulesQueueThread
                            }
                            if (!recoveryListenerReady(context)) return@runOnNativeModulesQueueThread
                            try {
                                val payload = Arguments.createMap().apply {
                                    putInt("port", recovery.port)
                                    putInt("reason", 5)
                                    putString("generation", recovery.generation.toString())
                                }
                                context.getJSModule(DeviceEventManagerModule.RCTDeviceEventEmitter::class.java)
                                    .emit("leftcarStreamTerminated", payload)
                                pendingRecoveries.remove(instanceId, recovery)
                            } catch (_: IllegalStateException) {
                                // Keep it for the replacement context/listener.
                            }
                        }
                    } catch (_: IllegalStateException) {
                        // The queue disappeared before accepting this request.
                    }
                }
            }
        }

        private fun emitTerminationEvent(port: Int, reason: Int, generation: Long?) {
            if (reason == 5) {
                val instanceId = "src-$port"
                val recovery = generation?.let { PendingRecovery(port, it) } ?: return
                if (!isLiveRecovery(instanceId, recovery)) return
                pendingRecoveries[instanceId] = recovery
                flushPendingRecoveries()
                return
            }
            val context = activeRegisteredReactContext() ?: run {
                android.util.Log.w("LeftcarStream", "stream event unavailable: React context port=$port reason=$reason")
                return
            }
            try {
                context.runOnNativeModulesQueueThread {
                    if (!isCurrentActiveReactContext(context)) return@runOnNativeModulesQueueThread
                    try {
                        val payload = Arguments.createMap().apply {
                            putInt("port", port)
                            putInt("reason", reason)
                            generation?.let { putString("generation", it.toString()) }
                        }
                        context
                            .getJSModule(DeviceEventManagerModule.RCTDeviceEventEmitter::class.java)
                            .emit("leftcarStreamTerminated", payload)
                    } catch (_: IllegalStateException) {
                        // React may invalidate after the queue check; do not emit.
                    }
                }
            } catch (_: IllegalStateException) {
                // The native-modules queue can disappear during React teardown.
            }
        }

        fun emitWindowClosed(instanceId: String, generation: Long, port: Int) {
            // A retired Activity must not close a replacement using this port.
            val current = liveStreams[instanceId]?.generation
            android.util.Log.i("LeftcarStream", "window close event port=$port generation=$generation current=$current")
            if (current != generation) return
            emitTermination(port, 0, generation)
        }

        /**
         * 활성 StreamActivity가 XR 핸들 리사이즈를 마쳤다고 알린다. RN은 이 값을
         * setWindowSize 제어 명령으로 호스트에 저장하고, 다음 스트림 창이 같은
         * 크기로 열린다(2026-09-20 창 크기 유지).
         */
        fun emitWindowSizeChanged(port: Int, widthPx: Int, heightPx: Int) {
            val context = activeRegisteredReactContext() ?: run {
                android.util.Log.w(
                    "LeftcarStream",
                    "stream event unavailable: React context port=$port (window size)",
                )
                return
            }
            try {
                context.runOnNativeModulesQueueThread {
                    if (!isCurrentActiveReactContext(context)) return@runOnNativeModulesQueueThread
                    try {
                        val payload = Arguments.createMap().apply {
                            putInt("port", port)
                            putInt("widthPx", widthPx)
                            putInt("heightPx", heightPx)
                        }
                        context
                            .getJSModule(DeviceEventManagerModule.RCTDeviceEventEmitter::class.java)
                            .emit("leftcarWindowSizeChanged", payload)
                    } catch (_: IllegalStateException) {
                        // React may invalidate after the queue check; do not emit.
                    }
                }
            } catch (_: IllegalStateException) {
                // The native-modules queue can disappear during React teardown.
            }
        }

        /**
         * 활성 StreamActivity에서 입력 잠금 배너를 탭했다고 알린다(2026-09-21).
         * RN은 그 포트의 세션으로 requestInputEnable 제어 명령을 보내고, 호스트
         * UI의 승인 알림으로 이어진다 — 승인 자체는 호스트 운용자만 한다.
         */
        fun emitInputEnableRequested(port: Int, instanceId: String, generation: String, requestId: String) {
            val context = activeRegisteredReactContext() ?: run {
                android.util.Log.w(
                    "LeftcarStream",
                    "stream event unavailable: React context port=$port (input request)",
                )
                return
            }
            try {
                context.runOnNativeModulesQueueThread {
                    if (!isCurrentActiveReactContext(context)) return@runOnNativeModulesQueueThread
                    if (closeLeases[instanceId]?.generation != generation) return@runOnNativeModulesQueueThread
                    try {
                        val payload = Arguments.createMap().apply {
                            putInt("port", port)
                            putString("instanceId", instanceId)
                            putString("generation", generation)
                            putString("requestId", requestId)
                        }
                        context
                            .getJSModule(DeviceEventManagerModule.RCTDeviceEventEmitter::class.java)
                            .emit("leftcarInputEnableRequested", payload)
                    } catch (_: IllegalStateException) {
                        // React may invalidate after the queue check; do not emit.
                    }
                }
            } catch (_: IllegalStateException) {
                // The native-modules queue can disappear during React teardown.
            }
        }

        @JvmStatic
        fun forgetStream(instanceId: String, generation: Long) {
            val current = liveStreams[instanceId]
            if (current?.generation == generation) {
                liveStreams.remove(instanceId, current)
                launchedInstances.remove(instanceId)
                cancelPendingRecovery(instanceId, generation)
            }
        }
    }

    init {
        registerReactContext(reactContext)
    }

    override fun getName() = "StreamLauncher"

    override fun invalidate() {
        clearReactContext(reactApplicationContext)
        super.invalidate()
    }

    @ReactMethod
    fun addListener(eventName: String) {
        if (eventName != "leftcarStreamTerminated") return
        synchronized(reactContextLock) {
            if (reactContextReference.get() !== reactApplicationContext) return
            terminationListenerCount += 1
        }
        flushPendingRecoveries()
    }

    @ReactMethod
    fun removeListeners(count: Int) {
        synchronized(reactContextLock) {
            if (reactContextReference.get() === reactApplicationContext) {
                terminationListenerCount = (terminationListenerCount - count.coerceAtLeast(0)).coerceAtLeast(0)
            }
        }
    }

    @ReactMethod
    fun getLocalIpv4Addresses(promise: Promise) {
        try {
            val result = Arguments.createArray()
            val seen = linkedSetOf<String>()
            val manager = reactApplicationContext.getSystemService(Context.CONNECTIVITY_SERVICE)
                as ConnectivityManager
            for (network in manager.allNetworks) {
                val capabilities = manager.getNetworkCapabilities(network) ?: continue
                val isPhysicalLan = capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) ||
                    capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)
                if (!isPhysicalLan) continue
                val properties = manager.getLinkProperties(network) ?: continue
                for (linkAddress in properties.linkAddresses) {
                    val address = linkAddress.address
                    if (address.address.size == 4 && address.isSiteLocalAddress) {
                        seen += address.hostAddress ?: continue
                    }
                }
            }
            seen.take(4).forEach(result::pushString)
            promise.resolve(result)
        } catch (t: Throwable) {
            promise.reject("ERR_LOCAL_ADDRESSES", t.message, t)
        }
    }

    @ReactMethod
    fun getDecoderCapabilityHint(promise: Promise) {
        val hint = runCatching { SplitDecoderCapability.readHint() }.getOrNull()
        val result = com.facebook.react.bridge.Arguments.createMap()
        result.putInt("maxInstances", hint?.maxInstances ?: 1)
        hint?.let {
            result.putString("codecName", it.codecName)
            it.maxInstancePixelRate?.let { rate -> result.putDouble("maxInstancePixelRate", rate) }
        }
        promise.resolve(result)
    }

    /** 뷰어 자체 패널의 물리 메트릭. 확장 디스플레이 기본 모드 도출에
     * 쓰인다(docs/2026-09-18-extended-display-design.md §4.5). 창이 아닌
     * 실제 패널의 real metrics라 freeform·XR 창 크기와 무관하다. */
    @ReactMethod
    fun getDisplayMetrics(promise: Promise) {
        runCatching {
            val displayManager =
                reactApplicationContext.getSystemService(Context.DISPLAY_SERVICE) as? DisplayManager
            val display = displayManager?.getDisplay(Display.DEFAULT_DISPLAY)
            val metrics = DisplayMetrics()
            if (display != null) {
                display.getRealMetrics(metrics)
            } else {
                metrics.setTo(reactApplicationContext.resources.displayMetrics)
            }
            val payload = physicalDisplayMetricsFrom(metrics)
            val result = com.facebook.react.bridge.Arguments.createMap()
            // 무효 메트릭은 0으로 resolve한다 — JS가 검증해 필드를 생략한다.
            result.putInt("physicalWidth", payload?.physicalWidth ?: 0)
            result.putInt("physicalHeight", payload?.physicalHeight ?: 0)
            result.putInt("densityDpi", payload?.densityDpi ?: 0)
            promise.resolve(result)
        }.onFailure { promise.reject("ERR_DISPLAY_METRICS", it) }
    }

    @ReactMethod
    fun prepareStream(
        port: Int,
        host: String,
        mediaTransport: String,
        encoderExperiment: String,
        language: String?,
        mediaKey: String?,
        promise: Promise,
    ) {
        ViewerStrings.applyLanguage(language)
        val keyBytes = mediaKey?.let { decodeMediaKey(it) }
        if (keyBytes == null) {
            promise.reject("ERR_STREAM_PREPARE", "missing or malformed media key")
            return
        }
        val splitVertical = encoderExperiment == "splitVertical"
        val decoderName = if (splitVertical) {
            SplitDecoderCapability.findQualifiedCodecName()
        } else {
            null
        }
        if (splitVertical && decoderName == null) {
            promise.reject(
                "ERR_SPLIT_DECODER_CAPABILITY",
                ViewerStrings.splitDecoderUnavailable,
            )
            return
        }
        val result = if (splitVertical) {
            ViewerNative.prepareSplitStream(port, host, mediaTransport, keyBytes)
        } else {
            ViewerNative.prepareStream(port, host, mediaTransport, keyBytes)
        }
        if (result == 0) {
            if (decoderName != null) splitDecoderByPort[port] = decoderName
            promise.resolve(null)
        } else {
            promise.reject(
                "ERR_STREAM_PREPARE",
                "${ViewerStrings.prepareFailed} (code=$result)",
            )
        }
    }

    @ReactMethod
    fun cancelPreparedStream(port: Int, encoderExperiment: String, promise: Promise) {
        splitDecoderByPort.remove(port)
        val result = if (encoderExperiment == "splitVertical") {
            ViewerNative.cancelPreparedSplitStream(port)
        } else {
            ViewerNative.cancelPreparedStream(port)
        }
        if (result == 0) {
            promise.resolve(null)
        } else {
            promise.reject(
                "ERR_STREAM_PREPARE_CANCEL",
                "${ViewerStrings.prepareCancelFailed} (code=$result)",
            )
        }
    }

    @ReactMethod
    fun getStreamGeneration(instanceId: String, promise: Promise) {
        promise.resolve(closeLeases[instanceId]?.generation ?: "")
    }

    @ReactMethod
    fun reportInputRequestResult(instanceId: String, generation: String, requestId: String, error: String?, promise: Promise) {
        reactApplicationContext.runOnUiQueueThread {
            if (closeLeases[instanceId]?.generation == generation) {
                activityRegistry.current(instanceId)?.reportInputRequestResult(generation, requestId, error)
            }
            promise.resolve(null)
        }
    }

    @ReactMethod
    fun closeStream(instanceId: String, generation: String, promise: Promise) {
        reactApplicationContext.runOnUiQueueThread {
            activityRegistry.close(instanceId, generation) { released ->
                if (released) {
                    generation.toLongOrNull()?.let { forgetStream(instanceId, it) }
                    promise.resolve(null)
                }
                else promise.reject("ERR_STREAM_CLOSE", "Decoder cleanup is incomplete or the stream changed; retry stopping the stream")
            }
        }
    }

    @ReactMethod
    fun openStream(
        port: Int,
        host: String,
        width: Int,
        height: Int,
        fps: Int,
        encoderExperiment: String,
        displayName: String?,
        showFps: Boolean?,
        localCursor: Boolean?,
        language: String?,
        localAudio: Boolean?,
        windowWidthPx: Int,
        windowHeightPx: Int,
        promise: Promise,
    ) {
        openStreamInternal(port, host, width, height, fps, encoderExperiment, displayName, showFps, localCursor, language, localAudio, false, windowWidthPx, windowHeightPx, promise)
    }

    @ReactMethod
    fun openStreamWithPresentation(
        port: Int,
        host: String,
        width: Int,
        height: Int,
        fps: Int,
        encoderExperiment: String,
        displayName: String?,
        showFps: Boolean?,
        localCursor: Boolean?,
        language: String?,
        localAudio: Boolean?,
        balancedPresentation: Boolean?,
        windowWidthPx: Int,
        windowHeightPx: Int,
        promise: Promise,
    ) {
        openStreamInternal(port, host, width, height, fps, encoderExperiment, displayName, showFps, localCursor, language, localAudio, balancedPresentation, windowWidthPx, windowHeightPx, promise)
    }

    private fun openStreamInternal(
        port: Int,
        host: String,
        width: Int,
        height: Int,
        fps: Int,
        encoderExperiment: String,
        displayName: String?,
        showFps: Boolean?,
        localCursor: Boolean?,
        language: String?,
        localAudio: Boolean?,
        balancedPresentation: Boolean?,
        windowWidthPx: Int,
        windowHeightPx: Int,
        promise: Promise,
    ) {
        reactApplicationContext.runOnUiQueueThread {
            ViewerStrings.applyLanguage(language)
            try {
                val splitVertical = encoderExperiment == "splitVertical"
                val decoderName = if (splitVertical) {
                    splitDecoderByPort[port] ?: SplitDecoderCapability.findQualifiedCodecName()
                } else {
                    null
                }
                if (splitVertical && decoderName == null) {
                    promise.reject(
                        "ERR_SPLIT_DECODER_CAPABILITY",
                        ViewerStrings.splitDecoderMissing,
                    )
                    return@runOnUiQueueThread
                }
                val instanceId = "src-$port"
                val previousClose = closeLeases[instanceId]
                val previousOwnership = liveStreams[instanceId]
                if (previousClose?.requested == true && !previousClose.released) {
                    promise.reject("ERR_STREAM_CLOSING", "Previous decoder cleanup is incomplete; retry stopping the stream")
                    return@runOnUiQueueThread
                }
                val titleName = displayName?.takeIf { it.isNotBlank() } ?: ViewerStrings.displayFallback
                val intent = Intent(reactApplicationContext, StreamActivity::class.java).apply {
                    data = Uri.Builder()
                        .scheme("leftcar-stream")
                        .authority("session")
                        .appendPath(host)
                        .appendPath(port.toString())
                        .build()
                    putExtra("instance", instanceId)
                    putExtra("port", port)
                    putExtra("host", host)
                    putExtra("width", width)
                    putExtra("height", height)
                    putExtra("fps", fps.coerceIn(1, 90))
                    putExtra("splitVertical", splitVertical)
                    putExtra("splitDecoderName", decoderName)
                    putExtra("displayName", titleName)
                    putExtra("showFps", showFps ?: false)
                    putExtra("localCursor", localCursor ?: true)
                    putExtra("localAudio", localAudio ?: true)
                    putExtra("balancedPresentation", balancedPresentation ?: false)
                    putExtra("language", language ?: "ko")
                    // 호스트가 기억한 마지막 창 크기(px). 0이면 저장된 값이
                    // 없으므로 Activity가 소스 비율 힌트로 폴백한다.
                    putExtra("windowWidthPx", windowWidthPx)
                    putExtra("windowHeightPx", windowHeightPx)
                    // A recovery reuses the existing document task and port. The
                    // Activity keeps its Surface and swaps only the native
                    // renderer when this intent is delivered via onNewIntent.
                    putExtra("reconnect", true)
                    // `intoExisting` in the manifest plus SINGLE_TOP routes a
                    // retry to the existing Activity instance and onNewIntent;
                    // it must not create a second window/task.
                    addFlags(Intent.FLAG_ACTIVITY_NEW_DOCUMENT or Intent.FLAG_ACTIVITY_SINGLE_TOP)
                }
                val ctx = getReactApplicationContext().getCurrentActivity()
                val firstLaunch = launchedInstances.add(instanceId)
                val ownershipGeneration = System.nanoTime()
                liveStreams[instanceId] = StreamOwnership(host, port, ownershipGeneration)
                val newClose = StreamCloseLease(ownershipGeneration.toString(), ::executeNativeRelease)
                closeLeases[instanceId] = newClose
                intent.putExtra("ownershipGeneration", ownershipGeneration)
                try {
                    val launchOptions = if (firstLaunch) initialLaunchOptions(width, height, ctx) else null
                    if (ctx != null) {
                        ctx.startActivity(intent, launchOptions)
                    } else {
                        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                        reactApplicationContext.startActivity(intent, launchOptions)
                    }
                } catch (failure: Throwable) {
                    if (restoreFailedStreamLaunch(closeLeases, instanceId, newClose, previousClose)) {
                        if (previousOwnership == null) liveStreams.remove(instanceId)
                        else liveStreams[instanceId] = previousOwnership
                        if (firstLaunch) launchedInstances.remove(instanceId)
                    }
                    throw failure
                }
                promise.resolve(instanceId)
            } catch (t: Throwable) {
                promise.reject("ERR_STREAM_LAUNCH", t.message, t)
            }
        }
    }

    @ReactMethod
    fun setCursorStream(instanceId: String, enabled: Boolean, promise: Promise) {
        toggleStreamExtra(instanceId, enabled, "localCursor", "ERR_CURSOR_TOGGLE", promise)
    }

    @ReactMethod
    fun setBalancedPresentation(instanceId: String, enabled: Boolean, promise: Promise) {
        toggleStreamExtra(instanceId, enabled, "balancedPresentation", "ERR_PRESENTATION_TOGGLE", promise)
    }

    @ReactMethod
    fun setPresentationSmooth(instanceId: String, enabled: Boolean, promise: Promise) {
        toggleStreamExtra(instanceId, enabled, "presentationSmooth", "ERR_SMOOTH_TOGGLE", promise)
    }

    @ReactMethod
    fun setAudioStream(instanceId: String, enabled: Boolean, promise: Promise) {
        toggleStreamExtra(instanceId, enabled, "localAudio", "ERR_AUDIO_TOGGLE", promise)
    }

    @ReactMethod
    fun getAudioStats(instanceId: String, promise: Promise) {
        reactApplicationContext.runOnUiQueueThread {
            val activity = activityRegistry.current(instanceId)
            promise.resolve(activity?.let { com.facebook.react.bridge.Arguments.makeNativeMap(it.audioStats()) })
        }
    }

    @ReactMethod
    fun setOpusAudio(instanceId: String, enabled: Boolean, promise: Promise) {
        toggleStreamExtra(instanceId, enabled, "opusAudio", "ERR_AUDIO_CODEC", promise)
    }

    private fun toggleStreamExtra(
        instanceId: String,
        enabled: Boolean,
        extraKey: String,
        errorTag: String,
        promise: Promise,
    ) {
        val target = liveStreams[instanceId]
        if (target == null) {
            promise.reject("ERR_STREAM_NOT_ACTIVE", ViewerStrings.streamNotActive)
            return
        }
        try {
            launchStreamIntent(instanceId, target) { intent ->
                intent.putExtra(extraKey, enabled)
                intent.putExtra("reconnect", false)
            }
            promise.resolve(null)
        } catch (t: Throwable) {
            promise.reject(errorTag, t.message, t)
        }
    }

    private fun launchStreamIntent(
        instanceId: String,
        target: StreamOwnership,
        configure: (Intent) -> Unit,
    ) {
        val intent = Intent(reactApplicationContext, StreamActivity::class.java).apply {
            data = Uri.Builder().scheme("leftcar-stream").authority("session")
                .appendPath(target.host).appendPath(target.port.toString()).build()
            putExtra("instance", instanceId)
            putExtra("host", target.host)
            putExtra("port", target.port)
            putExtra("ownershipGeneration", target.generation)
            putExtra("reconnect", true)
            configure(this)
            addFlags(Intent.FLAG_ACTIVITY_NEW_DOCUMENT or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        }
        val context = reactApplicationContext.getCurrentActivity()
        if (context != null) context.startActivity(intent) else {
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            reactApplicationContext.startActivity(intent)
        }
    }

    private fun initialLaunchOptions(
        sourceWidth: Int,
        sourceHeight: Int,
        context: Context?,
    ): android.os.Bundle? {
        if (context == null || Build.VERSION.SDK_INT < Build.VERSION_CODES.N) return null
        val packageManager = context.packageManager
        val freeform = packageManager.hasSystemFeature("android.software.freeform_window_management")
        val pip = packageManager.hasSystemFeature("android.software.picture_in_picture")
        if (!freeform && !pip) return null
        val display = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val metrics = context.getSystemService(android.view.WindowManager::class.java)
                ?.currentWindowMetrics ?: return null
            val insets = metrics.windowInsets.getInsetsIgnoringVisibility(
                android.view.WindowInsets.Type.systemBars(),
            )
            val bounds = metrics.bounds
            DisplayGeometry(
                bounds.width(), bounds.height(), bounds.left, bounds.top,
                insets.left, insets.top, insets.right, insets.bottom,
            )
        } else {
            val metrics = context.resources.displayMetrics
            DisplayGeometry(metrics.widthPixels, metrics.heightPixels, 0, 0, 0, 0, 0, 0)
        }
        val bounds = initialStreamWindowBounds(
            sourceWidth,
            sourceHeight,
            display.width,
            display.height,
            display.leftInset,
            display.topInset,
            display.rightInset,
            display.bottomInset,
        ) ?: return null
        val rect = Rect(
            display.left + bounds.left,
            display.top + bounds.top,
            display.left + bounds.right,
            display.top + bounds.bottom,
        )
        return ActivityOptions.makeBasic().setLaunchBounds(rect).toBundle()
    }

    private data class DisplayGeometry(
        val width: Int,
        val height: Int,
        val left: Int,
        val top: Int,
        val leftInset: Int,
        val topInset: Int,
        val rightInset: Int,
        val bottomInset: Int,
    )

    private data class StreamOwnership(val host: String, val port: Int, val generation: Long)
}

/** Main-thread incarnation gate. A timeout retains the exact release closure. */
internal class StreamCloseLease(
    val generation: String,
    private val executeRelease: (() -> Boolean, (Boolean) -> Unit) -> Unit = { release, complete -> complete(release()) },
) {
    fun newPhysicalLease(): StreamCloseLease = StreamCloseLease(generation, executeRelease)

    var onReleased: (() -> Unit)? = null
    var released = false
        private set
    var requested = false
        private set
    private var releaseInFlight = false
    private var retryRelease: (() -> Boolean)? = null
    private val callbacks = mutableListOf<(Boolean) -> Unit>()

    /** True means the current Activity must finish; stale callers never do. */
    fun request(expected: String, complete: (Boolean) -> Unit): Boolean {
        if (expected != generation) { complete(false); return false }
        if (released) { complete(true); return false }
        requested = true
        callbacks += complete
        if (releaseInFlight) return false
        val retry = retryRelease
        if (retry != null) { completeRelease(retry); return false }
        return true
    }

    val canTransferOwnership: Boolean get() = !released && !releaseInFlight && retryRelease == null

    /** Same Activity keeps its native state; only its logical close owner changes. */
    fun transferOwnership(): Boolean {
        if (releaseInFlight || retryRelease != null) return false
        if (!released) {
            released = true
            val pending = callbacks.toList()
            callbacks.clear()
            pending.forEach { it(true) }
            onReleased?.invoke()
        }
        return true
    }

    fun completeRelease(release: () -> Boolean) {
        if (released || releaseInFlight) return
        requested = true
        releaseInFlight = true
        executeRelease(release) { success ->
            releaseInFlight = false
            released = success
            retryRelease = if (released) null else release
            val pending = callbacks.toList()
            callbacks.clear()
            pending.forEach { it(released) }
            if (released) onReleased?.invoke()
        }
    }
}

/** Roll back only the failed launch incarnation; a newer launch is untouched. */
internal fun restoreFailedStreamLaunch(
    leases: MutableMap<String, StreamCloseLease>,
    instanceId: String,
    failed: StreamCloseLease,
    previous: StreamCloseLease?,
): Boolean {
    if (leases[instanceId] !== failed) return false
    if (previous == null) leases.remove(instanceId) else leases[instanceId] = previous
    return true
}

/** Physical Activity leases remain distinct even when Android restores a logical generation. */
internal class StreamActivityRegistry<A : Any>(
    private val leases: MutableMap<String, StreamCloseLease>,
    private val executeRelease: (() -> Boolean, (Boolean) -> Unit) -> Unit = { release, complete -> complete(release()) },
    private val onReleased: (String, String) -> Unit = { _, _ -> },
    private val finish: (A) -> Unit,
) {
    private data class Registration<A : Any>(
        val generation: String, val activity: WeakReference<A>, val lease: StreamCloseLease,
    )
    private class CloseRequest(val complete: (Boolean) -> Unit) {
        val pending = mutableSetOf<StreamCloseLease>()
        var allReleased = true
    }
    private val owners = mutableMapOf<String, MutableList<Registration<A>>>()
    // Weak tombstones preserve duplicate/stale release protection after pruning history.
    private val activityOwners = java.util.WeakHashMap<A, Registration<A>>()
    private val closingGeneration = mutableMapOf<String, String>()
    private val closeRequests = mutableMapOf<String, MutableList<CloseRequest>>()

    private fun prune(instanceId: String) {
        val current = leases[instanceId]
        owners[instanceId]?.removeAll { registration ->
            (registration.lease !== current && registration.lease.released).also { removed ->
                if (removed) registration.lease.onReleased = null
            }
        }
    }

    fun current(instanceId: String): A? {
        val lease = leases[instanceId] ?: return null
        if (lease.released) return null
        return owners[instanceId]?.lastOrNull { it.lease === lease }?.activity?.get()
    }

    internal fun retainedGenerationCount(instanceId: String): Int = owners[instanceId]?.size ?: 0

    fun isReleased(instanceId: String): Boolean = leases[instanceId]?.released == true &&
        owners[instanceId].orEmpty().all { it.lease.released }

    private fun watch(instanceId: String, request: CloseRequest, lease: StreamCloseLease): Boolean =
        lease.request(lease.generation) { released ->
            request.allReleased = request.allReleased && released
            request.pending.remove(lease)
            if (request.pending.isEmpty()) {
                closeRequests[instanceId]?.remove(request)
                request.complete(request.allReleased)
            }
        }

    fun register(instanceId: String, generation: String, activity: A): Boolean {
        val current = leases[instanceId]
        if (current != null && current.generation != generation) {
            if (activityOwners[activity] == null) finish(activity)
            return false
        }
        val previous = activityOwners[activity]
        if (previous?.generation == generation) return previous.lease.canTransferOwnership
        if (previous != null && !previous.lease.canTransferOwnership) {
            // The successor never acquired this physical state. Settle only its
            // empty launch obligation, retaining the old state and cleanup closure.
            current?.completeRelease { true }
            finish(activity)
            return false
        }
        val registrations = owners.getOrPut(instanceId) { mutableListOf() }
        val lease = when {
            current == null -> StreamCloseLease(generation, executeRelease)
            current.released || registrations.any { it.lease === current } -> current.newPhysicalLease()
            else -> current
        }
        leases[instanceId] = lease
        val registration = Registration(generation, WeakReference(activity), lease)
        registrations += registration
        activityOwners[activity] = registration
        lease.onReleased = {
            prune(instanceId)
            if (isReleased(instanceId)) leases[instanceId]?.let { current -> onReleased(instanceId, current.generation) }
        }
        // Extend a close already waiting for the old physical state before its
        // completion can acknowledge the newly recreated native state.
        closeRequests[instanceId]?.toList()?.forEach { request ->
            if (request.pending.add(lease)) watch(instanceId, request, lease)
        }
        // onNewIntent keeps the same physical native state. Its previous logical
        // owner can settle only when cleanup has not started on that state.
        previous?.lease?.transferOwnership()
        prune(instanceId)
        if (closingGeneration[instanceId] == generation) {
            finish(activity)
            // A reused Activity already owns native state: its caller must adopt
            // this generation so onDestroy releases the transferred obligation.
            return previous != null
        }
        return true
    }

    fun close(instanceId: String, generation: String, complete: (Boolean) -> Unit) {
        val lease = leases[instanceId]
        if (lease == null || lease.generation != generation) { complete(false); return }
        closingGeneration[instanceId] = generation
        val request = CloseRequest(complete)
        request.pending += owners[instanceId]?.map { it.lease }.orEmpty()
        request.pending += lease
        closeRequests.getOrPut(instanceId) { mutableListOf() } += request
        // Seed the complete set before synchronous callbacks can settle it.
        request.pending.toList().forEach { obligation ->
            val shouldFinish = watch(instanceId, request, obligation)
            // A pending successor intent must not destroy its incumbent.
            if (obligation === lease && shouldFinish) current(instanceId)?.let(finish)
        }
    }

    fun release(instanceId: String, generation: String, activity: A, release: () -> Boolean): Boolean {
        val registration = activityOwners[activity]
        if (registration != null && registration.generation != generation) return false
        val lease = registration?.lease ?: StreamCloseLease(generation, executeRelease).also {
            // Process restoration or a stale launch must still release captured JNI
            // state. Never attach this obligation to another Activity's lease.
            val orphan = Registration(generation, WeakReference(activity), it)
            activityOwners[activity] = orphan
            owners.getOrPut(instanceId) { mutableListOf() } += orphan
            it.onReleased = {
                prune(instanceId)
                if (isReleased(instanceId)) leases[instanceId]?.let { current -> onReleased(instanceId, current.generation) }
            }
            closeRequests[instanceId]?.toList()?.forEach { request ->
                if (request.pending.add(it)) watch(instanceId, request, it)
            }
        }
        lease.completeRelease(release)
        return lease.released
    }
}
