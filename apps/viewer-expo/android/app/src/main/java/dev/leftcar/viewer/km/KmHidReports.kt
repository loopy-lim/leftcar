package dev.leftcar.viewer.km

import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Standard USB HID boot keyboard and relative mouse reports; no Android framework dependency. */
internal class KmHidReports {
    private val keys = linkedSetOf<Int>()
    private var modifiers = 0
    private var buttons = 0

    fun key(usage: Int, down: Boolean) {
        require(usage in 4..0x73 || usage in 0xe0..0xe7)
        if (usage >= 0xe0) {
            val bit = 1 shl (usage - 0xe0)
            modifiers = if (down) modifiers or bit else modifiers and bit.inv()
        } else if (down) {
            require(usage in keys || keys.size < 6) { "Keyboard rollover limit" }
            keys.add(usage)
        } else keys.remove(usage)
    }

    fun button(bit: Int, down: Boolean) {
        require(bit in listOf(1, 2, 4, 8, 16))
        buttons = if (down) buttons or bit else buttons and bit.inv()
    }

    fun keyboard(): ByteArray = ByteArray(8).also { out ->
        out[0] = modifiers.toByte()
        keys.forEachIndexed { index, code -> out[index + 2] = code.toByte() }
    }

    fun mouse(dx: Int, dy: Int, vertical: Int, horizontal: Int): ByteArray {
        require(dx in -32767..32767 && dy in -32767..32767)
        require(vertical in -127..127 && horizontal in -127..127)
        return ByteBuffer.allocate(7).order(ByteOrder.LITTLE_ENDIAN)
            .put(buttons.toByte()).putShort(dx.toShort()).putShort(dy.toShort())
            .put(vertical.toByte()).put(horizontal.toByte()).array()
    }

    fun release() { keys.clear(); modifiers = 0; buttons = 0 }

    companion object {
        private fun bytes(vararg value: Int) = ByteArray(value.size) { value[it].toByte() }
        // USB HID 1.11 descriptors: Mouse Application / Pointer Physical collection.
        val MOUSE_DESCRIPTOR = bytes(
            0x05,1, 0x09,2, 0xa1,1, 0x09,1, 0xa1,0,
            0x05,9, 0x19,1, 0x29,5, 0x15,0, 0x25,1, 0x95,5, 0x75,1, 0x81,2,
            0x95,1, 0x75,3, 0x81,1,
            0x05,1, 0x09,0x30, 0x09,0x31, 0x16,1,0x80, 0x26,0xff,0x7f,
            0x75,16, 0x95,2, 0x81,6,
            0x09,0x38, 0x15,0x81, 0x25,0x7f, 0x75,8, 0x95,1, 0x81,6,
            0x05,0x0c, 0x0a,0x38,2, 0x15,0x81, 0x25,0x7f, 0x75,8, 0x95,1, 0x81,6,
            0xc0,0xc0,
        )
        val KEYBOARD_DESCRIPTOR = bytes(
            0x05,1, 0x09,6, 0xa1,1, 0x05,7, 0x19,0xe0, 0x29,0xe7,
            0x15,0, 0x25,1, 0x75,1, 0x95,8, 0x81,2,
            0x75,8, 0x95,1, 0x81,1,
            0x05,8, 0x19,1, 0x29,5, 0x75,1, 0x95,5, 0x91,2,
            0x75,3, 0x95,1, 0x91,1,
            0x05,7, 0x19,0, 0x29,0x73, 0x15,0, 0x25,0x73,
            0x75,8, 0x95,6, 0x81,0, 0xc0,
        )

        fun androidUsage(code: Int): Int? = when (code) {
            in 29..54 -> code - 29 + 4
            in 8..16 -> code - 8 + 0x1e
            7 -> 0x27
            in 131..142 -> code - 131 + 0x3a
            in 144..153 -> if (code == 144) 0x62 else code - 145 + 0x59
            else -> mapOf(66 to 0x28, 23 to 0x28, 111 to 0x29, 67 to 0x2a,
                61 to 0x2b, 62 to 0x2c, 69 to 0x2d, 70 to 0x2e, 71 to 0x2f,
                72 to 0x30, 73 to 0x31, 74 to 0x33, 75 to 0x34, 68 to 0x35,
                55 to 0x36, 56 to 0x37, 76 to 0x38, 115 to 0x39, 120 to 0x46,
                116 to 0x47, 121 to 0x48, 124 to 0x49, 122 to 0x4a, 92 to 0x4b,
                112 to 0x4c, 123 to 0x4d, 93 to 0x4e, 22 to 0x4f, 21 to 0x50,
                20 to 0x51, 19 to 0x52, 143 to 0x53, 154 to 0x54, 155 to 0x55,
                156 to 0x56, 157 to 0x57, 160 to 0x58, 158 to 0x63,
                113 to 0xe0, 59 to 0xe1, 57 to 0xe2, 117 to 0xe3,
                114 to 0xe4, 60 to 0xe5, 58 to 0xe6, 118 to 0xe7)[code]
        }
    }
}
