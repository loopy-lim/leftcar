package dev.leftcar.viewer.stream

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import dev.leftcar.viewer.MainActivity
import dev.leftcar.viewer.R

/** Network ownership follows video windows, including windows hidden behind another app. */
internal object StreamConnectionOwners {
    // All callers are Activity/Service lifecycle callbacks on the main thread.
    // Opaque tokens avoid retaining Activities or confusing replacement incarnations.
    private val owners = mutableSetOf<Any>()
    val isEmpty: Boolean get() = owners.isEmpty()

    fun acquire(context: Context, owner: Any) {
        if (!owners.add(owner) || owners.size != 1) return
        try {
            context.startForegroundService(Intent(context, StreamConnectionService::class.java))
        } catch (error: RuntimeException) {
            owners.remove(owner)
            android.util.Log.e("LeftcarStream", "Unable to keep background connection active", error)
        }
    }

    fun release(context: Context, owner: Any) {
        if (owners.remove(owner) && owners.isEmpty()) {
            context.stopService(Intent(context, StreamConnectionService::class.java))
        }
    }
}

/** Keeps Android's background network policy from blocking authenticated viewer heartbeats. */
class StreamConnectionService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val channel = "remote_screen_connection"
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(channel, getString(R.string.stream_connection_channel),
                NotificationManager.IMPORTANCE_LOW),
        )
        val openApp = PendingIntent.getActivity(this, 0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val notification = Notification.Builder(this, channel)
            .setSmallIcon(R.drawable.ic_stream_connection)
            .setContentTitle(getString(R.string.stream_connection_title))
            .setContentText(getString(R.string.stream_connection_description))
            .setContentIntent(openApp)
            .setCategory(Notification.CATEGORY_SERVICE)
            .setOngoing(true)
            .build()
        // Complete the foreground-service handshake even if the final window
        // closed before this queued start reached the service.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(1440, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
        } else {
            startForeground(1440, notification)
        }
        if (StreamConnectionOwners.isEmpty) stopSelf()
        android.util.Log.i("LeftcarStream", "connection service started ownersEmpty=${StreamConnectionOwners.isEmpty}")
        // A process killed by the user must not resurrect a disconnected service.
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        android.util.Log.i("LeftcarStream", "connection service stopped")
        super.onDestroy()
    }
}
