package dev.leftcar.viewer.stream

import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.os.Build

/** Advertised upper bounds, never proof of simultaneous hardware availability. */
internal data class DecoderCapabilityHint(val codecName: String, val maxInstances: Int, val maxInstancePixelRate: Double?)

internal fun conservativeDecoderHint(name: String, instances: Int?, pixelRate: Double?): DecoderCapabilityHint =
    DecoderCapabilityHint(name, (instances ?: 1).coerceIn(1, 4), pixelRate?.takeIf { it.isFinite() && it > 0 })

internal object SplitDecoderCapability {
    private const val MIME_AVC = "video/avc"
    private fun codecs(): List<MediaCodecInfo> = runCatching {
        MediaCodecList(MediaCodecList.ALL_CODECS).codecInfos.filter {
            !it.isEncoder && isHardwareCodec(it) && it.supportedTypes.any { type -> type.equals(MIME_AVC, true) }
        }.sortedWith(compareBy<MediaCodecInfo> {
            when { it.name.contains("low_latency", true) -> 0; it.name.startsWith("c2.", true) -> 1; else -> 2 }
        }.thenBy { it.name })
    }.getOrDefault(emptyList())

    fun findQualifiedCodecName(): String? = codecs().firstOrNull { info -> runCatching {
        val caps = info.getCapabilitiesForType(MIME_AVC)
        caps.maxSupportedInstances >= 2 && caps.videoCapabilities?.areSizeAndRateSupported(1_920, 2_160, 60.0) == true
    }.getOrDefault(false) }?.name

    fun readHint(): DecoderCapabilityHint? {
        val all = codecs()
        val preferred = findQualifiedCodecName()
        val info = all.firstOrNull { it.name == preferred } ?: all.firstOrNull() ?: return null
        val caps = runCatching { info.getCapabilitiesForType(MIME_AVC) }.getOrNull()
        val instances = runCatching { caps?.maxSupportedInstances }.getOrNull()
        // This is a declared single-codec size/rate ceiling. Do not multiply by
        // advertised instances: that does not establish aggregate throughput.
        val pixelRate = runCatching {
            val video = caps?.videoCapabilities ?: return@runCatching null
            val height = if (video.isSizeSupported(1_920, 2_160)) 2_160 else 1_080
            video.getSupportedFrameRatesFor(1_920, height).upper * 1_920 * height
        }.getOrNull()
        return conservativeDecoderHint(info.name, instances, pixelRate)
    }

    private fun isHardwareCodec(info: MediaCodecInfo): Boolean {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) return info.isHardwareAccelerated && !info.isSoftwareOnly
        val name = info.name.lowercase()
        return !name.startsWith("omx.google.") && !name.startsWith("c2.android.") && !name.contains("software")
    }
}
