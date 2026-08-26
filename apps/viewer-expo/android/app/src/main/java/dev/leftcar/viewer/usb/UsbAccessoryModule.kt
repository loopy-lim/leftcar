package dev.leftcar.viewer.usb

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbAccessory
import android.hardware.usb.UsbManager
import android.app.PendingIntent
import android.os.Build
import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.LifecycleEventListener
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.modules.core.DeviceEventManagerModule
import dev.leftcar.viewer.shim.ViewerNative

/** Owns the Android UsbAccessory fd and exposes the native bridge to JS. */
class UsbAccessoryModule(reactContext: ReactApplicationContext) :
    ReactContextBaseJavaModule(reactContext), LifecycleEventListener {

    companion object {
        private const val USB_PERMISSION_ACTION = "dev.leftcar.viewer.USB_ACCESSORY_PERMISSION"
        private const val ACCESSORY_MANUFACTURER = "Leftcar"
        private const val ACCESSORY_MODEL = "LeftcarHost"

        @Volatile
        private var active: UsbAccessoryModule? = null

        fun handleIntent(intent: Intent) {
            active?.onAccessoryAttached(intent)
        }
    }

    private var descriptor: android.os.ParcelFileDescriptor? = null
    private var currentAccessory: UsbAccessory? = null
    private var attached = false
    private var permissionPending = false
    private val permissionReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action != USB_PERMISSION_ACTION) return
            permissionPending = false
            val accessory = accessoryFromIntent(intent) ?: return
            val granted = intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false)
            if (granted) {
                openAccessory(accessory)
            } else {
                emitState(false, 0, present = true)
            }
        }
    }
    private val detachReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action == UsbManager.ACTION_USB_ACCESSORY_DETACHED) {
                closeAccessory()
            }
        }
    }

    override fun getName(): String = "UsbAccessory"

    init {
        active = this
        val filter = IntentFilter().apply {
            addAction(UsbManager.ACTION_USB_ACCESSORY_DETACHED)
            addAction(USB_PERMISSION_ACTION)
        }
        if (android.os.Build.VERSION.SDK_INT >= 33) {
            reactApplicationContext.registerReceiver(permissionReceiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            @Suppress("DEPRECATION")
            reactApplicationContext.registerReceiver(permissionReceiver, filter)
        }
        val detachFilter = IntentFilter(UsbManager.ACTION_USB_ACCESSORY_DETACHED)
        if (android.os.Build.VERSION.SDK_INT >= 33) {
            reactApplicationContext.registerReceiver(
                detachReceiver,
                detachFilter,
                Context.RECEIVER_NOT_EXPORTED,
            )
        } else {
            @Suppress("DEPRECATION")
            reactApplicationContext.registerReceiver(detachReceiver, detachFilter)
        }
        reactApplicationContext.addLifecycleEventListener(this)
        reactApplicationContext.currentActivity?.intent?.let(::onAccessoryAttached)
        refreshAccessory()
    }

    fun onAccessoryAttached(intent: Intent) {
        val accessory = accessoryFromIntent(intent) ?: return
        openAccessory(accessory)
    }

    private fun accessoryFromIntent(intent: Intent): UsbAccessory? {
        return if (Build.VERSION.SDK_INT >= 33) {
            intent.getParcelableExtra(UsbManager.EXTRA_ACCESSORY, UsbAccessory::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableExtra(UsbManager.EXTRA_ACCESSORY)
        }
    }

    private fun isLeftcarAccessory(accessory: UsbAccessory): Boolean =
        accessory.manufacturer == ACCESSORY_MANUFACTURER &&
            accessory.model == ACCESSORY_MODEL

    /** Reconcile state instead of trusting only the one-shot attach Intent. */
    private fun refreshAccessory() {
        val manager = reactApplicationContext.getSystemService(Context.USB_SERVICE) as UsbManager
        val accessory = manager.accessoryList
            ?.firstOrNull(::isLeftcarAccessory)
        if (accessory == null) {
            closeAccessory()
            return
        }
        if (attached && descriptor != null) return
        openAccessory(accessory)
    }

    private fun requestAccessoryPermission(manager: UsbManager, accessory: UsbAccessory) {
        if (permissionPending) return
        permissionPending = true
        emitState(false, 0, present = true, pending = true)
        val permissionIntent = PendingIntent.getBroadcast(
            reactApplicationContext,
            0,
            Intent(USB_PERMISSION_ACTION).setPackage(reactApplicationContext.packageName),
            if (Build.VERSION.SDK_INT >= 23) {
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
            } else {
                PendingIntent.FLAG_UPDATE_CURRENT
            },
        )
        manager.requestPermission(accessory, permissionIntent)
    }

    private fun emitState(
        isAttached: Boolean,
        port: Int,
        present: Boolean = isAttached,
        pending: Boolean = permissionPending,
    ) {
        val body = Arguments.createMap().apply {
            putBoolean("attached", isAttached)
            putInt("controlPort", port)
            putBoolean("accessoryPresent", present)
            putBoolean("permissionPending", pending)
        }
        reactApplicationContext
            .getJSModule(DeviceEventManagerModule.RCTDeviceEventEmitter::class.java)
            .emit("leftcarUsbState", body)
    }

    private fun openAccessory(accessory: UsbAccessory): Boolean {
        val manager = reactApplicationContext.getSystemService(Context.USB_SERVICE) as UsbManager
        if (!isLeftcarAccessory(accessory)) return false
        if (!manager.hasPermission(accessory)) {
            requestAccessoryPermission(manager, accessory)
            return false
        }
        val opened = manager.openAccessory(accessory) ?: return false
        val code = ViewerNative.prepareUsb(opened.fd)
        if (code != 0) {
            opened.close()
            return false
        }
        descriptor?.close()
        descriptor = opened
        currentAccessory = accessory
        permissionPending = false
        attached = true
        emitState(true, ViewerNative.usbControlPort())
        return true
    }

    private fun closeAccessory() {
        descriptor?.close()
        descriptor = null
        currentAccessory = null
        permissionPending = false
        if (attached) {
            attached = false
            emitState(false, 0, present = false, pending = false)
        }
    }

    @ReactMethod
    fun getAccessoryState(promise: Promise) {
        val manager = reactApplicationContext.getSystemService(Context.USB_SERVICE) as UsbManager
        val accessoryPresent = manager.accessoryList?.any(::isLeftcarAccessory) == true
        refreshAccessory()
        val body = Arguments.createMap().apply {
            putBoolean("attached", attached && descriptor != null)
            putInt("controlPort", if (attached) ViewerNative.usbControlPort() else 0)
            putBoolean("accessoryPresent", accessoryPresent)
            putBoolean("permissionPending", permissionPending)
        }
        promise.resolve(body)
    }

    override fun invalidate() {
        closeAccessory()
        reactApplicationContext.removeLifecycleEventListener(this)
        runCatching { reactApplicationContext.unregisterReceiver(permissionReceiver) }
        runCatching { reactApplicationContext.unregisterReceiver(detachReceiver) }
        if (active === this) active = null
        super.invalidate()
    }

    override fun onHostResume() {
        refreshAccessory()
    }

    override fun onHostPause() = Unit

    override fun onHostDestroy() {
        closeAccessory()
    }
}
