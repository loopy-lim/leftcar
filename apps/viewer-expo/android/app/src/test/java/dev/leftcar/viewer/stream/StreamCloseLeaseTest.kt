package dev.leftcar.viewer.stream

import org.junit.Assert.*
import org.junit.Test

class StreamCloseLeaseTest {
    @Test fun `old close cannot finish a replacement and timeout can retry without acknowledgment`() {
        val lease = StreamCloseLease("new")
        val results = mutableListOf<Boolean>()
        assertFalse(lease.request("old") { results += it })
        assertEquals(listOf(false), results)
        assertTrue(lease.request("new") { results += it })
        var finished = false
        lease.completeRelease { finished }
        assertEquals(listOf(false, false), results)
        assertFalse(lease.released)
        finished = true
        assertFalse(lease.request("new") { results += it })
        assertEquals(listOf(false, false, true), results)
        assertTrue(lease.released)
    }
    @Test fun `concurrent closes settle only after actual release and exactly once`() {
        val lease = StreamCloseLease("one")
        var acknowledgments = 0
        lease.request("one") { if (it) acknowledgments++ }
        lease.request("one") { if (it) acknowledgments++ }
        assertEquals(0, acknowledgments)
        lease.completeRelease { true }
        lease.completeRelease { error("completed release must not execute twice") }
        assertEquals(2, acknowledgments)
    }
    @Test fun `background release does not acknowledge before completion or run twice`() {
        var finish: ((Boolean) -> Unit)? = null
        var submitted = 0
        val lease = StreamCloseLease("one") { _, done -> submitted++; finish = done }
        var acknowledgments = 0
        lease.request("one") { if (it) acknowledgments++ }
        lease.completeRelease { true }
        lease.completeRelease { error("duplicate") }
        assertEquals(1, submitted)
        assertEquals(0, acknowledgments)
        assertFalse(lease.released)
        finish!!(true)
        assertEquals(1, acknowledgments)
        assertTrue(lease.released)
    }

    @Test fun `failed replacement retains incumbent cleanup ownership and cannot overwrite a newer launch`() {
        val incumbent = StreamCloseLease("old")
        val failed = StreamCloseLease("failed")
        val leases = mutableMapOf("port" to failed)
        assertTrue(restoreFailedStreamLaunch(leases, "port", failed, incumbent))
        assertSame(incumbent, leases["port"])
        assertFalse(incumbent.released)
        val newer = StreamCloseLease("newer")
        leases["port"] = newer
        assertFalse(restoreFailedStreamLaunch(leases, "port", failed, incumbent))
        assertSame(newer, leases["port"])
    }

    @Test fun `pending replacement close waits for its registered Activity and release`() {
        class Window { var finishes = 0 }
        val old = Window()
        val replacement = Window()
        val oldLease = StreamCloseLease("old")
        val leases = mutableMapOf("port" to oldLease)
        val registry = StreamActivityRegistry<Window>(leases) { it.finishes++ }
        registry.register("port", "old", old)
        // openStream publishes the new lease before Android delivers its intent.
        val pending = StreamCloseLease("new")
        leases["port"] = pending
        val results = mutableListOf<Boolean>()
        registry.close("port", "new") { results += it }
        assertEquals(0, old.finishes)
        assertFalse(oldLease.released)
        assertTrue(results.isEmpty())
        registry.register("port", "old", old)
        assertEquals(0, old.finishes)
        var oldAttempts = 0
        assertFalse(registry.release("port", "old", old) { ++oldAttempts > 1 })
        assertEquals(1, oldAttempts)
        assertTrue(results.isEmpty())
        registry.register("port", "new", replacement)
        assertEquals(1, replacement.finishes)
        assertTrue(results.isEmpty())
        var newAttempts = 0
        assertTrue(registry.release("port", "new", replacement) { newAttempts++; true })
        assertEquals(listOf(false), results)
        registry.close("port", "new") { results += it }
        assertEquals(2, oldAttempts)
        assertEquals(1, newAttempts)
        assertEquals(listOf(false, true), results)
        registry.release("port", "old", old) { error("completed old native state must not release twice") }
    }

    @Test fun `same Activity transfers its native owner without requiring a second destruction`() {
        class Window { var finishes = 0 }
        val window = Window()
        val old = StreamCloseLease("old")
        val leases = mutableMapOf("port" to old)
        val registry = StreamActivityRegistry<Window>(leases) { it.finishes++ }
        registry.register("port", "old", window)
        leases["port"] = StreamCloseLease("new")
        val results = mutableListOf<Boolean>()
        registry.close("port", "new") { results += it }
        registry.register("port", "new", window)
        assertEquals(1, window.finishes)
        assertTrue(results.isEmpty())
        registry.release("port", "old", window) { error("same native owner transferred; stale release cannot free it") }
        registry.release("port", "new", window) { true }
        assertEquals(listOf(true), results)
    }

