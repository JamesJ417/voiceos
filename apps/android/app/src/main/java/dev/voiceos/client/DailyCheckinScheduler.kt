package dev.voiceos.client

import android.app.AlarmManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import java.time.ZonedDateTime

object DailyCheckinScheduler {
    private val reminderHours = intArrayOf(9, 13, 18)
    private const val ALARM_REQUEST = 1201

    fun schedule(context: Context) {
        val now = ZonedDateTime.now()
        val next = reminderHours
            .map { now.withHour(it).withMinute(0).withSecond(0).withNano(0) }
            .firstOrNull { it.isAfter(now) }
            ?: now.plusDays(1).withHour(reminderHours.first()).withMinute(0).withSecond(0).withNano(0)
        scheduleAt(context, next)
    }

    fun scheduleTomorrow(context: Context) {
        val next = ZonedDateTime.now().plusDays(1)
            .withHour(reminderHours.first()).withMinute(0).withSecond(0).withNano(0)
        scheduleAt(context, next)
    }

    private fun scheduleAt(context: Context, next: ZonedDateTime) {
        val intent = Intent(context, DailyCheckinReceiver::class.java)
        val pending = PendingIntent.getBroadcast(
            context,
            ALARM_REQUEST,
            intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        context.getSystemService(AlarmManager::class.java).setAndAllowWhileIdle(
            AlarmManager.RTC_WAKEUP,
            next.toInstant().toEpochMilli(),
            pending,
        )
    }
}

class DailyCheckinReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_ID,
                "Daily planning",
                NotificationManager.IMPORTANCE_DEFAULT,
            ).apply {
                description = "VIC daily planning questions"
            },
        )
        val open = PendingIntent.getActivity(
            context,
            1202,
            Intent(context, MainActivity::class.java).apply {
                action = MainActivity.ACTION_DAILY_CHECKIN
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
            },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val notification = android.app.Notification.Builder(context, CHANNEL_ID)
            .setSmallIcon(dev.voiceos.client.R.drawable.ic_mic)
            .setContentTitle("VIC daily planning")
            .setContentText("Continue today’s 12 questions and turn the answers into a plan.")
            .setContentIntent(open)
            .setAutoCancel(true)
            .build()
        manager.notify(NOTIFICATION_ID, notification)
        DailyCheckinScheduler.schedule(context)
    }

    companion object {
        private const val CHANNEL_ID = "voiceos-daily-planning"
        private const val NOTIFICATION_ID = 1203
    }
}

class DailyCheckinBootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        if (intent?.action == Intent.ACTION_BOOT_COMPLETED) {
            DailyCheckinScheduler.schedule(context)
        }
    }
}
