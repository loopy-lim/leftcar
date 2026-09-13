package dev.leftcar.viewer.stream

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import dev.leftcar.viewer.shim.ViewerNative

/** Actual platform values are nullable until an output exists. These are
 * application buffers and playback counters, never end-to-end latency. */
internal data class AudioPlaybackMetrics(
    val requestedBufferFrames: Int? = null,
    val actualBufferFrames: Int? = null,
    val capacityFrames: Int? = null,
    val underruns: Int? = null,
    val playbackFrames: Long? = null,
    val writtenFrames: Long = 0,
    val writeErrors: Long = 0,
    val requestedCodec: String = "pcm",
    val effectiveCodec: String? = null,
    val decodeNanoseconds: Long? = null,
)

internal interface AudioOutput {
    val bufferFrames: Int
    val capacityFrames: Int
    val underruns: Int
    val playbackFrames: Long
    fun write(bytes: ByteArray, offset: Int, length: Int): Int
    fun resize(frames: Int): Int
    fun close()
}

internal fun audioBufferFrames(rate: Int, channels: Int, minimumBytes: Int): Int =
    maxOf(rate * 60 / 1000, (minimumBytes.coerceAtLeast(0) + channels * 2 - 1) / (channels * 2))

private class PlatformAudioOutput(rate: Int, channels: Int) : AudioOutput {
    private val mask = if (channels == 2) AudioFormat.CHANNEL_OUT_STEREO else AudioFormat.CHANNEL_OUT_MONO
    private val minimum = AudioTrack.getMinBufferSize(rate, mask, AudioFormat.ENCODING_PCM_16BIT)
    private val requested = audioBufferFrames(rate, channels, minimum)
    private val track: AudioTrack
    init {
        check(minimum > 0) { "AudioTrack format unavailable: $minimum" }
        track = AudioTrack(
            AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_MOVIE).build(),
            AudioFormat.Builder().setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                .setSampleRate(rate).setChannelMask(mask).build(),
            maxOf(requested, rate * 120 / 1000) * channels * 2,
            AudioTrack.MODE_STREAM, AudioManager.AUDIO_SESSION_ID_GENERATE,
        )
        try {
            check(track.state == AudioTrack.STATE_INITIALIZED)
            track.setBufferSizeInFrames(requested)
            track.play()
        } catch (failure: Throwable) {
            track.release()
            throw failure
        }
    }
    override val bufferFrames get() = track.bufferSizeInFrames
    override val capacityFrames get() = track.bufferCapacityInFrames
    override val underruns get() = track.underrunCount
    override val playbackFrames get() = track.playbackHeadPosition.toLong() and 0xffffffffL
    override fun write(bytes: ByteArray, offset: Int, length: Int) =
        track.write(bytes, offset, length, AudioTrack.WRITE_NON_BLOCKING)
    override fun resize(frames: Int) = track.setBufferSizeInFrames(frames)
    override fun close() {
        try { track.pause(); track.flush() } finally { track.release() }
    }
}

/** One worker owns each output and drain buffer through its final release.
 * Cancellation never releases a track under a pending write. A start during
 * cancellation queues a successor; only the retired worker may publish it. */
