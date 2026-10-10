package dev.leftcar.viewer.km

import java.nio.ByteBuffer
import javax.crypto.Cipher
import javax.crypto.Mac
import javax.crypto.spec.IvParameterSpec
import javax.crypto.spec.SecretKeySpec

/** Matches secure-channel StreamSealer; each direction has an independent key/counter. */
internal class KmCipher(private val key: ByteArray) : AutoCloseable {
    private var counter = 1L
    private var received = -1L
    init { require(key.size == 32) }

    @Synchronized fun seal(plain: ByteArray): ByteArray {
        require(plain.size in 1..8192 && counter < Long.MAX_VALUE)
        val number = counter++
        val cipher = Cipher.getInstance("ChaCha20-Poly1305")
        cipher.init(Cipher.ENCRYPT_MODE, SecretKeySpec(key, "ChaCha20"), IvParameterSpec(nonce(number)))
        return ByteBuffer.allocate(8).putLong(number).array() + cipher.doFinal(plain)
    }

    @Synchronized fun open(frame: ByteArray): ByteArray {
        require(frame.size in 25..8216)
        val number = ByteBuffer.wrap(frame, 0, 8).long
        require(number > 0 && number > received) { "Stale KM frame" }
        val cipher = Cipher.getInstance("ChaCha20-Poly1305")
        cipher.init(Cipher.DECRYPT_MODE, SecretKeySpec(key, "ChaCha20"), IvParameterSpec(nonce(number)))
        val plain = cipher.doFinal(frame, 8, frame.size - 8)
        received = number
        return plain
    }

    override fun close() { key.fill(0) }

    companion object {
        private fun nonce(value: Long) = ByteBuffer.allocate(12).putInt(0).putLong(value).array()
        fun keys(seed: ByteArray, sessionId: String): Pair<ByteArray, ByteArray> {
            require(seed.size == 32 && sessionId.toByteArray().size in 1..128)
            fun hmac(key: ByteArray, data: ByteArray): ByteArray = Mac.getInstance("HmacSHA256").run {
                init(SecretKeySpec(key, "HmacSHA256")); doFinal(data)
            }
            val prk = hmac(sessionId.toByteArray(Charsets.UTF_8), seed)
            val info = "leftcar/km/v1".toByteArray(Charsets.UTF_8)
            val first = hmac(prk, info + byteArrayOf(1))
            val second = hmac(prk, first + info + byteArrayOf(2))
            prk.fill(0)
            return first to second
        }
    }
}
