package dev.leftcar.viewer.stream

import android.app.Activity
import android.os.Handler
import android.os.Looper
import java.util.concurrent.Executor
import java.util.concurrent.Executors

/**
 * XR 홈 스페이스 메인 패널의 절대 픽셀 크기를 플랫폼 확장 API로 지정한다
 * (2026-09-20 창 크기 유지). androidx.xr 경유 없이 com.android.extensions.xr을
 * 곧장 호출한다. 확장 클래스는 XR 기기에서만 존재하므로(compileOnly 스텁)
 * [isSupported]를 통과한 뒤에만 확장형에 닿는다 — 비 XR 기기·확장 미탑재
 * 빌드에서는 false를 돌려주고 호출자가 소스 비율 힌트로 폴백한다.
 */
internal object XrMainWindowSizer {
    private val executor: Executor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "LeftcarXrSizer").apply { isDaemon = true }
    }
    private val mainHandler = Handler(Looper.getMainLooper())
    @Volatile private var cached: Any? = null

    /** XR 공간 API가 있는 기기인지. 확장 클래스 참조 없이 답한다. */
    fun isSupported(activity: Activity): Boolean =
        activity.packageManager.hasSystemFeature("android.software.xr.api.spatial")

    /**
     * 메인 패널(액티비티 창)을 절대 픽셀 크기로 맞춘다. 요청 접수 여부를
     * 돌려준다 — 실제 적용 결과는 [onResult]로 비동기 도착하며, 시스템이
     * 크기를 조정(클램프)하면 이어지는 onConfigurationChanged가 실측을
     * 보고한다. 사용자 핸들 리사이즈와 같은 경로라 상호 운용된다.
     */
    fun setMainWindowSize(
        activity: Activity,
        widthPx: Int,
        heightPx: Int,
        onResult: (Int) -> Unit = {},
    ): Boolean {
        if (!isSupported(activity)) return false
        val result = try {
            val extensions = obtain()
            extensions.setMainWindowSize(
                activity,
                widthPx,
                heightPx,
                executor,
            ) { value ->
                val code = value.result
                mainHandler.post {
                    android.util.Log.i(
                        "LeftcarStream",
                        "setMainWindowSize($widthPx,$heightPx) -> $code",
                    )
                    onResult(code)
                }
            }
            true
        } catch (t: Throwable) {
            // NoClassDefFoundError(확장 미탑재)·Stub! 등 전부 비율 폴백 트리거.
            android.util.Log.i("LeftcarStream", "setMainWindowSize unavailable: $t")
            false
        }
        return result
    }

    private fun obtain(): com.android.extensions.xr.XrExtensions =
        (cached as? com.android.extensions.xr.XrExtensions)
            ?: com.android.extensions.xr.XrExtensions().also { cached = it }
}
