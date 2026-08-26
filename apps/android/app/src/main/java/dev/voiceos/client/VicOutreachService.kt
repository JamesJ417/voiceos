package dev.voiceos.client

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.net.Uri
import android.os.Handler
import android.os.IBinder
import android.os.Looper

class VicOutreachService : Service() {
    private val handler = Handler(Looper.getMainLooper())
    private var transport: OutreachEventTransport? = null
    private var stopping = false
    private var reconnectAttempt = 0
    private val recoveryPoll = object : Runnable {
        override fun run() {
            syncPendingOutreach()
            if (!stopping) handler.postDelayed(this, RECOVERY_POLL_MILLIS)
        }
    }

    override fun onCreate() {
        super.onCreate()
        VicOutreachNotifications.ensureChannels(this)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            stopping = true
            transport?.stop()
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return START_NOT_STICKY
        }
        startForeground(CONNECTION_NOTIFICATION_ID, VicOutreachNotifications.connectionNotification(this))
        connect()
        handler.removeCallbacks(recoveryPoll)
        handler.post(recoveryPoll)
        return START_STICKY
    }

    override fun onDestroy() {
        stopping = true
        handler.removeCallbacksAndMessages(null)
        transport?.stop()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun connect() {
        if (stopping || transport != null || DeviceCredentials.token(this).isNullOrBlank()) return
        val preferences = getSharedPreferences(PREFERENCES, MODE_PRIVATE)
        transport = SseOutreachTransport(this).also { selected ->
            selected.start(
                preferences.getLong(CURSOR, 0),
                onConnected = { reconnectAttempt = 0 },
                onOutreach = { eventId, outreach ->
                    preferences.edit().putLong(CURSOR, eventId).apply()
                    deliver(outreach)
                },
                onClosed = {
                    transport = null
                    if (!stopping) {
                        reconnectAttempt += 1
                        handler.postDelayed(
                            { connect() },
                            GatewayTransportPolicy.reconnectDelayMillis(reconnectAttempt),
                        )
                    }
                },
            )
        }
    }

    private fun syncPendingOutreach() {
        val token = DeviceCredentials.token(this) ?: return
        GatewayClient.getPendingOutreach(GatewaySettings.baseUrl(this), token) { result ->
            result.getOrNull()?.forEach(::deliver)
        }
    }

    @Synchronized
    private fun deliver(outreach: VicOutreach) {
        val preferences = getSharedPreferences(PREFERENCES, MODE_PRIVATE)
        val shownKey = "shown:${outreach.id}"
        if (preferences.getBoolean(shownKey, false)) return
        preferences.edit().putBoolean(shownKey, true).apply()
        VicOutreachNotifications.show(this, outreach)
        GatewayClient.actOnOutreach(
            GatewaySettings.baseUrl(this), DeviceCredentials.token(this),
            outreach.id, "delivered",
        )
    }

    companion object {
        const val ACTION_START = "dev.voiceos.client.action.START_OUTREACH"
        const val ACTION_STOP = "dev.voiceos.client.action.STOP_OUTREACH"
        private const val PREFERENCES = "vic_outreach_events"
        private const val CURSOR = "cursor"
        private const val CONNECTION_NOTIFICATION_ID = 4_200
        private const val RECOVERY_POLL_MILLIS = 30_000L
    }
}

object VicOutreachNotifications {
    private const val CHANNEL_CONNECTION = "vic-outreach-connection-v1"
    private const val CHANNEL_QUIET = "vic-outreach-quiet-v1"
    private const val CHANNEL_CHECK_IN = "vic-outreach-check-in-v1"
    private const val CHANNEL_NEEDS_YOU = "vic-outreach-needs-you-v1"

