package dev.leftcar.viewer.km

import android.view.InputDevice

/** Same predicate is used for preflight, hotplug invalidation and every physical key callback. */
internal object KmKeyboardInventory {
    fun conflicts(external: Boolean, virtual: Boolean, keyboard: Boolean, own: Boolean): Boolean =
        external && !virtual && keyboard && !own
    fun eligible(device: InputDevice): Boolean {
        val keys = device.keyboardType != InputDevice.KEYBOARD_TYPE_NONE ||
            listOf(InputDevice.SOURCE_KEYBOARD, InputDevice.SOURCE_DPAD, InputDevice.SOURCE_GAMEPAD, InputDevice.SOURCE_JOYSTICK)
                .any(device::supportsSource)
        val touch = device.supportsSource(InputDevice.SOURCE_TOUCHSCREEN) || device.supportsSource(InputDevice.SOURCE_STYLUS)
        return conflicts(device.isExternal, device.isVirtual, keys && !touch, device.name.startsWith("Leftcar KM "))
    }
    fun anyPhysical(): Boolean = InputDevice.getDeviceIds().any { InputDevice.getDevice(it)?.let(::eligible) == true }
    fun mustStop(active: Boolean, keyBridgeEnabled: Boolean, leaseAcknowledged: Boolean, physicalKeyboard: Boolean) =
        active && keyBridgeEnabled && !leaseAcknowledged && physicalKeyboard
}
