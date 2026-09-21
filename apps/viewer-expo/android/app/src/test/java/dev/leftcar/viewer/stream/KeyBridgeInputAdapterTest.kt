package dev.leftcar.viewer.stream

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class KeyBridgeInputAdapterTest {
    @Test
    fun `availability distinguishes missing ready and old KeyBridge`() {
        assertEquals(
            KeyBridgeAvailability.AVAILABLE_WITHOUT_KEYBRIDGE,
            resolveKeyBridgeAvailability(packageInstalled = false, serviceResolvable = false),
        )
        assertEquals(
            KeyBridgeAvailability.READY,
            resolveKeyBridgeAvailability(packageInstalled = true, serviceResolvable = true),
        )
        assertEquals(
            KeyBridgeAvailability.UPDATE_REQUIRED,
            resolveKeyBridgeAvailability(packageInstalled = true, serviceResolvable = false),
        )
    }

    @Test
    fun `release and close are idempotent and unbind once`() {
        val backend = FakeKeyBridgeBindingBackend()
        val adapter = KeyBridgeInputAdapter(backend, directExecutor, directExecutor)
        adapter.prepare()
        var acquired = false
        adapter.acquire { acquired = it }
        assertTrue(acquired)

        adapter.release()
        adapter.release()
        adapter.close()
        adapter.close()

        assertEquals(1, backend.bindCalls)
        assertEquals(1, backend.unbindCalls)
    }

    private class FakeKeyBridgeBindingBackend : KeyBridgeBindingBackend {
        var bindCalls = 0
        var unbindCalls = 0
        private val remote = object : KeyBridgeOwnershipRemote {
            override fun acquire() = true
            override fun release() = Unit
        }

        override fun packageInstalled() = true
        override fun serviceResolvable() = true
        override fun bind(onConnected: (KeyBridgeOwnershipRemote) -> Unit, onDisconnected: () -> Unit): Boolean {
            bindCalls += 1
            onConnected(remote)
            return true
        }
        override fun unbind() {
            unbindCalls += 1
        }
    }

    private companion object {
        val directExecutor = java.util.concurrent.Executor { it.run() }
    }
}
