package dev.leftcar.viewer.stream

import android.content.Intent
import android.content.Context
import android.app.ActivityOptions
import android.graphics.Rect
import android.os.Build
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.Uri
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.Arguments
import com.facebook.react.modules.core.DeviceEventManagerModule
import dev.leftcar.viewer.shim.ViewerNative
import java.lang.ref.WeakReference
import java.util.concurrent.ConcurrentHashMap

internal fun isCurrentActiveTerminationContext(
    registeredContext: Any?,
    queuedContext: Any?,
    active: Boolean,
): Boolean = active && registeredContext === queuedContext

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
        private val reactContextLock = Any()
        private var reactContextReference = WeakReference<ReactApplicationContext>(null)

        private fun registerReactContext(context: ReactApplicationContext) {
            synchronized(reactContextLock) {
                reactContextReference = WeakReference(context)
            }
        }

        private fun activeRegisteredReactContext(): ReactApplicationContext? =
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
                }
            }
        }

        fun emitTermination(port: Int, reason: Int) {
            val context = activeRegisteredReactContext() ?: return
            try {
                context.runOnNativeModulesQueueThread {
                    if (!isCurrentActiveReactContext(context)) return@runOnNativeModulesQueueThread
                    try {
                        val payload = Arguments.createMap().apply {
                            putInt("port", port)
                            putInt("reason", reason)
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

        @JvmStatic
        fun forgetStream(instanceId: String, generation: Long) {
            val current = liveStreams[instanceId]
            if (current?.generation == generation) {
                liveStreams.remove(instanceId, current)
                launchedInstances.remove(instanceId)
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
    fun addListener(eventName: String) {}

    @ReactMethod
    fun removeListeners(count: Int) {}

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
    fun prepareStream(
        port: Int,
        host: String,
        mediaTransport: String,
        encoderExperiment: String,
        promise: Promise,
    ) {
        val splitVertical = encoderExperiment == "splitVertical"
        val decoderName = if (splitVertical) {
            SplitDecoderCapability.findQualifiedCodecName()
        } else {
            null
        }
        if (splitVertical && decoderName == null) {
            promise.reject(
                "ERR_SPLIT_DECODER_CAPABILITY",
                "4K 분할 스트림에 필요한 동시 2개 하드웨어 H.264 디코더를 사용할 수 없습니다.",
            )
            return
        }
        val result = if (splitVertical) {
            ViewerNative.prepareSplitStream(port, host, mediaTransport)
        } else {
            ViewerNative.prepareStream(port, host, mediaTransport)
        }
        if (result == 0) {
            if (decoderName != null) splitDecoderByPort[port] = decoderName
            promise.resolve(null)
        } else {
            promise.reject(
                "ERR_STREAM_PREPARE",
                "미디어 수신 포트를 준비하지 못했습니다. (code=$result)",
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
                "미디어 수신 포트 정리에 실패했습니다. (code=$result)",
            )
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
        promise: Promise,
    ) {
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
                    "검증된 동시 하드웨어 H.264 디코더가 없어 4K 분할 스트림을 열 수 없습니다.",
                )
                return
            }
            val instanceId = "src-$port"
            val titleName = displayName?.takeIf { it.isNotBlank() } ?: "디스플레이"
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
                putExtra("localCursor", localCursor ?: false)
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
            intent.putExtra("ownershipGeneration", ownershipGeneration)
            val launchOptions = if (firstLaunch) initialLaunchOptions(width, height, ctx) else null
            if (ctx != null) {
                ctx.startActivity(intent, launchOptions)
            } else {
                intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                reactApplicationContext.startActivity(intent, launchOptions)
            }
            promise.resolve(instanceId)
        } catch (t: Throwable) {
            launchedInstances.remove("src-$port")
            liveStreams.remove("src-$port")
            promise.reject("ERR_STREAM_LAUNCH", t.message, t)
        }
    }

    @ReactMethod
    fun setCursorStream(instanceId: String, enabled: Boolean, promise: Promise) {
        val target = liveStreams[instanceId]
        if (target == null) {
            promise.reject("ERR_STREAM_NOT_ACTIVE", "화면 공유 창을 찾을 수 없습니다.")
            return
        }
        launchStreamIntent(instanceId, target) { intent ->
            intent.putExtra("localCursor", enabled)
        }
        try {
            promise.resolve(null)
        } catch (t: Throwable) {
            promise.reject("ERR_CURSOR_TOGGLE", t.message, t)
        }
    }

    /**
     * 활성 StreamActivity 창에 XR 비율 프리셋을 전달한다. 컴퓨터 화면
     * 해상도는 변경하지 않는다 — 이 값은 SpatialWindow 비율에만 쓰인다.
     * XR이 아닌 기기에서는 Activity의 XR 검사가 no-op으로 처리한다.
     */
    @ReactMethod
    fun setWindowAspectRatio(instanceId: String, ratio: Double, promise: Promise) {
        val target = liveStreams[instanceId]
        if (target == null) {
            promise.reject("ERR_STREAM_NOT_ACTIVE", "화면 공유 창을 찾을 수 없습니다.")
            return
        }
        if (!ratio.isFinite()) {
            promise.reject("ERR_WINDOW_ASPECT_RATIO", "비율 값이 올바르지 않습니다.")
            return
        }
        try {
            launchStreamIntent(instanceId, target) { intent ->
                intent.putExtra("xrWindowRatio", ratio.toFloat())
            }
            promise.resolve(null)
        } catch (t: Throwable) {
            promise.reject("ERR_WINDOW_ASPECT_RATIO", t.message, t)
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
