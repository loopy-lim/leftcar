package dev.leftcar.viewer.stream

import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import org.junit.Assert.*
import org.junit.Test

class StreamAudioWorkerTest {
    private fun pcm() = byteArrayOf(0xbb.toByte(), 0x80.toByte(), 2, 0, 0, 3, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12)
    private class Output(val writes: (ByteArray, Int, Int) -> Int) : AudioOutput {
        override val bufferFrames = 2880
        override val capacityFrames = 5760
        override val underruns = 0
        override val playbackFrames = 0L
        var released = 0
        override fun write(bytes: ByteArray, offset: Int, length: Int) = writes(bytes, offset, length)
        override fun resize(frames: Int) = frames
        override fun close() { released++ }
    }
    @Test fun `60ms request honors platform minimum without accidental 100ms floor`() {
        assertEquals(2880, audioBufferFrames(48000, 2, 1024))
        assertEquals(4000, audioBufferFrames(48000, 2, 16000))
        assertEquals(2646, audioBufferFrames(44100, 1, 512))
    }
    @Test fun `actual worker retains unwritten bytes and counts only positive progress`() {
        val consumed = mutableListOf<Int>()
        val complete = CountDownLatch(1)
        val calls = AtomicInteger()
        val output = Output { bytes, offset, length ->
            when(calls.getAndIncrement()) {
                0 -> { synchronized(consumed) { consumed.addAll(bytes.slice(offset until offset + 4).map { it.toInt() }) }; 4 }
                1 -> 0
                else -> { synchronized(consumed) { consumed.addAll(bytes.slice(offset until offset + length).map { it.toInt() }) }; complete.countDown(); length }
            }
        }
        val polls = AtomicInteger()
        val player = StreamAudioPlayer("synthetic", { buffer ->
            if (polls.getAndIncrement() == 0) pcm().also { it.copyInto(buffer) }.size else 0
        }, { _, _ -> output })
        player.start()
        assertTrue(complete.await(2, TimeUnit.SECONDS))
        assertTrue(player.stop())
        assertEquals((1..12).toList(), consumed)
        assertEquals(3L, player.metrics.writtenFrames)
        assertEquals(1, output.released)
    }
    @Test fun `stop timeout never releases writing owner and restart waits for its acknowledgement`() {
        val entered = CountDownLatch(1)
        val unblock = CountDownLatch(1)
        val reopened = CountDownLatch(1)
        val created = AtomicInteger()
        val first = Output { _, _, length ->
            entered.countDown()
            while (unblock.count > 0) { try { unblock.await() } catch (_: InterruptedException) {} }
            length
        }
        val second = Output { _, _, length -> length }
        val player = StreamAudioPlayer("synthetic", { buffer -> pcm().also { it.copyInto(buffer) }.size }, { _, _ ->
            if(created.getAndIncrement() == 0) first else second.also { reopened.countDown() }
        })
        player.start()
        assertTrue(entered.await(2, TimeUnit.SECONDS))
        assertFalse(player.stop())
        player.start()
        assertEquals(1, created.get())
        assertEquals(0, first.released)
        unblock.countDown()
        assertTrue(reopened.await(2, TimeUnit.SECONDS))
        assertTrue(player.stop())
        assertEquals(1, first.released)
        assertEquals(1, second.released)
    }
    @Test fun `write error closes failed route and recreates output without counting failed bytes`() {
        val done = CountDownLatch(1)
        val created = AtomicInteger()
        val first = Output { _, _, _ -> -6 }
        val second = Output { _, _, length -> done.countDown(); length }
        val player = StreamAudioPlayer("synthetic", { buffer -> pcm().also { it.copyInto(buffer) }.size }, { _, _ ->
            if(created.getAndIncrement() == 0) first else second
        })
        player.start()
        assertTrue(done.await(2, TimeUnit.SECONDS))
        assertTrue(player.stop())
        assertEquals(1, first.released)
        assertTrue(player.metrics.writeErrors > 0)
    }
    @Test fun `partial success before route failure counts transferred frames only`() {
        val calls = AtomicInteger()
        val failed = CountDownLatch(1)
        val output = Output { _, _, _ -> if (calls.getAndIncrement() == 0) 4 else (-6).also { failed.countDown() } }
        val polls = AtomicInteger()
        val player = StreamAudioPlayer("synthetic", { buffer ->
            if (polls.getAndIncrement() == 0) pcm().also { it.copyInto(buffer) }.size else 0
        }, { _, _ -> output })
        player.start()
        assertTrue(failed.await(2, TimeUnit.SECONDS))
        assertTrue(player.stop())
        assertEquals(1L, player.metrics.writtenFrames)
        assertEquals(1L, player.metrics.writeErrors)
        assertNull(player.metrics.actualBufferFrames)
    }

