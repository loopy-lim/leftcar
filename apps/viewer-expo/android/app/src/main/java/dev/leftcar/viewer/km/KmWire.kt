package dev.leftcar.viewer.km

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

/** Host↔viewer KM envelope without org.json — Map payload + hand-rolled JSON. */
internal data class KmWire(val epoch: Long, val seq: Long, val payload: Map<String, Any>) {
    val kind: String get() = payload.str("kind")
    companion object {
        fun encode(sessionId: String, epoch: Long, seq: Long, payload: Map<String, Any>): ByteArray =
            encodeObject(linkedMapOf(
                "version" to 1,
                "sessionId" to sessionId,
                "epoch" to epoch,
                "seq" to seq,
                "origin" to "android",
                "payload" to payload,
            )).toByteArray(Charsets.UTF_8).also { require(it.size <= 8192) }

        fun decode(bytes: ByteArray, sessionId: String): KmWire {
            require(bytes.size in 1..8192)
            val envelope = parseObject(String(bytes, Charsets.UTF_8))
            fields(envelope, "version", "sessionId", "epoch", "seq", "origin", "payload")
            require(integer(envelope, "version") == 1L && envelope["sessionId"] == sessionId && envelope["origin"] == "mac")
            val epoch = integer(envelope, "epoch").also { require(it > 0) }
            val seq = integer(envelope, "seq").also { require(it > 0) }
            val p = envelope["payload"]
            require(p is Map<*, *>)
            @Suppress("UNCHECKED_CAST")
            val payload = p as Map<String, Any>
            when (payload["kind"]) {
                "hello", "releaseAll", "heartbeat", "stop" -> fields(payload, "kind")
                "state" -> { fields(payload, "kind", "owner"); require(payload["owner"] in setOf("local", "macToAndroid", "androidToMac")) }
                "move", "wheel" -> {
                    fields(payload, "kind", "dx", "dy")
                    number(payload, "dx", -65536.0, 65536.0); number(payload, "dy", -65536.0, 65536.0)
                }
                "button" -> { fields(payload, "kind", "button", "down"); require(integer(payload, "button") in 0..7); require(payload["down"] is Boolean) }
                "key" -> { fields(payload, "kind", "code", "down"); require(integer(payload, "code") in 0..255); require(payload["down"] is Boolean) }
                "cursor" -> {
                    fields(payload, "kind", "x", "y", "width", "height")
                    val width = integer(payload, "width").also { require(it in 1..32768) }
                    val height = integer(payload, "height").also { require(it in 1..32768) }
                    number(payload, "x", 0.0, width.toDouble()); number(payload, "y", 0.0, height.toDouble())
                }
                "edge" -> { fields(payload, "kind", "returning"); require(payload["returning"] is Boolean) }
                else -> error("Unsupported KM message")
            }
            return KmWire(epoch, seq, payload)
        }

        fun payload(kind: String, vararg values: Pair<String, Any>): Map<String, Any> {
            val p = linkedMapOf<String, Any>("kind" to kind)
            values.forEach { (key, value) -> p[key] = value }
            return p
        }

        private fun fields(value: Map<String, Any>, vararg allowed: String) {
            require(value.keys == allowed.toSet())
        }
        private fun integer(value: Map<String, Any>, key: String): Long {
            val raw = value[key]; require(raw is Int || raw is Long); return (raw as Number).toLong()
        }
        private fun number(value: Map<String, Any>, key: String, min: Double, max: Double): Double {
            val raw = value[key]; require(raw is Number && raw !is Boolean)
            // Reject integral boxed types masquerading — Doubles/Ints/Longs are fine; strings already excluded.
            require(raw is Int || raw is Long || raw is Double || raw is Float)
            return raw.toDouble().also { require(it.isFinite() && it in min..max) }
        }

        private fun encodeObject(value: Map<String, Any>): String = buildString {
            append('{')
            var first = true
            for ((key, entry) in value) {
                if (!first) append(',')
                first = false
                append(encodeString(key)).append(':').append(encodeValue(entry))
            }
            append('}')
        }

        private fun encodeValue(value: Any): String = when (value) {
            is String -> encodeString(value)
            is Boolean -> if (value) "true" else "false"
            is Int, is Long -> value.toString()
            is Float -> value.toDouble().toString()
            is Double -> value.toString()
            is Map<*, *> -> {
                @Suppress("UNCHECKED_CAST")
                encodeObject(value as Map<String, Any>)
            }
            else -> error("Unsupported KM JSON value")
        }

        private fun encodeString(value: String): String = buildString {
            append('"')
            for (ch in value) when (ch) {
                '\\' -> append("\\\\")
                '"' -> append("\\\"")
                '\n' -> append("\\n")
                '\r' -> append("\\r")
                '\t' -> append("\\t")
                else -> if (ch.code < 0x20) append("\\u%04x".format(ch.code)) else append(ch)
            }
            append('"')
        }

        private fun parseObject(text: String): Map<String, Any> {
            val parser = JsonParser(text)
            val value = parser.parseValue()
            parser.skipWs()
            require(parser.done) { "Trailing KM JSON" }
            require(value is Map<*, *>) { "KM envelope must be an object" }
            @Suppress("UNCHECKED_CAST")
            return value as Map<String, Any>
        }
    }
}

