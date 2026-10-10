package dev.leftcar.viewer.km

import android.util.Log

/** TEMPORARY: remove after the first real startup cause is captured. No session/input data. */
internal class KmHandshakeProbe(private val fresh: () -> Boolean) {
    enum class Stage {
        PREPARE, PREPARE_READY, START_CONFIG, START_PRECONDITION, START_SECRET, TRANSPORT_SETUP,
        SOCKET_CONNECT, SOCKET_CONNECTED, HELLO_QUEUE, HELLO_ENCRYPT, HELLO_WRITE, HELLO_WRITTEN,
        RECEIVE_FRAME, RECEIVE_AUTH, RECEIVE_DECODE, RECEIVE_DECODED,
        STATE_GATE, STATE_OWNER_GUARD, STATE_NEUTRAL, STATE_UNGRAB, STATE_CONTEXT,
        STATE_COEXISTENCE, STATE_CAPTURE, STATE_CONFIGURE, STATE_COMMIT, STATE_ACK, STATE_APPLY,
        STATE_WAIT, HEARTBEAT, FLUSH, STARTUP,
    }
    enum class Code { BEGIN, OK, FAILED, APPLIED, STOPPED }
    @Volatile var active = true; private set
    private var records = 0

    // One shared lock/count covers worker, main, reader and writer. Reserve record 20 for terminal.
    @Synchronized fun record(stage: Stage, code: Code, error: Throwable? = null) {
        runCatching {
            if (!active || !fresh() || records >= 19) return
            emit(stage, code, error)
        }
    }
    @Synchronized fun finish(code: Code) {
        runCatching {
            if (active && fresh() && records < 20) emit(Stage.STARTUP, code, null)
        }
        active = false
    }
    @Synchronized fun close() { active = false }

    private fun emit(stage: Stage, code: Code, error: Throwable?) {
        records++ // A failed Log call still consumes its slot; diagnostics never replace behavior.
        val type = error?.javaClass?.name?.take(160)?.replace(Regex("[^A-Za-z0-9_.$]"), "_") ?: "-"
        Log.i("LeftcarKmHandshakeProbe", "stage=${stage.name} code=${code.name} class=$type")
    }
}
