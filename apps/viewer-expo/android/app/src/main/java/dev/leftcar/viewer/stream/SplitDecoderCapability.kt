package dev.leftcar.viewer.stream

import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.os.Build

internal object SplitDecoderCapability {
    private const val MIME_AVC = "video/avc"

    fun findQualifiedCodecName(): String? = MediaCodecList(MediaCodecList.ALL_CODECS)
        .codecInfos
        .asSequence()
        .filterNot(MediaCodecInfo::isEncoder)
        .filter(::isHardwareCodec)
        .mapNotNull { info ->
            val supportsAvc = info.supportedTypes.any { it.equals(MIME_AVC, ignoreCase = true) }
            if (!supportsAvc) return@mapNotNull null
            val capabilities = runCatching { info.getCapabilitiesForType(MIME_AVC) }.getOrNull()
                ?: return@mapNotNull null
            val video = capabilities.videoCapabilities ?: return@mapNotNull null
            if (capabilities.maxSupportedInstances < 2) return@mapNotNull null
            if (!video.areSizeAndRateSupported(1_920, 2_160, 60.0)) return@mapNotNull null
            info.name
        }
        .sortedBy { name ->
            when {
                name.contains("low_latency", ignoreCase = true) -> 0
                name.startsWith("c2.", ignoreCase = true) -> 1
                else -> 2
            }
        }
        .firstOrNull()

    private fun isHardwareCodec(info: MediaCodecInfo): Boolean {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            return info.isHardwareAccelerated && !info.isSoftwareOnly
        }
        val name = info.name.lowercase()
        return !name.startsWith("omx.google.") &&
            !name.startsWith("c2.android.") &&
            !name.contains("software")
    }
}
