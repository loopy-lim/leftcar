package dev.leftcar.viewer.stream

import android.media.MediaCodec
import android.media.MediaCodecList
import android.media.MediaFormat
import java.nio.ByteBuffer
import java.nio.ByteOrder

internal fun opusInitializationData(channels: Int, preSkip: Int): List<ByteArray> {
    require(channels in 1..2 && preSkip in 0..5760)
    val header = ByteBuffer.allocate(19).order(ByteOrder.LITTLE_ENDIAN)
        .put("OpusHead".toByteArray(Charsets.US_ASCII)).put(1).put(channels.toByte())
        .putShort(preSkip.toShort()).putInt(48000).putShort(0).put(0).array()
    fun ns(value: Long) = ByteBuffer.allocate(8).order(ByteOrder.nativeOrder()).putLong(value).array()
    return listOf(header, ns(preSkip.toLong() * 1_000_000_000L / 48000), ns(80_000_000L))
}

/** A MediaCodec belongs to one encoder epoch. No packet or PCM crosses JS. */
internal class OpusAudioDecoder : AutoCloseable {
    companion object {
        fun available(): Boolean = runCatching {
            MediaCodecList(MediaCodecList.REGULAR_CODECS).findDecoderForFormat(format(2, 0)) != null
        }.getOrDefault(false)
        private fun format(channels: Int, preSkip: Int): MediaFormat =
            MediaFormat.createAudioFormat(MediaFormat.MIMETYPE_AUDIO_OPUS, 48000, channels).apply {
                opusInitializationData(channels, preSkip).forEachIndexed { index, bytes ->
                    setByteBuffer("csd-$index", ByteBuffer.wrap(bytes))
                }
            }
    }
    private var codec: MediaCodec? = null
    private var epoch = 0L
    private var channels = 0
    private var preSkip = 0
    private var framesQueued = 0L
    private val info = MediaCodec.BufferInfo()
    var decodeNanoseconds = 0L
        private set
    var decodedFrames = 0L
        private set

    /** Return PCM from available output, including zero outputs during priming. */
    fun decode(packet: ByteArray, length: Int): ByteArray? {
        require(length in 25..1300)
        val header = ByteBuffer.wrap(packet, 0, length).order(ByteOrder.BIG_ENDIAN)
        require(header.int == 0x4c434f31)
        header.short // sequence already validated by the native ring
        require((header.short.toInt() and 65535) == 48000)
        val nextChannels = header.get().toInt()
        require(nextChannels in 1..2 && header.get() == 0.toByte() && header.short.toInt() == 480)
        val nextEpoch = header.long
        val nextSkip = header.short.toInt() and 65535
        val payload = header.short.toInt() and 65535
        require(nextEpoch != 0L && nextSkip <= 5760 && payload in 1..1276 && length == 24 + payload)
        if (codec == null || epoch != nextEpoch || channels != nextChannels || preSkip != nextSkip) {
            close()
            val format = format(nextChannels, nextSkip)
            val name = MediaCodecList(MediaCodecList.REGULAR_CODECS).findDecoderForFormat(format)
                ?: error("Opus decoder unavailable")
            val created = MediaCodec.createByCodecName(name)
            try { created.configure(format, null, null, 0); created.start() }
            catch (failure: Exception) { created.release(); throw failure }
            codec = created
            epoch = nextEpoch
            channels = nextChannels
            preSkip = nextSkip
            framesQueued = 0
        }
        val active = checkNotNull(codec)
        val began = System.nanoTime()
        val index = active.dequeueInputBuffer(0)
        check(index >= 0) { "Opus input stalled" }
        val input = checkNotNull(active.getInputBuffer(index))
        input.clear()
        input.put(packet, 24, payload)
        active.queueInputBuffer(index, 0, payload, framesQueued * 1_000_000L / 48000, 0)
        framesQueued += 480
        val result = drain()
        decodeNanoseconds += System.nanoTime() - began
        return result
    }
    fun drain(): ByteArray? {
        val active = codec ?: return null
        val output = java.io.ByteArrayOutputStream()
        while (true) {
            when(val index = active.dequeueOutputBuffer(info, 0)) {
                MediaCodec.INFO_TRY_AGAIN_LATER -> break
                MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> {
                    val actual = active.outputFormat
                    check(actual.getInteger(MediaFormat.KEY_SAMPLE_RATE) == 48000 && actual.getInteger(MediaFormat.KEY_CHANNEL_COUNT) == channels)
                }
                else -> if (index >= 0) {
                    try {
                        if (info.size > 0) {
                            val bytes = ByteArray(info.size)
                            checkNotNull(active.getOutputBuffer(index)).apply {
                                position(info.offset); limit(info.offset + info.size); get(bytes)
                            }
                            output.write(bytes)
                            decodedFrames += info.size / (channels * 2)
                        }
                    } finally { active.releaseOutputBuffer(index, false) }
                }
            }
        }
        if (output.size() == 0) return null
        val pcm = output.toByteArray()
        require(pcm.size <= 16 * 1024 - 6 && pcm.size % (channels * 2) == 0)
        return ByteBuffer.allocate(pcm.size + 6).order(ByteOrder.BIG_ENDIAN)
            .putShort(48000.toShort()).put(channels.toByte()).put(0)
            .putShort((pcm.size / (channels * 2)).toShort()).put(pcm).array()
    }
    override fun close() {
        val retired = codec
        codec = null
        if (retired != null) { try { retired.stop() } finally { retired.release() } }
    }
}
