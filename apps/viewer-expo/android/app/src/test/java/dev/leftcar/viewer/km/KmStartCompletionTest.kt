package dev.leftcar.viewer.km

import org.junit.Assert.*
import org.junit.Test

class KmStartCompletionTest {
    @Test fun `only applied State completes delayed start and late completion cannot settle twice`() {
        val start = KmStartCompletion<String>()
        val results = mutableListOf<Result<String>>()
        val ticket = start.begin { results += it }
        assertTrue(results.isEmpty())
        start.applied(ticket, "armed")
        start.failed("disconnected")
        start.applied(ticket, "armed")
        assertEquals(listOf("armed"), results.map { it.getOrNull() })
    }
    @Test fun `timeout disconnect cancellation reject once and old State cannot complete replacement`() {
        for (reason in listOf("timeout", "disconnected", "cancelled", "apply failed")) {
            val start = KmStartCompletion<String>(); val results = mutableListOf<Result<String>>()
            val old = start.begin { results += it }
            start.failed(reason); start.failed(reason)
            val replacement = start.begin { results += it }
            start.applied(old, "armed")
            assertEquals(1, results.size)
            assertTrue(results.single().isFailure)
            start.applied(replacement, "armed")
            assertEquals(2, results.size)
            assertEquals("armed", results.last().getOrNull())
        }
    }
}