    @Test fun `reconnect history retains current plus every failed pending and inflight obligation`() {
        val leases = mutableMapOf("port" to StreamCloseLease("failed"))
        val registry = StreamActivityRegistry<Any>(leases) {}
        val failed = Any()
        registry.register("port", "failed", failed)
        registry.release("port", "failed", failed) { false }
        var complete: ((Boolean) -> Unit)? = null
        leases["port"] = StreamCloseLease("inflight") { _, done -> complete = done }
        val inflight = Any()
        registry.register("port", "inflight", inflight)
        registry.release("port", "inflight", inflight) { true }
        val pending = Any()
        leases["port"] = StreamCloseLease("pending")
        registry.register("port", "pending", pending)
        val reused = Any()
        repeat(2_000) { i ->
            val generation = "reconnect-$i"
            leases["port"] = StreamCloseLease(generation)
            registry.register("port", generation, reused)
        }
        assertEquals(4, registry.retainedGenerationCount("port"))
        complete!!(true)
        assertEquals(3, registry.retainedGenerationCount("port"))
        registry.release("port", "failed", failed) { true }
        assertEquals(2, registry.retainedGenerationCount("port"))
        registry.release("port", "pending", pending) { true }
        assertEquals(1, registry.retainedGenerationCount("port"))
    }

    @Test fun `same generation recreation owns a fresh native release after completed destruction`() {
        val old = Any()
        val recreated = Any()
        val finished = mutableListOf<Any>()
        val leases = mutableMapOf("port" to StreamCloseLease("17"))
        val registry = StreamActivityRegistry<Any>(leases) { finished += it }
        registry.register("port", "17", old)
        var oldCalls = 0
        registry.release("port", "17", old) { oldCalls++; true }
        registry.register("port", "17", recreated)
        assertSame(recreated, registry.current("port"))
        assertTrue(finished.isEmpty())
        var recreatedCalls = 0
        registry.release("port", "17", recreated) { recreatedCalls++; true }
        val results = mutableListOf<Boolean>()
        registry.close("port", "17") { results += it }
        assertEquals(1, oldCalls)
        assertEquals(1, recreatedCalls)
        assertEquals(listOf(true), results)
    }

    @Test fun `recreation while old native release is pending cannot acknowledge close early`() {
        val executions = mutableListOf<Pair<() -> Boolean, (Boolean) -> Unit>>()
        val leases = mutableMapOf("port" to StreamCloseLease("17") { release, done -> executions += release to done })
        val finished = mutableListOf<Any>()
        val registry = StreamActivityRegistry<Any>(leases) { finished += it }
        val old = Any()
        val recreated = Any()
        registry.register("port", "17", old)
        var oldCalls = 0
        var newCalls = 0
        registry.release("port", "17", old) { oldCalls++; true }
        val results = mutableListOf<Boolean>()
        registry.close("port", "17") { results += it }
        registry.register("port", "17", recreated)
        assertEquals(listOf(recreated), finished)
        registry.release("port", "17", recreated) { newCalls++; true }
        assertEquals(2, executions.size)
        val first = executions[0]
        first.second(first.first())
        assertTrue(results.isEmpty())
        val second = executions[1]
        second.second(second.first())
        assertEquals(listOf(true), results)
        registry.release("port", "17", old) { error("old release repeated") }
        registry.release("port", "17", recreated) { error("new release repeated") }
        assertEquals(1, oldCalls)
        assertEquals(1, newCalls)
    }

    @Test fun `pending destruction without close does not finish its recreated Activity`() {
        val executions = mutableListOf<Pair<() -> Boolean, (Boolean) -> Unit>>()
        val leases = mutableMapOf("port" to StreamCloseLease("17") { release, done -> executions += release to done })
        val registry = StreamActivityRegistry<Any>(leases) { error("recreation is not a close request") }
        val old = Any()
        val recreated = Any()
        registry.register("port", "17", old)
        registry.release("port", "17", old) { true }
        assertTrue(registry.register("port", "17", recreated))
        assertSame(recreated, registry.current("port"))
        registry.release("port", "17", recreated) { true }
        assertEquals(2, executions.size)
        executions.forEach { (release, done) -> done(release()) }
        assertTrue(registry.isReleased("port"))
    }

    @Test fun `onNewIntent cannot transfer a native state whose cleanup is already in flight`() {
        val executions = mutableListOf<Pair<() -> Boolean, (Boolean) -> Unit>>()
        val old = StreamCloseLease("old") { release, done -> executions += release to done }
        val leases = mutableMapOf("port" to old)
        val activity = Any()
        val registry = StreamActivityRegistry<Any>(leases) {}
        registry.register("port", "old", activity)
        var calls = 0
        registry.release("port", "old", activity) { calls++; true }
        leases["port"] = StreamCloseLease("new")
        val results = mutableListOf<Boolean>()
        registry.close("port", "new") { results += it }
        assertFalse(registry.register("port", "new", activity))
        registry.release("port", "new", activity) { error("unacquired successor cannot release native state") }
        assertTrue(results.isEmpty())
        executions.single().let { (release, done) -> done(release()) }
        assertEquals(1, calls)
        assertEquals(listOf(true), results)
    }

    @Test fun `missing registration still owns an exactly once captured cleanup`() {
        val activity = Any()
        val registry = StreamActivityRegistry<Any>(mutableMapOf()) {}
        var calls = 0
        registry.release("restored", "17", activity) { calls++; true }
        registry.release("restored", "17", activity) { error("duplicate orphan cleanup") }
        assertEquals(1, calls)
    }

}