internal class StreamAudioPlayer(
    private val instanceId: String,
    private val poll: (ByteArray) -> Int = { ViewerNative.pollAudio(instanceId, it) },
    private val createOutput: (Int, Int) -> AudioOutput = ::PlatformAudioOutput,
    private val onCodecFallback: () -> Unit = {},
    private val signaledPoll: Boolean = false,
    private val nanoTime: () -> Long = System::nanoTime,
) {
    private class Worker {
        @Volatile var cancelled = false
        lateinit var thread: Thread
    }
    private val ownership = Any()
    private var worker: Worker? = null
    private var wanted = false
    @Volatile private var opusRequested = false
    @Volatile private var opusAllowed = false
    fun configureOpus(requested: Boolean): Boolean {
        if (requested != opusRequested) {
            opusRequested = requested
            opusAllowed = requested && OpusAudioDecoder.available()
        }
        metrics = metrics.copy(requestedCodec = if (requested) "opus128k" else "pcm")
        return opusAllowed
    }
    @Volatile var metrics = AudioPlaybackMetrics()
        private set

    fun start() = synchronized(ownership) {
        wanted = true
        if (worker == null) launchWorker()
    }

    /** True acknowledges worker termination; false leaves release with it. */
    fun stop(): Boolean {
        val retiring = synchronized(ownership) {
            wanted = false
            worker?.also { it.cancelled = true; it.thread.interrupt() }
        } ?: return true
        try { retiring.thread.join(300) } catch (_: InterruptedException) { Thread.currentThread().interrupt() }
        return !retiring.thread.isAlive
    }

    private fun launchWorker() {
        val next = Worker()
        worker = next
        next.thread = Thread({ runLoop(next) }, "leftcar-audio").apply { start() }
    }

    private fun runLoop(owner: Worker) {
        val buffer = ByteArray(16 * 1024)
        var output: AudioOutput? = null
        var decoder: OpusAudioDecoder? = null
        var wireEpoch = 0L
        var effectiveCodec: String? = null
        var rate = 0
        var channels = 0
        var emptyPolls = 0
        var writtenBytes = 0L
        var errors = 0L
        var requested = 0
        var observedUnderruns = 0
        var stableSince = nanoTime()
        fun closeOutput() { output?.let { runCatching { it.close() } }; output = null }
        fun park(ms: Long) { try { Thread.sleep(ms) } catch (_: InterruptedException) {} }
        try {
            while (!owner.cancelled) {
                val pollBegan = nanoTime()
                var length = poll(buffer)
                val pollWaited = nanoTime() - pollBegan
                if (length >= 24 && buffer[0] == 0x4c.toByte() && buffer[1] == 0x43.toByte() && buffer[2] == 0x4f.toByte() && buffer[3] == 0x31.toByte()) {
                    if (!opusAllowed) continue
                    try {
                        val epoch = java.nio.ByteBuffer.wrap(buffer, 12, 8).long
                        if (wireEpoch != epoch || effectiveCodec != "opus128k") { closeOutput(); wireEpoch = epoch }
                        val active = decoder ?: OpusAudioDecoder().also { decoder = it }
                        val pcm = active.decode(buffer, length)
                        effectiveCodec = "opus128k"
                        if (pcm == null) continue
                        pcm.copyInto(buffer)
                        length = pcm.size
                    } catch (_: Exception) {
                        opusAllowed = false
                        metrics = metrics.copy(effectiveCodec = null, actualBufferFrames = null, underruns = null, playbackFrames = null)
                        runCatching { decoder?.close() }; decoder = null
                        closeOutput()
                        onCodecFallback()
                        continue
                    }
                } else if (length > 6) {
                    if (effectiveCodec != "pcm") { closeOutput(); runCatching { decoder?.close() }; decoder = null }
                    effectiveCodec = "pcm"
                } else if (decoder != null && opusAllowed) {
                    try {
                        decoder?.drain()?.let { pcm -> pcm.copyInto(buffer); length = pcm.size }
                    } catch (_: Exception) {
                        opusAllowed = false
                        metrics = metrics.copy(effectiveCodec = null, actualBufferFrames = null, underruns = null, playbackFrames = null)
                        runCatching { decoder?.close() }; decoder = null
                        closeOutput(); onCodecFallback()
                    }
                }
                if (owner.cancelled) break
                if (length <= 6 || length > buffer.size) {
                    // Owned JNI can return immediately before attach or after stop.
                    // A signaled poll is not proof that this call actually waited.
                    if (!signaledPoll) park(if (++emptyPolls == 1) 12 else 40)
                    else if (pollWaited < 12_000_000L) park(12)
                    continue
                }
                emptyPolls = 0
                val header = parseAudioBlobHeader(buffer) ?: continue
                val frameBytes = header.second * 2
                val frames = ((buffer[4].toInt() and 255) shl 8) or (buffer[5].toInt() and 255)
                if (length != 6 + frames * frameBytes) continue
                try {
                    if (output == null || rate != header.first || channels != header.second) {
                        closeOutput()
                        rate = header.first
                        channels = header.second
                        output = createOutput(rate, channels)
                        requested = audioBufferFrames(rate, channels, 0)
                        observedUnderruns = output!!.underruns
                        stableSince = nanoTime()
                        writtenBytes = 0
                    }
                    val active = output ?: continue
                    var offset = 6
                    var zeroSince = nanoTime()
                    while (offset < length && !owner.cancelled) {
                        val written = active.write(buffer, offset, length - offset)
                        if (written < 0 || written > length - offset) {
                            errors++
                            closeOutput()
                            break
                        }
                        if (written == 0) {
                            // A wedged/changed route may never make progress.
                            if (nanoTime() - zeroSince >= 250_000_000L) {
                                errors++
                                closeOutput()
                                break
                            }
                            park(2)
                            continue
                        }
                        offset += written
                        writtenBytes += written
                        zeroSince = nanoTime()
                    }
                    if (output != null) {
                        val now = nanoTime()
                        val underruns = active.underruns
                        val base = audioBufferFrames(rate, channels, 0)
                        val ceiling = minOf(active.capacityFrames, maxOf(base, rate * 120 / 1000))
                        val nextRequest = when {
                            underruns > observedUnderruns -> (active.bufferFrames + rate / 100).coerceAtMost(ceiling)
                            now - stableSince >= 5_000_000_000L -> (active.bufferFrames - rate / 100).coerceAtLeast(base).coerceAtMost(ceiling)
                            else -> requested
                        }
                        if (nextRequest != requested) {
                            if (active.resize(nextRequest) >= 0) requested = nextRequest
                            stableSince = now
                        }
                        if (underruns != observedUnderruns) stableSince = now
                        observedUnderruns = underruns
                        metrics = AudioPlaybackMetrics(requested, active.bufferFrames, active.capacityFrames,
                            underruns, active.playbackFrames, writtenBytes / frameBytes, errors,
                            if (opusRequested) "opus128k" else "pcm", effectiveCodec, decoder?.decodeNanoseconds)
                    } else metrics = metrics.copy(writtenFrames = writtenBytes / frameBytes, writeErrors = errors, actualBufferFrames = null, capacityFrames = null, underruns = null, playbackFrames = null)
                } catch (_: Exception) {
                    errors++
                    closeOutput()
                    metrics = metrics.copy(actualBufferFrames = null, capacityFrames = null,
                        underruns = null, playbackFrames = null, writeErrors = errors)
                    park(40)
                }
            }
        } finally {
            runCatching { decoder?.close() }
            closeOutput()
            metrics = metrics.copy(effectiveCodec = null, requestedBufferFrames = null,
                actualBufferFrames = null, capacityFrames = null, underruns = null,
                playbackFrames = null, decodeNanoseconds = null)
            synchronized(ownership) {
                if (worker === owner) {
                    worker = null
                    if (wanted) launchWorker()
                }
            }
        }
    }
}

/** Legacy PCM header; codec-specific blobs must be explicitly versioned. */
internal fun parseAudioBlobHeader(blob: ByteArray): Pair<Int, Int>? {
    if (blob.size < 6 || blob[3] != 0.toByte()) return null
    val rate = ((blob[0].toInt() and 0xff) shl 8) or (blob[1].toInt() and 0xff)
    val channels = blob[2].toInt()
    if (rate <= 0 || channels !in 1..2) return null
    return rate to channels
}
