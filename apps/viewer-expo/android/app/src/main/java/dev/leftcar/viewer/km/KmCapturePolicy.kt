package dev.leftcar.viewer.km

internal enum class KmCaptureMode { OFF, OBSERVE, CONSUME }

internal object KmCapturePolicy {
    fun mode(observationVerified: Boolean, owner: String): KmCaptureMode = when {
        !observationVerified -> KmCaptureMode.OFF
        owner == "androidToMac" -> KmCaptureMode.CONSUME
        owner == "local" || owner == "macToAndroid" -> KmCaptureMode.OBSERVE
        else -> KmCaptureMode.OFF
    }
}
