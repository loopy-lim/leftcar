package dev.leftcar.viewer.stream

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import dev.leftcar.viewer.shim.ViewerNative

/**
 * Plays host system audio drained from the native renderer ring.
 *
 * The drain blob is `rate u16 BE | channels u8 | rsv | frames u16 BE
 * | PCM int16 LE`; polling keeps the architecture's ownership boundary
 * (native owns the socket, Kotlin owns playback) without a JNI upcall.
 * AudioTrack's blocking write paces the loop to real time, and a stalled
 * consumer is bounded by the native drop-oldest ring, so this thread can
 * never build latency.
 */
internal class StreamAudioPlayer(private val instanceId: String) {
    companion object {
        /**
         * Two-level poll backoff (Q9b): MAX_PRIORITY 스레드가 무음에서도
         * 12ms마다 깨어나는 것을 줄인다. 데이터가 흐르다 막 끊긴 직후에는
         * 기존의 12ms 폴로 재개 버스트를 빠르게 받고, 계속 비어 있으면 40ms로
         * 늘려 유휴 웨이크를 줄인다. 네이티브 링은 16청크×300프레임
         * (44.1kHz 기준 ~109ms)이라 40ms 지연이 과잉 실행되지 않고, 링이
         * 넘치면 네이티브가 drop-oldest로 경계를 유지한다.
         */
        private const val POLL_ACTIVE_MS = 12L
        private const val POLL_IDLE_MS = 40L
        private const val BLOB_HEADER_BYTES = 6
        private const val MAX_DRAIN_BYTES = 16 * 1024
    }

    private val drainBuffer = ByteArray(MAX_DRAIN_BYTES)
    private var track: AudioTrack? = null
    private var trackRate = 0
    private var trackChannels = 0
    private var thread: Thread? = null
    /** 연속으로 빈 pollAudio 횟수 — 유휴 백오프 단계 판정에 쓴다. */
    private var emptyPolls = 0

    @Volatile
    private var running = false

    fun start() {
        if (running) return
        running = true
        thread = Thread({ runLoop() }, "leftcar-audio").apply {
            priority = Thread.MAX_PRIORITY
            start()
        }
    }

    fun stop() {
        running = false
        thread?.let { current ->
            // The loop parks at most POLL_IDLE_MS between drains, and the
            // AudioTrack write is bounded by its small buffer.
            runCatching { current.join(300) }
        }
        thread = null
        track?.let { audio ->
            runCatching {
                audio.pause()
                audio.flush()
                audio.release()
            }
        }
        track = null
    }

    private fun runLoop() {
        while (running) {
            val length = ViewerNative.pollAudio(instanceId, drainBuffer)
            if (length <= BLOB_HEADER_BYTES) {
                // First empty poll after live audio keeps the tight cadence
                // so a resumed burst is picked up quickly; sustained silence
                // stretches to the idle interval. While audio flows, the
                // blocking AudioTrack write paces the loop and no sleep runs.
                emptyPolls++
                Thread.sleep(if (emptyPolls == 1) POLL_ACTIVE_MS else POLL_IDLE_MS)
                continue
            }
            emptyPolls = 0
            val rate = readU16(0)
            val channels = drainBuffer[2].toInt()
            if (rate <= 0 || channels !in 1..2) continue
            if (track == null || rate != trackRate || channels != trackChannels) {
                rebuildTrack(rate, channels)
            }
            track?.write(drainBuffer, BLOB_HEADER_BYTES, length - BLOB_HEADER_BYTES)
        }
    }

    private fun readU16(offset: Int): Int =
        ((drainBuffer[offset].toInt() and 0xff) shl 8) or
            (drainBuffer[offset + 1].toInt() and 0xff)

    private fun rebuildTrack(rate: Int, channels: Int) {
        track?.let { old ->
            runCatching {
                old.pause()
                old.flush()
                old.release()
            }
        }
        val channelMask = if (channels == 2) {
            AudioFormat.CHANNEL_OUT_STEREO
        } else {
            AudioFormat.CHANNEL_OUT_MONO
        }
        val minBytes = AudioTrack.getMinBufferSize(
            rate,
            channelMask,
            AudioFormat.ENCODING_PCM_16BIT,
        ).coerceAtLeast(rate * channels * 2 / 10)
        val built = AudioTrack(
            AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_MOVIE)
                .build(),
            AudioFormat.Builder()
                .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                .setSampleRate(rate)
                .setChannelMask(channelMask)
                .build(),
            // ~60ms: small enough to keep latency near the video plane's,
            // large enough that scheduler jitter does not underrun.
            (rate * channels * 2 * 60 / 1000).coerceAtLeast(minBytes),
            AudioTrack.MODE_STREAM,
            AudioManager.AUDIO_SESSION_ID_GENERATE,
        )
        trackRate = rate
        trackChannels = channels
        track = built
        built.play()
    }
}

/** Pure header parsing for unit tests: (rate, channels) from a drain blob. */
internal fun parseAudioBlobHeader(blob: ByteArray): Pair<Int, Int>? {
    if (blob.size < 6) return null
    val rate = ((blob[0].toInt() and 0xff) shl 8) or (blob[1].toInt() and 0xff)
    val channels = blob[2].toInt()
    if (rate <= 0 || channels !in 1..2) return null
    return rate to channels
}
