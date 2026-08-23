package dev.voiceos.client

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.PackageManager
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.IBinder
import org.json.JSONObject
import java.io.BufferedOutputStream
import java.io.File
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.util.UUID

class RecordingService : Service() {
    @Volatile private var recording = false
    private var recorder: AudioRecord? = null
    private var worker: Thread? = null

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> startRecording()
            ACTION_STOP -> stopRecording()
        }
        return START_NOT_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        stopRecording()
        super.onDestroy()
    }

    private fun startRecording() {
        if (recording) return
        recording = true
        startForeground(NOTIFICATION_ID, notification("Listening…"))
        publish("Listening")
        VoiceWidgetProvider.updateStatus(this, "Listening")

        worker = Thread({ recordAndUpload() }, "voiceos-recorder").also { it.start() }
    }

    private fun stopRecording() {
        if (!recording) return
        recording = false
        try {
            recorder?.stop()
        } catch (_: IllegalStateException) {
            // The recorder may already be stopping on the worker thread.
        }
        publish("Processing")
        VoiceWidgetProvider.updateStatus(this, "Processing")
    }

    private fun recordAndUpload() {
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            publish("Error", response = "Microphone permission was revoked")
            VoiceWidgetProvider.updateStatus(this, "Permission required")
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return
        }

        val bufferSize = maxOf(
            AudioRecord.getMinBufferSize(SAMPLE_RATE, CHANNEL, ENCODING),
            SAMPLE_RATE * 2,
        )
        val audioFile = File.createTempFile("voice-turn-", ".pcm", cacheDir)

        try {
            val audioRecord = AudioRecord(
                MediaRecorder.AudioSource.VOICE_RECOGNITION,
                SAMPLE_RATE,
                CHANNEL,
                ENCODING,
                bufferSize,
            )
            recorder = audioRecord
            if (audioRecord.state != AudioRecord.STATE_INITIALIZED) {
                throw IllegalStateException("Android could not initialize the microphone")
            }

            BufferedOutputStream(FileOutputStream(audioFile)).use { output ->
                val buffer = ByteArray(bufferSize)
                audioRecord.startRecording()
                while (recording) {
                    val count = audioRecord.read(buffer, 0, buffer.size)
                    if (count > 0) output.write(buffer, 0, count)
                }
            }
            try {
                audioRecord.stop()
            } catch (_: IllegalStateException) {
                // stopRecording may already have stopped it.
            }
            audioRecord.release()
            recorder = null

            if (audioFile.length() == 0L) {
                throw IllegalStateException("No audio was captured")
            }

            val result = upload(audioFile)
            publish(
                state = "Ready",
                transcript = result.optString("transcript"),
                response = result.getString("response_text"),
            )
            VoiceWidgetProvider.updateStatus(this, "Ready")
        } catch (error: Exception) {
            publish("Error", response = error.message ?: "Voice request failed")
            VoiceWidgetProvider.updateStatus(this, "Error")
        } finally {
            recording = false
            recorder?.release()
            recorder = null
            audioFile.delete()
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
        }
    }

    private fun upload(audioFile: File): JSONObject {
        val connection = URL("${BuildConfig.GATEWAY_BASE_URL}/v1/turns/audio")
            .openConnection() as HttpURLConnection
        connection.requestMethod = "POST"
        connection.connectTimeout = 10_000
        connection.readTimeout = 90_000
        connection.doOutput = true
        connection.setFixedLengthStreamingMode(audioFile.length())
        connection.setRequestProperty("Content-Type", "application/octet-stream")
        connection.setRequestProperty("X-Audio-Format", "pcm_s16le;rate=16000;channels=1")
        connection.setRequestProperty("X-Session-Id", UUID.randomUUID().toString())

        audioFile.inputStream().use { input ->
            connection.outputStream.use { output -> input.copyTo(output) }
        }

        val status = connection.responseCode
        val body = (if (status in 200..299) connection.inputStream else connection.errorStream)
            .bufferedReader()
            .use { it.readText() }
        connection.disconnect()

        if (status !in 200..299) throw IllegalStateException("Gateway returned HTTP $status: $body")
        return JSONObject(body)
    }

    private fun publish(state: String, transcript: String? = null, response: String? = null) {
        sendBroadcast(Intent(ACTION_STATE).apply {
            setPackage(packageName)
            putExtra(EXTRA_STATE, state)
            putExtra(EXTRA_TRANSCRIPT, transcript)
            putExtra(EXTRA_RESPONSE, response)
        })
    }

    private fun notification(text: String): Notification {
        val openApp = PendingIntent.getActivity(
            this,
            200,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val stop = PendingIntent.getService(
            this,
            201,
            Intent(this, RecordingService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        return Notification.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_mic)
            .setContentTitle("Omarchy Voice")
            .setContentText(text)
            .setContentIntent(openApp)
            .setOngoing(true)
            .addAction(Notification.Action.Builder(null, "Done", stop).build())
            .build()
    }

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            "Voice sessions",
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = "Shows when Omarchy Voice is using the microphone"
        }
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    companion object {
        const val ACTION_START = "dev.voiceos.client.action.START_RECORDING"
        const val ACTION_STOP = "dev.voiceos.client.action.STOP_RECORDING"
        const val ACTION_STATE = "dev.voiceos.client.action.RECORDING_STATE"
        const val EXTRA_STATE = "state"
        const val EXTRA_TRANSCRIPT = "transcript"
        const val EXTRA_RESPONSE = "response"

        private const val CHANNEL_ID = "voice_sessions"
        private const val NOTIFICATION_ID = 10
        private const val SAMPLE_RATE = 16_000
        private const val CHANNEL = AudioFormat.CHANNEL_IN_MONO
        private const val ENCODING = AudioFormat.ENCODING_PCM_16BIT
    }
}
