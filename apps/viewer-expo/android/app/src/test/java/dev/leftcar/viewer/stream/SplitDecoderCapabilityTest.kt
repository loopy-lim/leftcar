package dev.leftcar.viewer.stream

import android.media.MediaCodecInfo
import android.media.MediaFormat
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.shadows.MediaCodecInfoBuilder
import org.robolectric.shadows.ShadowMediaCodecList
import org.robolectric.util.ReflectionHelpers

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE)
class SplitDecoderCapabilityTest {
    @Before fun reset() { ShadowMediaCodecList.reset() }

    private fun codec(name: String, instances: Int, level: Int = MediaCodecInfo.CodecProfileLevel.AVCLevel52, hardware: Boolean = true, canonicalName: String = name) {
        val profile = MediaCodecInfo.CodecProfileLevel().apply {
            this.profile = MediaCodecInfo.CodecProfileLevel.AVCProfileHigh
            this.level = level
        }
        val caps = MediaCodecInfoBuilder.CodecCapabilitiesBuilder.newBuilder()
            .setMediaFormat(MediaFormat.createVideoFormat("video/avc", 1920, 2160))
            .setProfileLevels(arrayOf(profile)).setColorFormats(intArrayOf(MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)).build()
        ReflectionHelpers.setField(caps, "mMaxSupportedInstances", instances)
        val info = MediaCodecInfoBuilder.newBuilder().setName(name)
            .setIsEncoder(false).setIsVendor(true).setIsHardwareAccelerated(hardware)
            .setIsSoftwareOnly(!hardware).setCapabilities(caps).build()
        ReflectionHelpers.setField(info, "mCanonicalName", canonicalName)
        ShadowMediaCodecList.addCodec(info)
    }

    @Test fun `real codec list absence and software-only entries cannot qualify split`() {
        assertNull(SplitDecoderCapability.readHint())
        codec("c2.android.avc.decoder", 16, hardware = false)
        assertNull(SplitDecoderCapability.readHint())
        assertNull(SplitDecoderCapability.findQualifiedCodecName())
    }

    @Test fun `named advertised two-slot capability includes a separate per-instance configuration ceiling`() {
        codec("c2.qti.avc.decoder.low_latency", 2)
        val hint = SplitDecoderCapability.readHint()!!
        assertEquals("c2.qti.avc.decoder.low_latency", hint.codecName)
        assertEquals(2, hint.maxInstances)
        assertTrue(hint.maxInstancePixelRate!! >= 1920.0 * 2160 * 60)
        assertEquals(hint.codecName, SplitDecoderCapability.findQualifiedCodecName())
    }

    @Test fun `large instance hints are capped and insufficient geometry cannot qualify split`() {
        codec("c2.vendor.avc.decoder", 99, level = MediaCodecInfo.CodecProfileLevel.AVCLevel1)
        assertEquals(4, SplitDecoderCapability.readHint()!!.maxInstances)
        assertNull(SplitDecoderCapability.findQualifiedCodecName())
    }

    @Test fun `hardware alias selection hands the canonical codec name to the strict native decoder`() {
        val canonical = "c2.qti.avc.decoder.low_latency"
        codec("OMX.qcom.video.decoder.avc.low_latency", 2, canonicalName = canonical)
        codec(canonical, 2)
        assertEquals(canonical, SplitDecoderCapability.findQualifiedCodecName())
        assertEquals(canonical, SplitDecoderCapability.readHint()!!.codecName)
        assertEquals(2, SplitDecoderCapability.readHint()!!.maxInstances)
    }
}