    @Test fun `real worker expands on underrun within route capacity`() {
        val written = CountDownLatch(1)
        var current = 2880
        var underrunCount = 0
        val output = object : AudioOutput {
            override val bufferFrames get() = current
            override val capacityFrames = 3200
            override val underruns get() = underrunCount
            override val playbackFrames = 1L
            override fun write(bytes: ByteArray, offset: Int, length: Int): Int { underrunCount = 1; written.countDown(); return length }
            override fun resize(frames: Int): Int { current = frames; return current }
            override fun close() {}
        }
        val polls = AtomicInteger()
        val player = StreamAudioPlayer("synthetic", { buffer ->
            if (polls.getAndIncrement() == 0) pcm().also { it.copyInto(buffer) }.size else 0
        }, { _, _ -> output })
        player.start()
        assertTrue(written.await(2, TimeUnit.SECONDS))
        val deadline = System.nanoTime() + 2_000_000_000L
        while (player.metrics.underruns != 1 && System.nanoTime() < deadline) Thread.sleep(1)
        assertEquals(3200, player.metrics.requestedBufferFrames)
        assertEquals(3200, player.metrics.actualBufferFrames)
        assertEquals(1, player.metrics.underruns)
        assertEquals(1L, player.metrics.playbackFrames)
        assertTrue(player.stop())
        assertEquals(3200, current)
        assertNull(player.metrics.requestedBufferFrames)
        assertNull(player.metrics.actualBufferFrames)
        assertNull(player.metrics.underruns)
        assertNull(player.metrics.playbackFrames)
    }

    @Test fun `signaled worker bounds immediate absent and stopped polls then delivers live data`() {
        val phase = AtomicInteger(0)
        val polls = AtomicInteger()
        val delivered = CountDownLatch(1)
        val player = StreamAudioPlayer("synthetic", { buffer ->
            polls.incrementAndGet()
            if (phase.get() == 2) pcm().also { it.copyInto(buffer) }.size else 0
        }, { _, _ -> Output { _, _, length -> delivered.countDown(); length } }, signaledPoll = true)
        player.start()
        try {
            Thread.sleep(120)
            assertTrue("absent owned renderer must not busy poll: ${polls.get()}", polls.get() <= 20)
            phase.set(1); polls.set(0)
            Thread.sleep(120)
            assertTrue("stopped owned renderer must not busy poll: ${polls.get()}", polls.get() <= 20)
            phase.set(2)
            assertTrue(delivered.await(300, TimeUnit.MILLISECONDS))
        } finally { assertTrue(player.stop()) }
    }

    @Test fun `acknowledged retirement invalidates current output while retaining actual writes`() {
        val polls = AtomicInteger()
        val player = StreamAudioPlayer("synthetic", { buffer ->
            if (polls.getAndIncrement() == 0) pcm().also { it.copyInto(buffer) }.size else 0
        }, { _, _ -> Output { _, _, length -> length } })
        player.start()
        val deadline = System.nanoTime() + 2_000_000_000L
        while (player.metrics.writtenFrames == 0L && System.nanoTime() < deadline) Thread.sleep(1)
        assertEquals(3L, player.metrics.writtenFrames)
        assertTrue(player.stop())
        assertNull(player.metrics.effectiveCodec)
        assertNull(player.metrics.actualBufferFrames)
        assertNull(player.metrics.capacityFrames)
        assertNull(player.metrics.playbackFrames)
        assertNull(player.metrics.underruns)
        assertEquals(3L, player.metrics.writtenFrames)
    }

}
