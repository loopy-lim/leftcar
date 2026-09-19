package com.asterinet.react.tcpsocket

import java.util.concurrent.AbstractExecutorService
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.util.ReflectionHelpers

/** Run the dependency's actual queued close, including a missing connection. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE)
class TcpSocketCloseTest {
    private class QueuedExecutor : AbstractExecutorService() {
        val tasks = ArrayDeque<Runnable>()
        override fun execute(command: Runnable) { tasks.addLast(command) }
        fun drain() { while (tasks.isNotEmpty()) tasks.removeFirst().run() }
        override fun shutdown() = Unit
        override fun shutdownNow(): MutableList<Runnable> = mutableListOf()
        override fun isShutdown() = false
        override fun isTerminated() = false
        override fun awaitTermination(timeout: Long, unit: TimeUnit) = true
    }

    @Test fun `failed or already removed connection can be closed repeatedly`() {
        val module = TcpSocketModule(null)
        val executor = QueuedExecutor()
        ReflectionHelpers.setField(module, "executorService", executor)
        module.end(15)
        module.destroy(15)
        executor.drain() // Previously threw No socket with id 15 on the native worker.
    }

    @Test fun `queued close still closes a live client but tolerates its removal`() {
        val module = TcpSocketModule(null)
        val executor = QueuedExecutor()
        ReflectionHelpers.setField(module, "executorService", executor)
        val sockets = ReflectionHelpers.getField<ConcurrentHashMap<Int, TcpSocket>>(module, "socketMap")
        var closes = 0
        sockets[16] = object : TcpSocketClient(null, 16, null) {
            override fun destroy() { closes++ }
        }
        module.end(16)
        executor.drain()
        assertEquals(1, closes)
        module.destroy(16)
        sockets.remove(16)
        executor.drain()
        assertEquals(1, closes)
    }
}
