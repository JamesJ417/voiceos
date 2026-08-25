package dev.voiceos.client

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.media.AudioAttributes
import android.media.AudioManager
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import android.speech.tts.Voice
import org.json.JSONObject
import java.util.Locale
import java.util.UUID

/** Owns a user-started, finite conversational session across activity and screen lifecycle. */
class VICConversationService : Service(), TextToSpeech.OnInitListener {
    private val handler = Handler(Looper.getMainLooper())
    private var recognizer: SpeechRecognizer? = null
    private var textToSpeech: TextToSpeech? = null
    private var wakeLock: PowerManager.WakeLock? = null
    private var active = false
    private var paused = false
    private var speaking = false
    private var requestInFlight = false
    private var stopAfterSpeech = false
    private var pauseAfterSpeech = false
    private var ttsReady = false
    private var pendingSpeech: String? = null
    private var currentSpeech: String? = null
    private var interruptedSpeech: String? = null
    private var latestPartial: String? = null
    private var silentAttempts = 0
    private var generation = 0
    private var pendingApproval: ApprovalRequest? = null
    private var floorEvents: EventSubscription? = null
    private var floorCursor = 0L
    private var lastFloorUpdateMillis = 0L
    private val sessionId by lazy {
        val preferences = getSharedPreferences(PREFERENCES, MODE_PRIVATE)
        preferences.getString(SESSION_ID, null)?.takeIf(String::isNotBlank)
            ?: UUID.randomUUID().toString().also {
                preferences.edit().putString(SESSION_ID, it).apply()
            }
    }

    private val sessionTimeout = Runnable {
        if (active) speakThenStop("This conversation has reached its time limit. Say hello again whenever you are ready.")
    }

    private val resumeInterruptedSpeech = object : Runnable {
        override fun run() {
            if (!active || paused || stopAfterSpeech || pauseAfterSpeech) {
                interruptedSpeech = null
                return
            }
            val audioManager = getSystemService(AudioManager::class.java)
            if (audioManager.mode in setOf(
                    AudioManager.MODE_RINGTONE,
                    AudioManager.MODE_IN_CALL,
                    AudioManager.MODE_IN_COMMUNICATION,
                )
            ) {
                handler.postDelayed(this, PHONE_AUDIO_RECHECK_MILLIS)
                return
            }
            val text = interruptedSpeech ?: return
            interruptedSpeech = null
            publish(STATE_SPEAKING, response = text, detail = "VIC is resuming")
            updateNotification("VIC is resuming", paused = false)
            speakResponse(text)
        }
    }

