package dev.voiceos.client

import android.Manifest
import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.MediaRecorder
import android.media.ToneGenerator
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import android.os.SystemClock
import android.util.Log
import kotlin.math.abs

/**
 * Always-on, fully local wake-word listener. It never owns the microphone at the same time as
 * [VICConversationService]: detection releases AudioRecord before Conversation Mode claims the floor.
 */
class VICWakeService : Service() {
    private val handler = Handler(Looper.getMainLooper())
    private val monitorLock = Any()
    private var engine: SherpaWakeWordEngine? = null
    private var audioRecord: AudioRecord? = null
    private var monitorThread: Thread? = null
    private var wakeLock: PowerManager.WakeLock? = null
    @Volatile private var monitoring = false
    @Volatile private var conversationActive = false
    private var lastActivationMillis = -WakeWordSettings.COOLDOWN_MILLIS
    private var awaitingConversation = false
    private var receiverRegistered = false

    private val conversationReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if (intent?.action != VICConversationService.ACTION_STATE) return
            val wasActive = conversationActive
            conversationActive = intent.getBooleanExtra(VICConversationService.EXTRA_ACTIVE, false)
            if (conversationActive) {
                awaitingConversation = false
                stopMonitoring()
                updateNotification("Conversation active — wake word paused")
            } else if (wasActive) {
                WakeSoundPlayer.deactivated(this@VICWakeService)
                scheduleMonitoring(WakeWordSettings.RESTART_DELAY_MILLIS)
            }
        }
    }

    @SuppressLint("UnspecifiedRegisterReceiverFlag")
    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        val filter = IntentFilter(VICConversationService.ACTION_STATE)
        if (Build.VERSION.SDK_INT >= 33) {
            registerReceiver(conversationReceiver, filter, RECEIVER_NOT_EXPORTED)
        } else {
            @Suppress("DEPRECATION")
            registerReceiver(conversationReceiver, filter)
        }
        receiverRegistered = true
        conversationActive = getSharedPreferences(VICConversationService.PREFERENCES, MODE_PRIVATE)
            .getBoolean(VICConversationService.SNAPSHOT_ACTIVE, false)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_DISABLE -> disableAndStop()
            ACTION_TALK_NOW -> {
                WakeWordSettings.setEnabled(this, true)
                startWakeForeground("Opening wake-word listener")
                activateConversation(manual = true)
            }
            else -> {
                if (!WakeWordSettings.isEnabled(this)) {
                    stopSelf()
                    return START_NOT_STICKY
                }
                startWakeForeground("Loading on-device “Hey VIC”")
                if (conversationActive) updateNotification("Conversation active — wake word paused")
                else scheduleMonitoring(0L)
            }
        }
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        monitoring = false
        runCatching { audioRecord?.stop() }
        audioRecord = null
        releaseWakeLock()
        handler.removeCallbacksAndMessages(null)
        if (receiverRegistered) unregisterReceiver(conversationReceiver)
        receiverRegistered = false
        val activeEngine = engine
        engine = null
        Thread({
            runCatching { monitorThread?.join(1_000L) }
            runCatching { activeEngine?.close() }
        }, "vic-wake-release").start()
        super.onDestroy()
    }

    private fun disableAndStop() {
        WakeWordSettings.setEnabled(this, false)
        stopMonitoring()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun scheduleMonitoring(delayMillis: Long) {
        handler.removeCallbacks(startMonitoringRunnable)
        handler.postDelayed(startMonitoringRunnable, delayMillis)
    }

    private val startMonitoringRunnable = Runnable { startMonitoring() }

    @SuppressLint("MissingPermission")
    private fun startMonitoring() {
        if (
            !WakeWordSettings.isEnabled(this) || conversationActive || awaitingConversation ||
            checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED
        ) {
            if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
                updateNotification("Microphone permission required — open VoiceOS")
            }
            return
        }
        synchronized(monitorLock) {
            if (monitoring || monitorThread?.isAlive == true) return
            monitoring = true
            acquireWakeLock()
            monitorThread = Thread({ monitorLoop() }, "vic-wake-monitor").apply { start() }
        }
    }

    @SuppressLint("MissingPermission")
    private fun monitorLoop() {
        var recorder: AudioRecord? = null
        try {
            val activeEngine = engine ?: SherpaWakeWordEngine(assets).also { engine = it }
            activeEngine.reset()
            val minimum = AudioRecord.getMinBufferSize(
                SherpaWakeWordEngine.SAMPLE_RATE,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT,
            )
            check(minimum > 0) { "Unsupported microphone format" }
            recorder = AudioRecord(
                MediaRecorder.AudioSource.VOICE_RECOGNITION,
                SherpaWakeWordEngine.SAMPLE_RATE,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT,
                minimum * 2,
            )
            check(recorder.state == AudioRecord.STATE_INITIALIZED) { "Microphone initialization failed" }
            audioRecord = recorder
            recorder.startRecording()
            handler.post { updateNotification("Listening locally for “Hey VIC”") }
            val buffer = ShortArray(1_600)
            var recentPeak = 0f
            var decayBuffers = 0
            while (monitoring && !conversationActive && WakeWordSettings.isEnabled(this)) {
                val count = recorder.read(buffer, 0, buffer.size)
                if (count <= 0) continue
                val samples = FloatArray(count)
                var peak = 0f
                for (index in 0 until count) {
                    val value = buffer[index] / 32768f
                    samples[index] = value
                    peak = maxOf(peak, abs(value))
                }
                recentPeak = maxOf(recentPeak, peak)
                decayBuffers += 1
                if (decayBuffers >= 20) {
                    recentPeak *= 0.55f
                    decayBuffers = 0
                }
                val keyword = activeEngine.accept(samples) ?: continue
                val now = SystemClock.elapsedRealtime()
                if (WakeWordPolicy.shouldActivate(
                        keyword,
                        recentPeak,
                        now,
                        lastActivationMillis,
                        conversationActive,
                    )
                ) {
                    lastActivationMillis = now
                    handler.post { activateConversation(manual = false) }
                    break
                }
                activeEngine.reset()
                recentPeak = 0f
            }
        } catch (error: Throwable) {
            Log.e(TAG, "Wake-word monitor failed", error)
            handler.post { updateNotification("Wake-word listener needs attention") }
        } finally {
            monitoring = false
            audioRecord = null
            releaseWakeLock()
            runCatching { recorder?.stop() }
            recorder?.release()
            synchronized(monitorLock) { monitorThread = null }
        }
    }

    private fun stopMonitoring() {
        monitoring = false
        runCatching { audioRecord?.stop() }
        audioRecord = null
        releaseWakeLock()
    }

    private fun acquireWakeLock() {
        if (wakeLock?.isHeld == true) return
        wakeLock = getSystemService(PowerManager::class.java)
            .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "$packageName:vic-wake-word")
            .apply {
                setReferenceCounted(false)
                acquire()
            }
    }

    private fun releaseWakeLock() {
        wakeLock?.takeIf { it.isHeld }?.release()
        wakeLock = null
    }

    private fun activateConversation(manual: Boolean) {
        if (conversationActive || awaitingConversation) return
        awaitingConversation = true
        stopMonitoring()
        updateNotification(if (manual) "Starting conversation" else "Hey VIC detected")
        WakeSoundPlayer.activated(this)
        handler.postDelayed({
            try {
                startForegroundService(
                    Intent(this, VICConversationService::class.java)
                        .setAction(VICConversationService.ACTION_START)
                )
            } catch (error: RuntimeException) {
                awaitingConversation = false
                updateNotification("Could not start Conversation Mode")
                scheduleMonitoring(WakeWordSettings.COOLDOWN_MILLIS)
            }
        }, MICROPHONE_HANDOFF_MILLIS)
        handler.postDelayed({
            if (awaitingConversation && !conversationActive) {
                awaitingConversation = false
                scheduleMonitoring(0L)
            }
        }, CONVERSATION_START_TIMEOUT_MILLIS)
    }

    private fun notification(text: String): Notification {
        val open = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val talk = PendingIntent.getService(
            this,
            1,
            Intent(this, VICWakeService::class.java).setAction(ACTION_TALK_NOW),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val disable = PendingIntent.getService(
            this,
            2,
            Intent(this, VICWakeService::class.java).setAction(ACTION_DISABLE),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        return Notification.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_mic)
            .setContentTitle("Hey VIC enabled")
            .setContentText(text)
            .setContentIntent(open)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setCategory(Notification.CATEGORY_SERVICE)
            .setVisibility(Notification.VISIBILITY_PRIVATE)
            .addAction(Notification.Action.Builder(null, "Talk now", talk).build())
            .addAction(Notification.Action.Builder(null, "Disable", disable).build())
            .build()
    }

    private fun startWakeForeground(text: String) {
        if (Build.VERSION.SDK_INT >= 29) {
            startForeground(NOTIFICATION_ID, notification(text), ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE)
        } else {
            startForeground(NOTIFICATION_ID, notification(text))
        }
    }

    private fun updateNotification(text: String) {
        getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification(text))
    }

    private fun createNotificationChannel() {
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL_ID, "Hey VIC wake word", NotificationManager.IMPORTANCE_LOW).apply {
                description = "Shows while VoiceOS listens locally for the Hey VIC wake phrase"
                setSound(null, null)
                lockscreenVisibility = Notification.VISIBILITY_PRIVATE
            }
        )
    }

    companion object {
        const val ACTION_ENABLE = "dev.voiceos.client.action.ENABLE_WAKE_WORD"
        const val ACTION_DISABLE = "dev.voiceos.client.action.DISABLE_WAKE_WORD"
        const val ACTION_TALK_NOW = "dev.voiceos.client.action.WAKE_TALK_NOW"
        private const val CHANNEL_ID = "vic_wake_word"
        private const val NOTIFICATION_ID = 13
        private const val MICROPHONE_HANDOFF_MILLIS = 450L
        private const val CONVERSATION_START_TIMEOUT_MILLIS = 8_000L
        private const val TAG = "VICWakeService"

        fun enable(context: Context) {
            WakeWordSettings.setEnabled(context, true)
            context.startForegroundService(Intent(context, VICWakeService::class.java).setAction(ACTION_ENABLE))
        }

        fun disable(context: Context) {
            WakeWordSettings.setEnabled(context, false)
            context.startService(Intent(context, VICWakeService::class.java).setAction(ACTION_DISABLE))
        }

        fun ensureStartedIfEnabled(context: Context) {
            if (WakeWordSettings.isEnabled(context)) enable(context)
        }
    }
}

object WakeSoundPlayer {
    fun activated(context: Context) = play(context, ToneGenerator.TONE_PROP_ACK, 120)
    fun deactivated(context: Context) = play(context, ToneGenerator.TONE_PROP_NACK, 110)

    private fun play(context: Context, tone: Int, durationMillis: Int) {
        val generator = ToneGenerator(AudioManager.STREAM_NOTIFICATION, 55)
        generator.startTone(tone, durationMillis)
        Handler(context.mainLooper).postDelayed({ generator.release() }, durationMillis + 80L)
    }
}
