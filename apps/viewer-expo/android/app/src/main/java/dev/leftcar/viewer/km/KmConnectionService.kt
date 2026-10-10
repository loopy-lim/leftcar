package dev.leftcar.viewer.km

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import dev.leftcar.viewer.MainActivity
import dev.leftcar.viewer.R

/** Input-only background lifetime. Never creates a video Activity or capture session. */
class KmConnectionService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val channel = "leftcar_input_sharing"
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(channel, getString(R.string.km_notification_title), NotificationManager.IMPORTANCE_LOW))
        val stop = PendingIntent.getService(this, 0, Intent(this, KmConnectionService::class.java).setAction(STOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val open = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val notification = Notification.Builder(this, channel)
            .setSmallIcon(R.drawable.ic_stream_connection).setContentTitle(getString(R.string.km_notification_title))
            .setContentText(getString(R.string.km_notification_body)).setContentIntent(open)
            .setOngoing(true).setCategory(Notification.CATEGORY_SERVICE)
            .addAction(Notification.Action.Builder(null, getString(R.string.km_notification_stop), stop).build()).build()
        startForeground(1441, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
        if (intent?.action == STOP) KmRuntime.stop()
        if (KmRuntime.status.state == "off") stopSelf()
        return START_NOT_STICKY
    }
    override fun onTaskRemoved(rootIntent: Intent?) { KmRuntime.stop("앱이 종료되어 입력 공유를 중지했습니다"); super.onTaskRemoved(rootIntent) }
    override fun onDestroy() { if (KmRuntime.status.state != "off") KmRuntime.stop("입력 백그라운드 서비스가 종료되었습니다"); super.onDestroy() }
    companion object { private const val STOP = "dev.leftcar.viewer.km.STOP" }
}