internal fun Map<String, Any>.str(key: String): String {
    val raw = this[key]; require(raw is String); return raw
}
internal fun Map<String, Any>.bool(key: String): Boolean {
    val raw = this[key]; require(raw is Boolean); return raw
}
internal fun Map<String, Any>.int(key: String): Int {
    val raw = this[key]; require(raw is Int || raw is Long); return (raw as Number).toInt()
}
internal fun Map<String, Any>.dbl(key: String): Double {
    val raw = this[key]; require(raw is Number && raw !is Boolean); return raw.toDouble()
}

/** Minimal JSON object/number/string/bool parser for the closed KM wire schema. */
private class JsonParser(private val text: String) {
    private var i = 0
    val done: Boolean get() = i >= text.length

    fun skipWs() { while (i < text.length && text[i].isWhitespace()) i++ }

    fun parseValue(): Any {
        skipWs()
        require(i < text.length)
        return when (val ch = text[i]) {
            '{' -> parseObject()
            '"' -> parseString()
            't' -> { expect("true"); true }
            'f' -> { expect("false"); false }
            'n' -> error("KM JSON null unsupported")
            else -> if (ch == '-' || ch in '0'..'9') parseNumber() else error("Unexpected KM JSON $ch")
        }
    }

    private fun parseObject(): Map<String, Any> {
        expect('{')
        val out = linkedMapOf<String, Any>()
        skipWs()
        if (peek('}')) { i++; return out }
        while (true) {
            skipWs()
            val key = parseString()
            skipWs(); expect(':')
            val value = parseValue()
            require(key !in out)
            out[key] = value
            skipWs()
            when {
                peek(',') -> i++
                peek('}') -> { i++; return out }
                else -> error("Expected , or } in KM JSON object")
            }
        }
    }

    private fun parseString(): String {
        expect('"')
        return buildString {
            while (i < text.length) {
                when (val ch = text[i++]) {
                    '"' -> return@buildString
                    '\\' -> {
                        require(i < text.length)
                        when (val esc = text[i++]) {
                            '"', '\\', '/' -> append(esc)
                            'b' -> append('\b')
                            'f' -> append('\u000c')
                            'n' -> append('\n')
                            'r' -> append('\r')
                            't' -> append('\t')
                            'u' -> {
                                require(i + 4 <= text.length)
                                append(text.substring(i, i + 4).toInt(16).toChar())
                                i += 4
                            }
                            else -> error("Bad KM JSON escape")
                        }
                    }
                    else -> {
                        require(ch.code >= 0x20)
                        append(ch)
                    }
                }
            }
            error("Unterminated KM JSON string")
        }
    }

    private fun parseNumber(): Number {
        val start = i
        if (peek('-')) i++
        require(i < text.length && text[i] in '0'..'9')
        if (text[i] == '0') i++
        else while (i < text.length && text[i] in '0'..'9') i++
        var fractional = false
        if (peek('.')) {
            fractional = true; i++
            require(i < text.length && text[i] in '0'..'9')
            while (i < text.length && text[i] in '0'..'9') i++
        }
        if (peek('e') || peek('E')) {
            fractional = true; i++
            if (peek('+') || peek('-')) i++
            require(i < text.length && text[i] in '0'..'9')
            while (i < text.length && text[i] in '0'..'9') i++
        }
        val raw = text.substring(start, i)
        if (!fractional) {
            val long = raw.toLong()
            return if (long in Int.MIN_VALUE..Int.MAX_VALUE) long.toInt() else long
        }
        return raw.toDouble().also { require(it.isFinite()) }
    }

    private fun expect(literal: String) {
        require(text.startsWith(literal, i)); i += literal.length
    }
    private fun expect(ch: Char) { require(i < text.length && text[i] == ch); i++ }
    private fun peek(ch: Char): Boolean = i < text.length && text[i] == ch
}
