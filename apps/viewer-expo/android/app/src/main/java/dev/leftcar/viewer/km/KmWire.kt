package dev.leftcar.viewer.km

import org.json.JSONObject

internal class KmEpochGate(initial: Long) {
    var epoch = initial; private set
    private var sequence = 0L
    init { require(initial > 0) }
    fun accept(incomingEpoch: Long, seq: Long, kind: String) {
        require(incomingEpoch >= epoch && seq > sequence)
        require(incomingEpoch == epoch || kind == "state")
        epoch = incomingEpoch; sequence = seq
    }
}

internal data class KmWire(val epoch: Long, val seq: Long, val payload: JSONObject) {
    val kind: String get() = payload.getString("kind")
    companion object {
        fun encode(sessionId: String, epoch: Long, seq: Long, payload: JSONObject): ByteArray =
            JSONObject().put("version", 1).put("sessionId", sessionId).put("epoch", epoch)
                .put("seq", seq).put("origin", "android").put("payload", payload)
                .toString().toByteArray(Charsets.UTF_8).also { require(it.size <= 8192) }

        fun decode(bytes: ByteArray, sessionId: String): KmWire {
            require(bytes.size in 1..8192)
            val envelope = JSONObject(String(bytes, Charsets.UTF_8))
            fields(envelope, "version", "sessionId", "epoch", "seq", "origin", "payload")
            require(integer(envelope, "version") == 1L && envelope.get("sessionId") == sessionId && envelope.get("origin") == "mac")
            val epoch = integer(envelope, "epoch").also { require(it > 0) }
            val seq = integer(envelope, "seq").also { require(it > 0) }
            val p = envelope.getJSONObject("payload")
            when (p.get("kind")) {
                "hello", "releaseAll", "heartbeat", "stop" -> fields(p, "kind")
                "state" -> { fields(p, "kind", "owner"); require(p.get("owner") in setOf("local", "macToAndroid", "androidToMac")) }
                "move", "wheel" -> {
                    fields(p, "kind", "dx", "dy")
                    number(p, "dx", -65536.0, 65536.0); number(p, "dy", -65536.0, 65536.0)
                }
                "button" -> { fields(p, "kind", "button", "down"); require(integer(p, "button") in 0..7); require(p.get("down") is Boolean) }
                "key" -> { fields(p, "kind", "code", "down"); require(integer(p, "code") in 0..255); require(p.get("down") is Boolean) }
                "cursor" -> {
                    fields(p, "kind", "x", "y", "width", "height")
                    val width = integer(p, "width").also { require(it in 1..32768) }
                    val height = integer(p, "height").also { require(it in 1..32768) }
                    number(p, "x", 0.0, width.toDouble()); number(p, "y", 0.0, height.toDouble())
                }
                "edge" -> { fields(p, "kind", "returning"); require(p.get("returning") is Boolean) }
                else -> error("Unsupported KM message")
            }
            return KmWire(epoch, seq, p)
        }

        fun payload(kind: String, vararg values: Pair<String, Any>): JSONObject = JSONObject().put("kind", kind).also { p ->
            values.forEach { (key, value) -> p.put(key, value) }
        }
        private fun fields(value: JSONObject, vararg allowed: String) { require(value.keys().asSequence().toSet() == allowed.toSet()) }
        private fun integer(value: JSONObject, key: String): Long {
            val raw = value.get(key); require(raw is Int || raw is Long); return (raw as Number).toLong()
        }
        private fun number(value: JSONObject, key: String, min: Double, max: Double): Double {
            val raw = value.get(key); require(raw is Number)
            return raw.toDouble().also { require(it.isFinite() && it in min..max) }
        }
    }
}