    fun ensureChannels(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java)
        val sound = Uri.parse("android.resource://${context.packageName}/${R.raw.vic_checkin}")
        val audio = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_NOTIFICATION_EVENT)
            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
            .build()
        manager.createNotificationChannels(listOf(
            NotificationChannel(CHANNEL_CONNECTION, "VIC connection", NotificationManager.IMPORTANCE_LOW).apply {
                description = "Keeps VIC connected for private proactive check-ins"
                setSound(null, null)
                setShowBadge(false)
            },
            NotificationChannel(CHANNEL_QUIET, "VIC quiet updates", NotificationManager.IMPORTANCE_LOW).apply {
                description = "Non-interrupting VIC progress updates"
                setSound(null, null)
            },
            NotificationChannel(CHANNEL_CHECK_IN, "VIC check-ins", NotificationManager.IMPORTANCE_HIGH).apply {
                description = "VIC status updates and questions"
                setSound(sound, audio)
                enableVibration(true)
                vibrationPattern = longArrayOf(0, 90, 70, 150)
                lockscreenVisibility = Notification.VISIBILITY_PRIVATE
            },
            NotificationChannel(CHANNEL_NEEDS_YOU, "VIC needs you", NotificationManager.IMPORTANCE_HIGH).apply {
                description = "Blockers, approvals, and time-sensitive requests from VIC"
                setSound(sound, audio)
                enableVibration(true)
                vibrationPattern = longArrayOf(0, 180, 80, 180, 80, 260)
                lockscreenVisibility = Notification.VISIBILITY_PRIVATE
            },
        ))
    }

    fun connectionNotification(context: Context): Notification {
        val open = PendingIntent.getActivity(
            context, 4_201,
            Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        return Notification.Builder(context, CHANNEL_CONNECTION)
            .setSmallIcon(R.drawable.ic_mic)
            .setContentTitle("VIC check-ins connected")
            .setContentText("Private outreach is listening through VIC")
            .setContentIntent(open)
            .setOngoing(true)
            .setCategory(Notification.CATEGORY_SERVICE)
            .build()
    }

    fun show(context: Context, outreach: VicOutreach) {
        ensureChannels(context)
        val channel = when (outreach.priority) {
            "quiet" -> CHANNEL_QUIET
            "needs_you" -> CHANNEL_NEEDS_YOU
            else -> CHANNEL_CHECK_IN
        }
        val talk = actionIntent(context, outreach, "talk_now", MainActivity.ACTION_VIC_TALK, 1)
        val progress = actionIntent(context, outreach, "show_progress", MainActivity.ACTION_VIC_SHOW_PROGRESS, 2)
        val later = actionIntent(context, outreach, "later", ACTION_LATER, 3)
        val dismiss = actionIntent(context, outreach, "dismiss", ACTION_DISMISS, 4)
        val publicVersion = Notification.Builder(context, channel)
            .setSmallIcon(R.drawable.ic_mic)
            .setContentTitle("VIC wants to talk")
            .setContentText("Open VIC to see the private update")
            .build()
        val notification = Notification.Builder(context, channel)
            .setSmallIcon(R.drawable.ic_mic)
            .setContentTitle(outreach.title)
            .setContentText(outreach.body)
            .setStyle(Notification.BigTextStyle().bigText(outreach.body).setSummaryText(outreach.reason))
            .setContentIntent(talk)
            .setDeleteIntent(dismiss)
            .setAutoCancel(true)
            .setCategory(Notification.CATEGORY_MESSAGE)
            .setVisibility(Notification.VISIBILITY_PRIVATE)
            .setPublicVersion(publicVersion)
            .addAction(Notification.Action.Builder(null, "Talk now", talk).build())
            .addAction(Notification.Action.Builder(null, "Show progress", progress).build())
            .addAction(Notification.Action.Builder(null, "Later", later).build())
            .build()
        context.getSystemService(NotificationManager::class.java)
            .notify(outreach.id.hashCode(), notification)
    }

    private fun actionIntent(
        context: Context,
        outreach: VicOutreach,
        serverAction: String,
        receiverAction: String,
        offset: Int,
    ): PendingIntent = PendingIntent.getBroadcast(
        context,
        outreach.id.hashCode() + offset,
        Intent(context, VicOutreachActionReceiver::class.java).apply {
            action = receiverAction
            putExtra(EXTRA_OUTREACH_ID, outreach.id)
            putExtra(EXTRA_TASK_ID, outreach.taskId)
            putExtra(EXTRA_BODY, outreach.body)
            putExtra(EXTRA_SERVER_ACTION, serverAction)
            putExtra(EXTRA_NOTIFICATION_ID, outreach.id.hashCode())
        },
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

    const val ACTION_LATER = "dev.voiceos.client.action.OUTREACH_LATER"
    const val ACTION_DISMISS = "dev.voiceos.client.action.OUTREACH_DISMISS"
    const val EXTRA_OUTREACH_ID = "outreach_id"
    const val EXTRA_TASK_ID = "task_id"
    const val EXTRA_BODY = "outreach_body"
    const val EXTRA_SERVER_ACTION = "outreach_action"
    const val EXTRA_NOTIFICATION_ID = "notification_id"
}

class VicOutreachActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val outreachId = intent.getStringExtra(VicOutreachNotifications.EXTRA_OUTREACH_ID) ?: return
        val serverAction = intent.getStringExtra(VicOutreachNotifications.EXTRA_SERVER_ACTION) ?: return
        val pending = goAsync()
        GatewayClient.actOnOutreach(
            GatewaySettings.baseUrl(context), DeviceCredentials.token(context), outreachId,
            serverAction, if (serverAction == "later") 30 else null,
        ) { pending.finish() }
        context.getSystemService(NotificationManager::class.java)
            .cancel(intent.getIntExtra(VicOutreachNotifications.EXTRA_NOTIFICATION_ID, outreachId.hashCode()))
        if (intent.action == MainActivity.ACTION_VIC_TALK || intent.action == MainActivity.ACTION_VIC_SHOW_PROGRESS) {
            context.startActivity(Intent(context, MainActivity::class.java).apply {
                action = intent.action
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
                putExtra(VicOutreachNotifications.EXTRA_OUTREACH_ID, outreachId)
                putExtra(VicOutreachNotifications.EXTRA_TASK_ID, intent.getStringExtra(VicOutreachNotifications.EXTRA_TASK_ID))
                putExtra(VicOutreachNotifications.EXTRA_BODY, intent.getStringExtra(VicOutreachNotifications.EXTRA_BODY))
            })
        }
    }
}