    private val recognitionListener = object : RecognitionListener {
        override fun onReadyForSpeech(params: Bundle?) {
            publish(STATE_LISTENING, detail = "Listening")
        }

        override fun onBeginningOfSpeech() {
            silentAttempts = 0
            publish(STATE_LISTENING, detail = "I hear you")
        }

        override fun onRmsChanged(rmsdB: Float) = Unit
        override fun onBufferReceived(buffer: ByteArray?) = Unit

        override fun onEndOfSpeech() {
            if (active && !paused) publish(STATE_PROCESSING, detail = "Finishing transcript")
        }

        override fun onError(error: Int) {
            latestPartial = null
            if (!active || paused || speaking || requestInFlight) return
            when (error) {
                SpeechRecognizer.ERROR_NO_MATCH,
                SpeechRecognizer.ERROR_SPEECH_TIMEOUT -> handleSilence()
                SpeechRecognizer.ERROR_RECOGNIZER_BUSY -> {
                    resetRecognizer()
                    scheduleListening(500L)
                }
                SpeechRecognizer.ERROR_CLIENT -> scheduleListening(350L)
                else -> speakThenStop(recognitionErrorMessage(error))
            }
        }

        override fun onResults(results: Bundle?) {
            if (!active || paused) return
            val finalText = recognitionText(results)
            val text = moreCompleteTranscript(finalText, latestPartial)
            latestPartial = null
            if (text.isNullOrBlank()) {
                handleSilence()
                return
            }
            silentAttempts = 0
            handleRecognizedText(text)
        }

        override fun onPartialResults(partialResults: Bundle?) {
            val partial = recognitionText(partialResults) ?: return
            latestPartial = partial
            publish(STATE_LISTENING, transcript = partial, detail = "Listening")
        }

        override fun onEvent(eventType: Int, params: Bundle?) = Unit
    }

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        textToSpeech = TextToSpeech(this, this)
        bootstrapFloorEvents()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> stopSession("Conversation ended")
            ACTION_PAUSE -> pauseSession()
            ACTION_RESUME -> resumeSession()
            else -> startSession()
        }
        return START_NOT_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onInit(status: Int) {
        ttsReady = status == TextToSpeech.SUCCESS
        if (!ttsReady) {
            if (active) failAndStop("Android's speech voice is unavailable.")
            return
        }
        val engine = textToSpeech ?: return
        engine.setLanguage(Locale.US)
        engine.setAudioAttributes(
            AudioAttributes.Builder()
                .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                .setUsage(AudioAttributes.USAGE_ASSISTANT)
                .build(),
        )
        selectSavedVoice(engine)
        engine.setOnUtteranceProgressListener(object : UtteranceProgressListener() {
            override fun onStart(utteranceId: String?) = Unit

            override fun onDone(utteranceId: String?) {
                handler.post {
                    speaking = false
                    currentSpeech = null
                    interruptedSpeech = null
                    if (stopAfterSpeech) {
                        stopSession("Conversation ended")
                    } else if (pauseAfterSpeech) {
                        pauseAfterSpeech = false
                        pauseSession()
                    } else if (active && !paused) {
                        scheduleListening(250L)
                    }
                }
            }

            @Deprecated("Deprecated in Java")
            override fun onError(utteranceId: String?) {
                handler.post {
                    speaking = false
                    currentSpeech = null
                    if (stopAfterSpeech) stopSession("Conversation ended")
                    else if (active && !paused) scheduleListening(350L)
                }
            }

            override fun onStop(utteranceId: String?, interrupted: Boolean) {
                handler.post {
                    speaking = false
                    if (!interrupted || !active || paused || stopAfterSpeech || pauseAfterSpeech) {
                        currentSpeech = null
                        return@post
                    }
                    interruptedSpeech = currentSpeech
                    currentSpeech = null
                    if (interruptedSpeech.isNullOrBlank()) {
                        scheduleListening(350L)
                        return@post
                    }
                    publish(STATE_STARTING, detail = "VIC paused for phone audio")
                    updateNotification("Reply paused — VIC will resume", paused = false)
                    handler.removeCallbacks(resumeInterruptedSpeech)
                    handler.postDelayed(resumeInterruptedSpeech, INTERRUPTION_RESUME_DELAY_MILLIS)
                }
            }
        })
        pendingSpeech?.let {
            pendingSpeech = null
            speakResponse(it)
        }
    }

    override fun onDestroy() {
        generation += 1
        handler.removeCallbacksAndMessages(null)
        releaseRecognizer()
        textToSpeech?.stop()
        textToSpeech?.shutdown()
        textToSpeech = null
        releaseWakeLock()
        floorEvents?.close()
        floorEvents = null
        changeFloor("release", "idle")
        active = false
        persistSnapshot(STATE_STOPPED, false, null, null, null, 0L)
        super.onDestroy()
    }

    private fun startSession() {
        if (active) {
            if (paused) resumeSession() else scheduleListening(100L)
            return
        }
        startMicrophoneForeground(notification("Starting conversation", paused = false))
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            failAndStop("Microphone permission is required for Conversation Mode.")
            return
        }
        if (!SpeechRecognizer.isOnDeviceRecognitionAvailable(this)) {
            failAndStop("On-device speech recognition is unavailable. Install the offline English speech model.")
            return
        }
        if (DeviceCredentials.token(this).isNullOrBlank()) {
            failAndStop("VoiceOS device enrollment is required before starting a VIC conversation.")
            return
        }
        active = true
        paused = false
        stopAfterSpeech = false
        pauseAfterSpeech = false
        silentAttempts = 0
        acquireWakeLock()
        handler.removeCallbacks(sessionTimeout)
        handler.postDelayed(sessionTimeout, SESSION_MAX_MILLIS)
        changeFloor("claim", "listening") { result ->
            handler.post {
                if (!active) return@post
                result.fold(
                    onSuccess = {
                        publish(STATE_STARTING, detail = "Conversation Mode starting")
                        scheduleListening(150L)
                    },
                    onFailure = { failAndStop("I could not claim the VIC conversation channel.") },
                )
            }
        }
    }

    private fun pauseSession() {
        if (!active || paused) return
        generation += 1
        paused = true
        speaking = false
        requestInFlight = false
        recognizer?.cancel()
        textToSpeech?.stop()
        publish(STATE_PAUSED, detail = "Conversation paused")
        changeFloor("release", "idle")
        updateNotification("Conversation paused", paused = true)
    }

    private fun resumeSession() {
        if (!active) {
            startSession()
            return
        }
        paused = false
        stopAfterSpeech = false
        pauseAfterSpeech = false
        changeFloor("claim", "listening") { result ->
            handler.post {
                result.fold(
                    onSuccess = {
                        publish(STATE_STARTING, detail = "Resuming conversation")
                        updateNotification("Listening for you", paused = false)
                        scheduleListening(150L)
                    },
                    onFailure = { pauseSession() },
                )
            }
        }
    }

    private fun stopSession(detail: String) {
        if (!active && !speaking) {
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return
        }
        generation += 1
        active = false
        paused = false
        speaking = false
        requestInFlight = false
        stopAfterSpeech = false
        pendingSpeech = null
        currentSpeech = null
        interruptedSpeech = null
        handler.removeCallbacksAndMessages(null)
        releaseRecognizer()
        textToSpeech?.stop()
        releaseWakeLock()
        pendingApproval = null
        publish(STATE_STOPPED, detail = detail, activeOverride = false)
        changeFloor("release", "idle")
        VoiceWidgetProvider.updateStatus(this, "Ready")
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun beginListening() {
        if (!active || paused || speaking || requestInFlight) return
        if (recognizer == null) {
            recognizer = SpeechRecognizer.createOnDeviceSpeechRecognizer(this).apply {
                setRecognitionListener(recognitionListener)
            }
        }
        latestPartial = null
        val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
            putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
            putExtra(RecognizerIntent.EXTRA_LANGUAGE, Locale.US.toLanguageTag())
            putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
            putExtra(RecognizerIntent.EXTRA_PREFER_OFFLINE, true)
            putExtra(RecognizerIntent.EXTRA_MAX_RESULTS, 3)
            putExtra(RecognizerIntent.EXTRA_SPEECH_INPUT_MINIMUM_LENGTH_MILLIS, 700L)
            putExtra(RecognizerIntent.EXTRA_SPEECH_INPUT_COMPLETE_SILENCE_LENGTH_MILLIS, 1_250L)
            putExtra(RecognizerIntent.EXTRA_SPEECH_INPUT_POSSIBLY_COMPLETE_SILENCE_LENGTH_MILLIS, 800L)
        }
        publish(STATE_STARTING, detail = "Opening microphone")
        updateNotification("Listening for you", paused = false)
        try {
            recognizer?.startListening(intent)
        } catch (_: RuntimeException) {
            resetRecognizer()
            scheduleListening(500L)
        }
    }

    private fun scheduleListening(delayMillis: Long) {
        handler.postDelayed({ beginListening() }, delayMillis)
    }

    private fun handleSilence() {
        silentAttempts += 1
        if (silentAttempts >= MAX_SILENT_ATTEMPTS) {
            speakThenStop("I haven't heard anything, so I am ending this conversation.")
        } else {
            publish(STATE_STARTING, detail = "Still listening")
            scheduleListening(450L)
        }
    }

    private fun handleRecognizedText(text: String) {
        val normalized = ConversationCommands.normalize(text)
        when (ConversationCommands.action(text)) {
            ConversationCommands.Action.STOP -> {
                speakThenStop("Okay. Conversation ended.")
                return
            }
            ConversationCommands.Action.PAUSE -> {
                pauseAfterSpeech = true
                speakResponse("Conversation paused. Use the notification to resume.")
                return
            }
            null -> Unit
        }
        PlaybackSpeed.resolveCommand(text, currentSpeechRate())?.let { rate ->
            val resolved = PlaybackSpeed.clamp(rate)
            getSharedPreferences(PLAYBACK_PREFERENCES, MODE_PRIVATE)
                .edit().putFloat(SPEECH_RATE_KEY, resolved).apply()
            speakResponse("Voice playback speed is now ${PlaybackSpeed.label(resolved)}.")
            return
        }
        InterestCommands.followTopic(text)?.let { topic ->
            val interest = InterestStore.follow(this, topic)
            val response = "Following ${interest.topic}. I added it to your private feed."
            publish(
                STATE_SPEAKING,
                transcript = text,
                response = response,
                provider = "local-interest",
                detail = "Interest followed",
            )
            VoiceWidgetProvider.updateStatus(this, "Interest followed")
            speakResponse(response)
            return
        }
        if (handlePendingApproval(text)) return
        submitTurn(text)
    }

    private fun submitTurn(text: String) {
        val requestGeneration = ++generation
        requestInFlight = true
        publish(STATE_PROCESSING, transcript = text, detail = "VIC is thinking")
        updateNotification("VIC is thinking", paused = false)
        GatewayClient.submitText(
            GatewaySettings.baseUrl(this),
            sessionId,
            text,
            DeviceCredentials.token(this),
        ) { result ->
            handler.post {
                if (!active || requestGeneration != generation) return@post
                requestInFlight = false
                result.fold(
                    onSuccess = { turn ->
                        pendingApproval = turn.approval
                        publish(
                            STATE_SPEAKING,
                            transcript = turn.transcript,
                            response = turn.responseText,
                            provider = turn.provider,
                            processingMillis = turn.processingMs,
                            approval = turn.approval,
                            detail = "VIC is speaking",
                        )
                        VoiceWidgetProvider.refreshTasks(this)
                        speakResponse(turn.responseText)
                    },
                    onFailure = { error ->
                        speakThenStop(
                            "I couldn't reach VoiceOS. ${error.message.orEmpty()}".trim()
                        )
                    },
                )
            }
        }
    }

    private fun handlePendingApproval(text: String): Boolean {
        val approval = pendingApproval ?: return false
        val normalized = ConversationCommands.normalize(text)
        val approve = normalized in APPROVE_COMMANDS
        val deny = normalized in DENY_COMMANDS
        if (!approve && !deny) return false
        if (approval.tool == "rig.root_command") {
            speakResponse("Administrative actions require the unlocked on-screen approval card.")
            return true
        }
        val requestGeneration = ++generation
        requestInFlight = true
        publish(STATE_PROCESSING, transcript = text, detail = "Recording approval decision")
        GatewayClient.decideApproval(
            GatewaySettings.baseUrl(this),
            approval.requestId,
            approve,
            DeviceCredentials.token(this),
        ) { result ->
            handler.post {
                if (!active || requestGeneration != generation) return@post
                requestInFlight = false
                result.fold(
                    onSuccess = { decision ->
                        pendingApproval = null
                        val response = decision.responseText.ifBlank {
                            if (approve) "Approved." else "Rejected."
                        }
                        publish(STATE_SPEAKING, transcript = text, response = response, detail = "VIC is speaking")
                        speakResponse(response)
                    },
                    onFailure = { speakResponse("I could not record that approval decision.") },
                )
            }
        }
        return true
    }

    private fun speakThenStop(text: String) {
        stopAfterSpeech = true
        speakResponse(text)
    }

    private fun speakResponse(text: String) {
        if (!active) return
        handler.removeCallbacks(resumeInterruptedSpeech)
        recognizer?.cancel()
        speaking = true
        currentSpeech = text
        publish(STATE_SPEAKING, response = text, detail = "VIC is speaking")
        updateNotification("VIC is speaking", paused = false)
        if (!ttsReady) {
            pendingSpeech = text
            return
        }
        textToSpeech?.setSpeechRate(currentSpeechRate())
        if (textToSpeech?.speak(text, TextToSpeech.QUEUE_FLUSH, null, UTTERANCE_ID) == TextToSpeech.ERROR) {
            speaking = false
            currentSpeech = null
            if (stopAfterSpeech) stopSession("Conversation ended") else scheduleListening(350L)
        }
    }

    private fun publish(
        state: String,
        transcript: String? = null,
        response: String? = null,
        provider: String? = null,
        processingMillis: Long = 0L,
        approval: ApprovalRequest? = null,
        detail: String? = null,
        activeOverride: Boolean = active,
    ) {
        persistSnapshot(state, activeOverride, transcript, response, provider, processingMillis)
        sendBroadcast(Intent(ACTION_STATE).apply {
            setPackage(packageName)
            putExtra(EXTRA_STATE, state)
            putExtra(EXTRA_ACTIVE, activeOverride)
            putExtra(EXTRA_TRANSCRIPT, transcript)
            putExtra(EXTRA_RESPONSE, response)
            putExtra(EXTRA_PROVIDER, provider)
            putExtra(EXTRA_PROCESSING_MS, processingMillis)
            putExtra(EXTRA_DETAIL, detail)
            approval?.let {
                putExtra(EXTRA_APPROVAL_ID, it.requestId)
                putExtra(EXTRA_APPROVAL_TOOL, it.tool)
                putExtra(EXTRA_APPROVAL_EXPIRES, it.expiresAtUnix)
                putExtra(EXTRA_APPROVAL_ARGUMENTS, it.arguments.toString())
            }
        })
        VoiceWidgetProvider.updateStatus(this, when (state) {
            STATE_LISTENING, STATE_STARTING -> "Conversation listening"
            STATE_PROCESSING -> "VIC thinking"
            STATE_SPEAKING -> "VIC speaking"
            STATE_PAUSED -> "Conversation paused"
            STATE_ERROR -> "Conversation error"
            else -> "Ready"
        })
        val phase = when (state) {
            STATE_LISTENING, STATE_STARTING -> "listening"
            STATE_PROCESSING -> "processing"
            STATE_SPEAKING -> "speaking"
            else -> null
        }
        if (phase != null && activeOverride) {
            val now = android.os.SystemClock.elapsedRealtime()
            if (transcript.isNullOrBlank() || now - lastFloorUpdateMillis >= 750L) {
                lastFloorUpdateMillis = now
                changeFloor("update", phase, transcript, response)
            }
        }
    }

    private fun changeFloor(
        action: String,
        phase: String,
        transcript: String? = null,
        response: String? = null,
        callback: (Result<ConversationFloor>) -> Unit = {},
    ) {
        GatewayClient.changeConversationFloor(
            baseUrl = GatewaySettings.baseUrl(this),
            action = action,
            phase = phase,
            partialTranscript = transcript,
            responseText = response,
            deviceToken = DeviceCredentials.token(this),
            callback = callback,
        )
    }

    private fun bootstrapFloorEvents() {
        val token = DeviceCredentials.token(this) ?: return
        GatewayClient.getLatestEventCursor(
            GatewaySettings.baseUrl(this),
            token,
        ) { result ->
            handler.post {
                if (!active) return@post
                result.fold(
                    onSuccess = { cursor ->
                        floorCursor = cursor
                        startFloorEvents(token)
                    },
                    onFailure = {
                        handler.postDelayed({ bootstrapFloorEvents() }, FLOOR_RECONNECT_MILLIS)
                    },
                )
            }
        }
    }

    private fun startFloorEvents(token: String) {
        floorEvents?.close()
        floorEvents = GatewayClient.streamEvents(
            GatewaySettings.baseUrl(this),
            token,
            floorCursor,
            onEvent = { event ->
                floorCursor = event.id
                if (event.type != "conversation.floor.changed") return@streamEvents
                val value = event.payload.optJSONObject("floor") ?: return@streamEvents
                val next = GatewayClient.parseConversationFloor(value)
                val thisDevice = DeviceCredentials.deviceId(this)
                if (active && next.active && next.holderDeviceId != thisDevice) {
                    handler.post {
                        stopSession("Conversation continued on ${next.holderDisplayName ?: "another device"}")
                    }
                }
            },
            onClosed = {
                if (active) handler.postDelayed({ startFloorEvents(token) }, FLOOR_RECONNECT_MILLIS)
            },
        )
    }

    private fun persistSnapshot(
        state: String,
        isActive: Boolean,
        transcript: String?,
        response: String?,
        provider: String?,
        processingMillis: Long,
    ) {
        getSharedPreferences(PREFERENCES, MODE_PRIVATE).edit()
            .putBoolean(SNAPSHOT_ACTIVE, isActive)
            .putString(SNAPSHOT_STATE, state)
            .apply {
                if (transcript != null) putString(SNAPSHOT_TRANSCRIPT, transcript)
                if (response != null) putString(SNAPSHOT_RESPONSE, response)
                if (provider != null) putString(SNAPSHOT_PROVIDER, provider)
                if (processingMillis > 0) putLong(SNAPSHOT_PROCESSING_MS, processingMillis)
            }
            .apply()
    }

    private fun notification(text: String, paused: Boolean): Notification {
        val open = PendingIntent.getActivity(
            this,
            510,
            Intent(this, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP
            },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val pauseOrResume = PendingIntent.getService(
            this,
            511,
            Intent(this, VICConversationService::class.java)
                .setAction(if (paused) ACTION_RESUME else ACTION_PAUSE),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val stop = PendingIntent.getService(
            this,
            512,
            Intent(this, VICConversationService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val publicVersion = Notification.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_mic)
            .setContentTitle("VIC Conversation Mode")
            .setContentText("Voice session active")
            .build()
        return Notification.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_mic)
            .setContentTitle("VIC Conversation Mode")
            .setContentText(text)
            .setContentIntent(open)
            .setOngoing(true)
            .setCategory(Notification.CATEGORY_SERVICE)
            .setVisibility(Notification.VISIBILITY_PRIVATE)
            .setPublicVersion(publicVersion)
            .addAction(Notification.Action.Builder(null, if (paused) "Resume" else "Pause", pauseOrResume).build())
            .addAction(Notification.Action.Builder(null, "Stop", stop).build())
            .build()
    }

    private fun updateNotification(text: String, paused: Boolean) {
        getSystemService(NotificationManager::class.java)
            .notify(NOTIFICATION_ID, notification(text, paused))
    }

    private fun startMicrophoneForeground(notification: Notification) {
        if (android.os.Build.VERSION.SDK_INT >= 29) {
            startForeground(
                NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE,
            )
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    private fun createNotificationChannel() {
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(
                CHANNEL_ID,
                "VIC Conversation Mode",
                NotificationManager.IMPORTANCE_LOW,
            ).apply {
                description = "Shows when VIC is listening through the microphone"
                lockscreenVisibility = Notification.VISIBILITY_PRIVATE
            }
        )
    }

    private fun acquireWakeLock() {
        val manager = getSystemService(PowerManager::class.java)
        wakeLock = manager.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "$packageName:vic-conversation")
            .apply { acquire(SESSION_MAX_MILLIS + 30_000L) }
    }

    private fun releaseWakeLock() {
        wakeLock?.takeIf { it.isHeld }?.release()
        wakeLock = null
    }

    private fun resetRecognizer() {
        releaseRecognizer()
        recognizer = SpeechRecognizer.createOnDeviceSpeechRecognizer(this).apply {
            setRecognitionListener(recognitionListener)
        }
    }

    private fun releaseRecognizer() {
        recognizer?.cancel()
        recognizer?.destroy()
        recognizer = null
    }

    private fun selectSavedVoice(engine: TextToSpeech) {
        val saved = getSharedPreferences(PLAYBACK_PREFERENCES, MODE_PRIVATE)
            .getString(TTS_VOICE_KEY, null)
        val voices = engine.voices.orEmpty()
            .filter { it.locale.language.equals(Locale.US.language, ignoreCase = true) }
            .sortedWith(
                compareByDescending<Voice> { it.quality }
                    .thenByDescending { it.isNetworkConnectionRequired }
                    .thenBy { it.name },
            )
        engine.voice = voices.firstOrNull { it.name == saved } ?: voices.firstOrNull() ?: engine.voice
    }

    private fun currentSpeechRate(): Float = getSharedPreferences(PLAYBACK_PREFERENCES, MODE_PRIVATE)
        .getFloat(SPEECH_RATE_KEY, PlaybackSpeed.DEFAULT)
        .let(PlaybackSpeed::clamp)

    private fun failAndStop(message: String) {
        publish(STATE_ERROR, response = message, detail = message)
        stopSession(message)
    }

    private fun recognitionText(results: Bundle?): String? =
        results?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull()?.trim()

    private fun moreCompleteTranscript(finalText: String?, partialText: String?): String? {
        if (finalText.isNullOrBlank()) return partialText
        if (partialText.isNullOrBlank()) return finalText
        return if (partialText.split(Regex("\\s+")).size > finalText.split(Regex("\\s+")).size) {
            partialText
        } else {
            finalText
        }
    }

    private fun recognitionErrorMessage(error: Int): String = when (error) {
        SpeechRecognizer.ERROR_AUDIO -> "The microphone stopped working, so I ended the conversation."
        SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS -> "Microphone permission is required for Conversation Mode."
        SpeechRecognizer.ERROR_LANGUAGE_NOT_SUPPORTED,
        SpeechRecognizer.ERROR_LANGUAGE_UNAVAILABLE -> "The offline English speech model is unavailable."
        SpeechRecognizer.ERROR_NETWORK,
        SpeechRecognizer.ERROR_NETWORK_TIMEOUT -> "Offline speech recognition could not start."
        else -> "Speech recognition stopped unexpectedly, so I ended the conversation."
    }

    companion object {
        const val ACTION_START = "dev.voiceos.client.action.START_CONVERSATION"
        const val ACTION_STOP = "dev.voiceos.client.action.STOP_CONVERSATION"
        const val ACTION_PAUSE = "dev.voiceos.client.action.PAUSE_CONVERSATION"
        const val ACTION_RESUME = "dev.voiceos.client.action.RESUME_CONVERSATION"
        const val ACTION_STATE = "dev.voiceos.client.action.CONVERSATION_STATE"

        const val EXTRA_STATE = "state"
        const val EXTRA_ACTIVE = "active"
        const val EXTRA_TRANSCRIPT = "transcript"
        const val EXTRA_RESPONSE = "response"
        const val EXTRA_PROVIDER = "provider"
        const val EXTRA_PROCESSING_MS = "processing_ms"
        const val EXTRA_DETAIL = "detail"
        const val EXTRA_APPROVAL_ID = "approval_id"
        const val EXTRA_APPROVAL_TOOL = "approval_tool"
        const val EXTRA_APPROVAL_EXPIRES = "approval_expires"
        const val EXTRA_APPROVAL_ARGUMENTS = "approval_arguments"

        const val STATE_STARTING = "STARTING"
        const val STATE_LISTENING = "LISTENING"
        const val STATE_PROCESSING = "PROCESSING"
        const val STATE_SPEAKING = "SPEAKING"
        const val STATE_PAUSED = "PAUSED"
        const val STATE_STOPPED = "STOPPED"
        const val STATE_ERROR = "ERROR"

        const val PREFERENCES = "vic_conversation"
        const val SNAPSHOT_ACTIVE = "snapshot_active"
        const val SNAPSHOT_STATE = "snapshot_state"
        const val SNAPSHOT_TRANSCRIPT = "snapshot_transcript"
        const val SNAPSHOT_RESPONSE = "snapshot_response"
        const val SNAPSHOT_PROVIDER = "snapshot_provider"
        const val SNAPSHOT_PROCESSING_MS = "snapshot_processing_ms"

        private const val SESSION_ID = "session_id"
        private const val PLAYBACK_PREFERENCES = "voiceos_playback"
        private const val SPEECH_RATE_KEY = "speech_rate"
        private const val TTS_VOICE_KEY = "tts_voice_name"
        private const val CHANNEL_ID = "vic_conversation"
        private const val NOTIFICATION_ID = 12
        private const val UTTERANCE_ID = "vic-conversation-response"
        private const val SESSION_MAX_MILLIS = 30 * 60 * 1_000L
        private const val MAX_SILENT_ATTEMPTS = 2
        private const val INTERRUPTION_RESUME_DELAY_MILLIS = 900L
        private const val PHONE_AUDIO_RECHECK_MILLIS = 1_000L
        private const val FLOOR_RECONNECT_MILLIS = 2_000L

        private val APPROVE_COMMANDS = setOf("approve", "approved", "yes approve", "confirm")
        private val DENY_COMMANDS = setOf("deny", "denied", "no deny", "reject", "cancel")
    }
}
