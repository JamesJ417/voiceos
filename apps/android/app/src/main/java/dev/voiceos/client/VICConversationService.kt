package dev.voiceos.client

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.net.Uri
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.media.AudioAttributes
import android.media.AudioManager
import android.media.session.MediaSession
import android.media.ToneGenerator
import android.view.KeyEvent
import android.net.ConnectivityManager
import android.net.Network
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import android.os.SystemClock
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import android.speech.tts.Voice
import android.util.Log
import org.json.JSONObject
import java.util.Locale
import java.util.UUID

/** Owns a user-started, finite conversational session across activity and screen lifecycle. */
class VICConversationService : Service(), TextToSpeech.OnInitListener {
    private val handler = Handler(Looper.getMainLooper())
    private var recognizer: SpeechRecognizer? = null
    private var textToSpeech: TextToSpeech? = null
    private var wakeLock: PowerManager.WakeLock? = null
    private var mediaSession: MediaSession? = null
    private val controller = ConversationController()
    private val active: Boolean get() = controller.active
    private val paused: Boolean get() = controller.paused
    private val speaking: Boolean get() = controller.speaking
    private val requestInFlight: Boolean get() = controller.requestInFlight
    private var stopAfterSpeechReason: ConversationStopReason? = null
    private var pauseAfterSpeechDetail: String? = null
    private val ttsTerminalCompletionGate = TtsTerminalCompletionGate()
    private var ttsReady = false
    private var pendingSpeech: String? = null
    private var currentSpeech: String? = null
    private var currentUtteranceId: String? = null
    private var toneGenerator: ToneGenerator? = null
    private val resumableResponse = ResumableResponse()
    private var latestPartial: String? = null
    private val silencePolicy = ConversationSilencePolicy(
        inactivityTimeoutMillis = CONVERSATION_IDLE_TIMEOUT_MILLIS,
        clock = SystemClock::elapsedRealtime,
    )
    private var generation = 0
    private var pendingTurn: PendingConversationTurn? = null
    private var recognizerRecoveryAttempt = 0
    private var resettingRecognizer = false
    private var recognitionInProgress = false
    private var recognitionFinalizationRequested = false
    private var recognitionFinalizationReason: String? = null
    private var recognitionResultAccepted = false
    private var recognitionSpeechDetected = false
    private var recognitionBackend = RecognitionBackend.ON_DEVICE
    private val backgroundConversationUpdates = BackgroundConversationUpdateQueue()
    private var pendingApproval: ApprovalRequest? = null
    private var floorEvents: EventSubscription? = null
    private var floorCursor = 0L
    private var floorReconnectAttempt = 0
    @Volatile private var floorLeaseId: String? = null
    @Volatile private var floorRevision = 0L
    private var lastFloorUpdateMillis = 0L
    private val sessionId by lazy {
        val preferences = getSharedPreferences(PREFERENCES, MODE_PRIVATE)
        preferences.getString(SESSION_ID, null)?.takeIf(String::isNotBlank)
            ?: UUID.randomUUID().toString().also {
                preferences.edit().putString(SESSION_ID, it).apply()
            }
    }

    private val sessionTimeout = Runnable {
        if (active && !paused) speakThenPause(
            "This conversation has reached its time limit, so I paused it. Resume whenever you are ready.",
            "Conversation paused at its time limit",
        )
    }

    private val listenRunnable = Runnable { beginListening() }
    private val turnRetryRunnable = Runnable { submitPendingTurn() }
    private val turnWatchdog = Runnable {
        if (active && !paused && requestInFlight && pendingTurn != null) {
            Log.e(TAG, "event=gateway_turn_watchdog_timeout")
            handleTurnFailure(pendingTurn!!, java.io.IOException("Gateway turn timed out"))
        }
    }
    private val recognitionQuietTimeout = Runnable {
        requestRecognitionFinalization(RecognitionWatchdogPolicy.PARTIAL_QUIET_REASON)
    }
    private val recognitionHardTimeout = Runnable {
        requestRecognitionFinalization(RecognitionWatchdogPolicy.HARD_LIMIT_REASON)
    }
    private val recognitionResultFallback = Runnable {
        if (
            !recognitionFinalizationRequested || recognitionResultAccepted ||
            !active || paused || speaking || requestInFlight
        ) {
            clearRecognitionWatchdog()
            return@Runnable
        }
        val finalizationReason = recognitionFinalizationReason
        val partial = latestPartial
        latestPartial = null
        recognitionResultAccepted = true
        clearRecognitionWatchdog()
        if (
            recognitionBackend == RecognitionBackend.ON_DEVICE &&
            RecognitionWatchdogPolicy.hardLimitPartialNeedsPlatformRetry(
                finalizationReason,
                partial,
            )
        ) {
            recognitionResultAccepted = false
            switchToPlatformRecognizer("hard_limit_fragment")
            recoverRecognizer(SpeechRecognizer.ERROR_CLIENT)
            return@Runnable
        }
        if (partial.isNullOrBlank()) {
            Log.w(TAG, "event=recognizer_final_result_missing")
            recoverRecognizer(SpeechRecognizer.ERROR_CLIENT)
        } else {
            resetRecognizer()
            controller.dispatch(ConversationEvent.SpeechDetected)
            recognizerRecoveryAttempt = 0
            silencePolicy.markActivity()
            Log.w(TAG, "event=recognizer_partial_fallback chars=${partial.length}")
            handleRecognizedText(partial)
        }
    }

    private val networkCallback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            Log.i(TAG, "event=network_available")
            handler.post {
                if (active && !paused && pendingTurn != null && !requestInFlight) {
                    handler.removeCallbacks(turnRetryRunnable)
                    submitPendingTurn()
                }
            }
        }

        override fun onLost(network: Network) {
            Log.w(TAG, "event=network_lost")
        }
    }

    private val resumeInterruptedSpeech = object : Runnable {
        override fun run() {
            if (!active || paused || stopAfterSpeechReason != null || pauseAfterSpeechDetail != null) {
                resumableResponse.clear()
                clearResumableResponse()
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
            val text = resumableResponse.peekForResume() ?: return
            publish(STATE_SPEAKING, response = text, detail = "VIC is resuming")
            updateNotification("VIC is resuming", paused = false)
            speakResponse(text)
        }
    }

    private val recognitionListener = object : RecognitionListener {
        override fun onReadyForSpeech(params: Bundle?) {
            controller.dispatch(ConversationEvent.ListenerReady)
            recognitionInProgress = true
            handler.removeCallbacks(recognitionHardTimeout)
            handler.postDelayed(
                recognitionHardTimeout,
                RecognitionWatchdogPolicy.RECOGNIZER_HARD_LIMIT_MILLIS,
            )
            Log.i(TAG, "event=recognizer_ready")
            publish(STATE_LISTENING, detail = "Listening")
        }

        override fun onBeginningOfSpeech() {
            controller.dispatch(ConversationEvent.SpeechDetected)
            recognitionSpeechDetected = true
            recognizerRecoveryAttempt = 0
            silencePolicy.markActivity()
            Log.i(TAG, "event=speech_started")
            publish(STATE_LISTENING, detail = "I hear you")
        }

        override fun onRmsChanged(rmsdB: Float) = Unit
        override fun onBufferReceived(buffer: ByteArray?) = Unit

        override fun onEndOfSpeech() {
            Log.i(TAG, "event=speech_ended")
            handler.removeCallbacks(recognitionQuietTimeout)
            handler.removeCallbacks(recognitionHardTimeout)
            recognitionFinalizationRequested = true
            recognitionFinalizationReason = RecognitionWatchdogPolicy.END_OF_SPEECH_REASON
            handler.removeCallbacks(recognitionResultFallback)
            handler.postDelayed(
                recognitionResultFallback,
                RecognitionWatchdogPolicy.FINAL_RESULT_GRACE_MILLIS,
            )
            if (active && !paused) publish(STATE_PROCESSING, detail = "Finishing transcript")
        }

        override fun onError(error: Int) {
            val stalledAfterSpeech = recognitionFinalizationRequested && recognitionSpeechDetected
            val finalizationReason = recognitionFinalizationReason
            val finalizationPartial = latestPartial?.takeIf {
                recognitionFinalizationRequested && it.isNotBlank()
            }
            clearRecognitionWatchdog()
            latestPartial = null
            if (resettingRecognizer || !active || paused || speaking || requestInFlight) return
            Log.w(TAG, "event=recognizer_error code=$error idle_ms=${silencePolicy.idleDurationMillis()}")
            if (
                stalledAfterSpeech && recognitionBackend == RecognitionBackend.ON_DEVICE &&
                error in setOf(
                    SpeechRecognizer.ERROR_NO_MATCH,
                    SpeechRecognizer.ERROR_SPEECH_TIMEOUT,
                    SpeechRecognizer.ERROR_CLIENT,
                ) &&
                RecognitionWatchdogPolicy.hardLimitPartialNeedsPlatformRetry(
                    finalizationReason,
                    finalizationPartial,
                )
            ) {
                switchToPlatformRecognizer("hard_limit_fragment")
                recoverRecognizer(error)
                return
            }
            if (finalizationPartial != null) {
                recognitionResultAccepted = true
                resetRecognizer()
                controller.dispatch(ConversationEvent.SpeechDetected)
                recognizerRecoveryAttempt = 0
                silencePolicy.markActivity()
                Log.w(
                    TAG,
                    "event=recognizer_partial_fallback chars=${finalizationPartial.length} source=error",
                )
                handleRecognizedText(finalizationPartial)
                return
            }
            if (
                stalledAfterSpeech && recognitionBackend == RecognitionBackend.ON_DEVICE &&
                error in setOf(
                    SpeechRecognizer.ERROR_NO_MATCH,
                    SpeechRecognizer.ERROR_SPEECH_TIMEOUT,
                    SpeechRecognizer.ERROR_CLIENT,
                )
            ) {
                switchToPlatformRecognizer("stalled_after_speech")
                recoverRecognizer(error)
                return
            }
            when (error) {
                SpeechRecognizer.ERROR_NO_MATCH,
                SpeechRecognizer.ERROR_SPEECH_TIMEOUT -> handleSilence()
                SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS,
                SpeechRecognizer.ERROR_LANGUAGE_NOT_SUPPORTED,
                SpeechRecognizer.ERROR_LANGUAGE_UNAVAILABLE ->
                    pauseForConfigurationError(recognitionErrorMessage(error))

                else -> recoverRecognizer(error)
            }
        }

        override fun onResults(results: Bundle?) {
            if (!active || paused || recognitionResultAccepted) return
            recognitionResultAccepted = true
            clearRecognitionWatchdog()
            val finalText = recognitionText(results)
            val text = moreCompleteTranscript(finalText, latestPartial)
            latestPartial = null
            if (text.isNullOrBlank()) {
                handleSilence()
                return
            }
            controller.dispatch(ConversationEvent.SpeechDetected)
            recognizerRecoveryAttempt = 0
            silencePolicy.markActivity()
            Log.i(TAG, "event=recognizer_result")
            handleRecognizedText(text)
        }

        override fun onPartialResults(partialResults: Bundle?) {
            if (!active || paused || recognitionResultAccepted) return
            val partial = recognitionText(partialResults) ?: return
            silencePolicy.markActivity()
            latestPartial = partial
            if (recognitionInProgress && !recognitionFinalizationRequested) {
                handler.removeCallbacks(recognitionQuietTimeout)
                handler.postDelayed(
                    recognitionQuietTimeout,
                    RecognitionWatchdogPolicy.partialResultQuietMillis(partial),
                )
            }
            publish(STATE_LISTENING, transcript = partial, detail = "Listening")
        }

        override fun onEvent(eventType: Int, params: Bundle?) = Unit
    }

    override fun onCreate() {
        super.onCreate()
        recognitionBackend = RecognitionBackend.fromPersisted(
            getSharedPreferences(PREFERENCES, MODE_PRIVATE)
                .getString(RECOGNITION_BACKEND, null),
        )
        createNotificationChannel()
        mediaSession = MediaSession(this, "VICConversation").apply {
            setFlags(MediaSession.FLAG_HANDLES_MEDIA_BUTTONS)
            setCallback(object : MediaSession.Callback() {
                override fun onMediaButtonEvent(mediaButtonIntent: Intent): Boolean {
                    val event = mediaButtonIntent.getParcelableExtra<KeyEvent>(Intent.EXTRA_KEY_EVENT)
                    if (event?.action != KeyEvent.ACTION_DOWN || event.repeatCount != 0) return true
                    if (event.keyCode in setOf(
                            KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE,
                            KeyEvent.KEYCODE_HEADSETHOOK,
                            KeyEvent.KEYCODE_MEDIA_PLAY,
                            KeyEvent.KEYCODE_MEDIA_PAUSE,
                        )
                    ) {
                        handler.post {
                            if (active) {
                                if (paused) resumeSession()
                                else pauseSession("Conversation paused from headphone button")
                            }
                        }
                        return true
                    }
                    return super.onMediaButtonEvent(mediaButtonIntent)
                }
            })
        }
        textToSpeech = TextToSpeech(this, this)
        runCatching {
            getSystemService(ConnectivityManager::class.java)
                .registerDefaultNetworkCallback(networkCallback)
        }.onFailure { Log.w(TAG, "event=network_callback_unavailable", it) }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> stopSession(
                ConversationStopReason.fromWire(intent.getStringExtra(EXTRA_STOP_REASON)),
                "Conversation ended",
            )
            ACTION_PAUSE -> pauseSession("Conversation paused")
            ACTION_RESUME -> resumeSession()
            else -> startSession()
        }
        return START_REDELIVER_INTENT
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onInit(status: Int) {
        ttsReady = status == TextToSpeech.SUCCESS
        if (!ttsReady) {
            if (active) pauseForConfigurationError("Android's speech voice is unavailable.")
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
                    if (utteranceId != currentUtteranceId) return@post
                    resumableResponse.markReplayComplete()
                    clearResumableResponse()
                    if (!ttsTerminalCompletionGate.tryComplete()) return@post
                    Log.i(TAG, "event=tts_done")
                    finishSpeech(LISTEN_AFTER_TTS_DELAY_MILLIS)
                }
            }

            @Deprecated("Deprecated in Java")
            override fun onError(utteranceId: String?) {
                handler.post {
                    if (utteranceId != currentUtteranceId) return@post
                    if (!ttsTerminalCompletionGate.tryComplete()) return@post
                    Log.w(TAG, "event=tts_error")
                    finishSpeech(350L)
                }
            }

            override fun onStop(utteranceId: String?, interrupted: Boolean) {
                handler.post {
                    if (utteranceId != currentUtteranceId) return@post
                    if (
                        !interrupted || !active || paused ||
                        stopAfterSpeechReason != null || pauseAfterSpeechDetail != null
                    ) {
                        currentSpeech = null
                        return@post
                    }
                    captureCurrentResponseForResume()
                    currentSpeech = null
                    if (resumableResponse.pending.isNullOrBlank()) {
                        silencePolicy.markActivity()
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
        pendingSpeech?.takeIf { active && !paused }?.let {
            pendingSpeech = null
            speakResponse(it)
        }
    }

    override fun onDestroy() {
        val interruptedWhileActive = active
        captureCurrentResponseForResume()
        ttsTerminalCompletionGate.reset()
        generation += 1
        handler.removeCallbacksAndMessages(null)
        releaseRecognizer()
        textToSpeech?.stop()
        textToSpeech?.shutdown()
        textToSpeech = null
        toneGenerator?.release()
        toneGenerator = null
        mediaSession?.isActive = false
        mediaSession?.release()
        mediaSession = null
        releaseWakeLock()
        floorEvents?.close()
        floorEvents = null
        changeFloor("release", "idle")
        runCatching {
            getSystemService(ConnectivityManager::class.java)
                .unregisterNetworkCallback(networkCallback)
        }
        if (interruptedWhileActive) {
            controller.dispatch(ConversationEvent.Pause)
            Log.w(TAG, "event=session_interrupted reason=${ConversationStopReason.SERVICE_DESTROYED.wireValue}")
            persistSnapshot(
                ConversationSnapshot(
                    state = STATE_PAUSED,
                    active = true,
                    stopReason = ConversationStopReason.SERVICE_DESTROYED.wireValue,
                ),
            )
        } else {
            persistSnapshot(
                ConversationSnapshot(
                    state = STATE_STOPPED,
                    active = false,
                    stopReason = controller.lastStopReason?.wireValue,
                ),
            )
        }
        super.onDestroy()
    }

    private fun finishSpeech(listenDelayMillis: Long) {
        currentSpeech = null
        val stopReason = stopAfterSpeechReason
        val pauseDetail = pauseAfterSpeechDetail
        stopAfterSpeechReason = null
        pauseAfterSpeechDetail = null
        if (stopReason != null) {
            stopSession(stopReason, "Conversation ended")
        } else if (pauseDetail != null) {
            pauseSession(pauseDetail)
        } else if (active && !paused) {
            controller.dispatch(ConversationEvent.ResponseFinished)
            silencePolicy.markActivity()
            scheduleListening(listenDelayMillis)
        }
    }

    private fun startSession() {
        if (active) {
            if (paused) {
                resumeSession()
            } else {
                silencePolicy.markActivity()
                scheduleListening(100L)
            }
            return
        }
        startMicrophoneForeground(notification("Starting conversation", paused = false))
        mediaSession?.isActive = true
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            pauseForConfigurationError("Microphone permission is required for Conversation Mode.")
            return
        }
        if (!SpeechRecognizer.isOnDeviceRecognitionAvailable(this)) {
            pauseForConfigurationError(
                "On-device speech recognition is unavailable. Install the offline English speech model.",
            )
            return
        }
        if (DeviceCredentials.token(this).isNullOrBlank()) {
            pauseForConfigurationError("VoiceOS device enrollment is required before starting a VIC conversation.")
            return
        }
        controller.dispatch(ConversationEvent.Start)
        stopAfterSpeechReason = null
        pauseAfterSpeechDetail = null
        pendingTurn = restorePendingTurn()
        silencePolicy.markActivity()
        Log.i(TAG, "event=session_started idle_timeout_ms=$CONVERSATION_IDLE_TIMEOUT_MILLIS")
        bootstrapFloorEvents()
        acquireWakeLock()
        handler.removeCallbacks(sessionTimeout)
        handler.postDelayed(sessionTimeout, SESSION_MAX_MILLIS)
        changeFloor("claim", "listening") { result ->
            handler.post {
                if (!active) return@post
                result.fold(
                    onSuccess = {
                        publish(STATE_STARTING, detail = "Conversation Mode starting")
                        if (pendingTurn != null) submitPendingTurn() else scheduleListening(150L)
                    },
                    onFailure = { scheduleFloorClaimRetry("Connecting conversation channel") },
                )
            }
        }
    }

    private fun pauseSession(detail: String = "Conversation paused", releaseFloor: Boolean = true) {
        if (!active || paused) return
        Log.i(TAG, "event=session_paused detail=${detail.replace(' ', '_')}")
        captureCurrentResponseForResume()
        generation += 1
        controller.dispatch(ConversationEvent.Pause)
        recognizer?.cancel()
        textToSpeech?.stop()
        ttsTerminalCompletionGate.reset()
        handler.removeCallbacks(listenRunnable)
        handler.removeCallbacks(turnRetryRunnable)
        handler.removeCallbacks(sessionTimeout)
        releaseWakeLock()
        publish(STATE_PAUSED, detail = detail)
        if (releaseFloor) changeFloor("release", "idle")
        updateNotification(detail, paused = true)
    }

    private fun resumeSession() {
        if (!active) {
            startSession()
            return
        }
        conversationPrerequisiteError()?.let {
            pauseForConfigurationError(it)
            return
        }
        controller.dispatch(ConversationEvent.Resume)
        restoreResumableResponse()
        stopAfterSpeechReason = null
        pauseAfterSpeechDetail = null
        silencePolicy.markActivity()
        Log.i(TAG, "event=session_resumed")
        acquireWakeLock()
        handler.removeCallbacks(sessionTimeout)
        handler.postDelayed(sessionTimeout, SESSION_MAX_MILLIS)
        bootstrapFloorEvents()
        changeFloor("claim", "listening") { result ->
            handler.post {
                result.fold(
                    onSuccess = {
                        publish(STATE_STARTING, detail = "Resuming conversation")
                        updateNotification("Listening for you", paused = false)
                        if (pendingTurn == null) pendingTurn = restorePendingTurn()
                        if (resumableResponse.pending != null) {
                            resumeInterruptedSpeech.run()
                        } else if (pendingTurn != null) {
                            submitPendingTurn()
                        } else {
                            scheduleListening(150L)
                        }
                    },
                    onFailure = { scheduleFloorClaimRetry("Waiting to reconnect") },
                )
            }
        }
    }

    private fun stopSession(reason: ConversationStopReason, detail: String) {
        if (!active && !speaking) {
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return
        }
        generation += 1
        controller.dispatch(ConversationEvent.End(reason))
        Log.i(TAG, "event=session_stopped reason=${reason.wireValue}")
        stopAfterSpeechReason = null
        pauseAfterSpeechDetail = null
        pendingSpeech = null
        currentSpeech = null
        resumableResponse.clear()
        clearResumableResponse()
        handler.removeCallbacksAndMessages(null)
        releaseRecognizer()
        textToSpeech?.stop()
        ttsTerminalCompletionGate.reset()
        mediaSession?.isActive = false
        mediaSession?.release()
        mediaSession = null
        releaseWakeLock()
        pendingApproval = null
        clearPendingTurn()
        publish(
            STATE_STOPPED,
            detail = detail,
            activeOverride = false,
            stopReason = reason.wireValue,
        )
        changeFloor("release", "idle")
        VoiceWidgetProvider.updateStatus(this, "Ready")
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun beginListening() {
        if (!active || paused || speaking || requestInFlight || pendingTurn != null) return
        if (recognizer == null) {
            recognizer = createRecognizer()
        }
        clearRecognitionWatchdog()
        latestPartial = null
        recognitionResultAccepted = false
        recognitionSpeechDetected = false
        val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
            putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
            putExtra(RecognizerIntent.EXTRA_LANGUAGE, Locale.US.toLanguageTag())
            putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
            putExtra(RecognizerIntent.EXTRA_PREFER_OFFLINE, true)
            putExtra(RecognizerIntent.EXTRA_MAX_RESULTS, 3)
            putExtra(
                RecognizerIntent.EXTRA_SPEECH_INPUT_MINIMUM_LENGTH_MILLIS,
                RecognitionWatchdogPolicy.SPEECH_INPUT_MINIMUM_LENGTH_MILLIS,
            )
            putExtra(
                RecognizerIntent.EXTRA_SPEECH_INPUT_COMPLETE_SILENCE_LENGTH_MILLIS,
                RecognitionWatchdogPolicy.SPEECH_INPUT_COMPLETE_SILENCE_MILLIS,
            )
            putExtra(
                RecognizerIntent.EXTRA_SPEECH_INPUT_POSSIBLY_COMPLETE_SILENCE_LENGTH_MILLIS,
                RecognitionWatchdogPolicy.SPEECH_INPUT_POSSIBLY_COMPLETE_SILENCE_MILLIS,
            )
        }
        Log.i(TAG, "event=recognizer_start idle_ms=${silencePolicy.idleDurationMillis()}")
        playConversationTone(ToneGenerator.TONE_PROP_BEEP2)
        publish(STATE_STARTING, detail = "Opening microphone")
        updateNotification("Listening for you", paused = false)
        try {
            recognizer?.startListening(intent)
        } catch (error: RuntimeException) {
            Log.w(TAG, "event=recognizer_start_failure", error)
            recoverRecognizer(SpeechRecognizer.ERROR_CLIENT)
        }
    }

    private fun scheduleListening(delayMillis: Long) {
        handler.removeCallbacks(listenRunnable)
        handler.postDelayed(listenRunnable, delayMillis)
    }

    private fun requestRecognitionFinalization(reason: String) {
        if (
            !recognitionInProgress || recognitionFinalizationRequested || recognitionResultAccepted ||
            !active || paused || speaking || requestInFlight
        ) return
        recognitionFinalizationRequested = true
        recognitionFinalizationReason = reason
        handler.removeCallbacks(recognitionQuietTimeout)
        handler.removeCallbacks(recognitionHardTimeout)
        Log.w(
            TAG,
            "event=recognizer_finalization_requested reason=$reason partial_chars=${latestPartial?.length ?: 0}",
        )
        publish(STATE_PROCESSING, detail = "Finishing transcript")
        runCatching { recognizer?.stopListening() }
            .onFailure { Log.w(TAG, "event=recognizer_stop_failure", it) }
        handler.removeCallbacks(recognitionResultFallback)
        handler.postDelayed(
            recognitionResultFallback,
            RecognitionWatchdogPolicy.FINAL_RESULT_GRACE_MILLIS,
        )
    }

    private fun clearRecognitionWatchdog() {
        handler.removeCallbacks(recognitionQuietTimeout)
        handler.removeCallbacks(recognitionHardTimeout)
        handler.removeCallbacks(recognitionResultFallback)
        recognitionInProgress = false
        recognitionFinalizationRequested = false
        recognitionFinalizationReason = null
        recognitionSpeechDetected = false
    }

    private fun handleSilence() {
        val idleMillis = silencePolicy.idleDurationMillis()
        if (silencePolicy.shouldEndConversation()) {
            Log.i(TAG, "event=inactivity_timeout idle_ms=$idleMillis")
            speakThenPause(
                "I haven't heard anything, so I paused our conversation. Resume whenever you are ready.",
                "Paused after inactivity",
            )
        } else {
            Log.i(TAG, "event=silence_retry idle_ms=$idleMillis")
            publish(STATE_STARTING, detail = "Still listening")
            scheduleListening(SILENCE_RETRY_DELAY_MILLIS)
        }
    }

    private fun handleRecognizedText(text: String) {
        when (ConversationCommands.action(text)) {
            ConversationCommands.Action.STOP -> {
                Log.i(TAG, "event=voice_command action=end")
                speakThenStop("Okay. Conversation ended.", ConversationStopReason.USER_VOICE)
                return
            }
            ConversationCommands.Action.PAUSE -> {
                Log.i(TAG, "event=voice_command action=pause")
                pauseAfterSpeechDetail = "Conversation paused by voice"
                speakResponse("Conversation paused. Use the notification to resume.")
                return
            }
            null -> Unit
        }
        if (ConversationCommands.normalize(text) in setOf("read worker results", "read background updates", "what did the workers find")) {
            val reports = backgroundConversationUpdates.drain()
            val response = reports.joinToString(" ").ifBlank { "There are no background worker updates yet." }
            publish(STATE_SPEAKING, transcript = text, response = response, provider = "hermes-subagent", detail = "Reading worker updates")
            speakResponse(response)
            return
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
        pendingTurn = PendingConversationTurn(UUID.randomUUID().toString(), text)
        persistPendingTurn(pendingTurn!!)
        submitPendingTurn()
    }

    private fun submitPendingTurn() {
        val queued = pendingTurn ?: return
        if (!active || paused || requestInFlight) return
        publish(
            STATE_PROCESSING,
            transcript = queued.text,
            detail = "VIC is thinking",
        )
        submitPendingTurnCore()
    }

    private fun submitPendingTurnCore() {
        val queued = pendingTurn ?: return
        if (!active || paused || requestInFlight) return
        val requestGeneration = ++generation
        controller.dispatch(ConversationEvent.TurnSubmitted)
        publish(
            STATE_PROCESSING,
            transcript = queued.text,
            detail = if (queued.retryAttempt == 0) "VIC is thinking" else "Connection restored — retrying",
        )
        updateNotification("VIC is thinking", paused = false)
        GatewayClient.submitText(
            GatewaySettings.baseUrl(this),
            sessionId,
            queued.text,
            DeviceCredentials.token(this),
            requestId = queued.requestId,
        ) { result ->
            handler.post {
                handler.removeCallbacks(turnWatchdog)
                if (!active || requestGeneration != generation) return@post
                result.fold(
                    onSuccess = { turn ->
                        clearPendingTurn()
                        Log.i(
                            TAG,
                            "event=gateway_turn_success provider=${turn.provider} processing_ms=${turn.processingMs}",
                        )
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
                        Log.e(TAG, "event=gateway_turn_failure", error)
                        handleTurnFailure(queued, error)
                    },
                )
            }
        }
        handler.removeCallbacks(turnWatchdog)
        handler.postDelayed(turnWatchdog, GatewayTimeoutPolicy.CONVERSATION_TURN_WATCHDOG_MILLIS)
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
        controller.dispatch(ConversationEvent.TurnSubmitted)
        publish(STATE_PROCESSING, transcript = text, detail = "Recording approval decision")
        GatewayClient.decideApproval(
            GatewaySettings.baseUrl(this),
            approval.requestId,
            approve,
            DeviceCredentials.token(this),
        ) { result ->
            handler.post {
                if (!active || requestGeneration != generation) return@post
                result.fold(
                    onSuccess = { decision ->
                        pendingApproval = null
                        val response = decision.responseText.ifBlank {
                            if (approve) "Approved." else "Rejected."
                        }
                        publish(STATE_SPEAKING, transcript = text, response = response, detail = "VIC is speaking")
                        speakResponse(response)
                    },
                    onFailure = {
                        controller.dispatch(ConversationEvent.RetryScheduled)
                        speakResponse("I could not record that approval decision. Please try again.")
                    },
                )
            }
        }
        return true
    }

    private fun speakThenStop(text: String, reason: ConversationStopReason) {
        stopAfterSpeechReason = reason
        speakResponse(text)
    }

    private fun speakThenPause(text: String, detail: String) {
        pauseAfterSpeechDetail = detail
        speakResponse(text)
    }

    private fun speakResponse(text: String) {
        if (!active) return
        ttsTerminalCompletionGate.reset()
        handler.removeCallbacks(resumeInterruptedSpeech)
        recognizer?.cancel()
        controller.dispatch(ConversationEvent.ResponseStarted)
        currentSpeech = text
        currentUtteranceId = "vic-${UUID.randomUUID()}"
        playConversationTone(ToneGenerator.TONE_PROP_BEEP)
        publish(STATE_SPEAKING, response = text, detail = "VIC is speaking")
        updateNotification("VIC is speaking", paused = false)
        if (!ttsReady) {
            pendingSpeech = text
            return
        }
        textToSpeech?.setSpeechRate(currentSpeechRate())
        if (textToSpeech?.speak(text, TextToSpeech.QUEUE_FLUSH, null, currentUtteranceId) == TextToSpeech.ERROR) {
            currentSpeech = null
            val stopReason = stopAfterSpeechReason
            val pauseDetail = pauseAfterSpeechDetail
            stopAfterSpeechReason = null
            pauseAfterSpeechDetail = null
            when {
                stopReason != null -> stopSession(stopReason, "Conversation ended")
                pauseDetail != null -> pauseSession(pauseDetail)
                else -> {
                    controller.dispatch(ConversationEvent.ResponseFinished)
                    scheduleListening(350L)
                }
            }
        }
    }

    private fun recoverRecognizer(error: Int) {
        recognizerRecoveryAttempt += 1
        val exponent = (recognizerRecoveryAttempt - 1).coerceIn(0, 4)
        val delay = (350L * (1L shl exponent)).coerceAtMost(5_000L)
        controller.dispatch(ConversationEvent.RetryScheduled)
        Log.w(
            TAG,
            "event=recognizer_recovery code=$error attempt=$recognizerRecoveryAttempt delay_ms=$delay",
        )
        resetRecognizer()
        publish(STATE_RECONNECTING, detail = "Reopening microphone")
        updateNotification("Reopening microphone", paused = false)
        scheduleListening(delay)
    }

    private fun handleTurnFailure(queued: PendingConversationTurn, error: Throwable) {
        if (error is GatewayHttpException && error.status in setOf(401, 403)) {
            pendingTurn = queued
            persistPendingTurn(queued)
            pauseForConfigurationError("VoiceOS enrollment needs attention. Your last request is saved.")
            return
        }
        val retry = queued.copy(retryAttempt = queued.retryAttempt + 1)
        pendingTurn = retry
        persistPendingTurn(retry)
        controller.dispatch(ConversationEvent.RetryScheduled)
        val delay = ConversationRetryPolicy.delayMillis(retry.retryAttempt)
        Log.w(
            TAG,
            "event=turn_retry_scheduled attempt=${retry.retryAttempt} delay_ms=$delay request_id=${retry.requestId}",
        )
        publish(
            STATE_RECONNECTING,
            transcript = retry.text,
            detail = "Connection interrupted — retrying automatically",
        )
        updateNotification("Connection interrupted — retrying", paused = false)
        handler.removeCallbacks(turnRetryRunnable)
        handler.postDelayed(turnRetryRunnable, delay)
    }

    private fun pauseForConfigurationError(message: String) {
        if (!active) controller.dispatch(ConversationEvent.Start)
        Log.w(TAG, "event=configuration_pause detail=${message.replace(' ', '_')}")
        if (paused) {
            publish(STATE_PAUSED, response = message, detail = message)
            updateNotification(message, paused = true)
        } else {
            pauseSession(message)
        }
    }

    private fun conversationPrerequisiteError(): String? = when {
        checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED ->
            "Microphone permission is required for Conversation Mode."

        !SpeechRecognizer.isOnDeviceRecognitionAvailable(this) ->
            "On-device speech recognition is unavailable. Install the offline English speech model."

        DeviceCredentials.token(this).isNullOrBlank() ->
            "VoiceOS device enrollment is required before starting a VIC conversation."

        else -> null
    }

    private fun scheduleFloorClaimRetry(detail: String) {
        if (!active || paused) return
        floorReconnectAttempt += 1
        controller.dispatch(ConversationEvent.RetryScheduled)
        val delay = ConversationRetryPolicy.delayMillis(floorReconnectAttempt)
        Log.w(TAG, "event=floor_claim_retry attempt=$floorReconnectAttempt delay_ms=$delay")
        publish(STATE_RECONNECTING, detail = detail)
        updateNotification(detail, paused = false)
        handler.postDelayed({
            if (active && !paused) resumeSession()
        }, delay)
    }

    private fun pauseForRemoteHandoff(displayName: String?) {
        if (!active || paused) return
        floorLeaseId = null
        Log.i(TAG, "event=remote_handoff device=${displayName.orEmpty().replace(' ', '_')}")
        pauseSession(
            "Conversation continued on ${displayName ?: "another device"}",
            releaseFloor = false,
        )
    }

    private fun captureCurrentResponseForResume() {
        if (resumableResponse.pause(currentSpeech, speaking)) {
            getSharedPreferences(PREFERENCES, MODE_PRIVATE).edit()
                .putString(RESUMABLE_RESPONSE, resumableResponse.pending)
                .apply()
        }
    }

    private fun restoreResumableResponse() {
        resumableResponse.restore(
            getSharedPreferences(PREFERENCES, MODE_PRIVATE)
                .getString(RESUMABLE_RESPONSE, null),
        )
    }

    private fun clearResumableResponse() {
        getSharedPreferences(PREFERENCES, MODE_PRIVATE).edit()
            .remove(RESUMABLE_RESPONSE)
            .apply()
    }

    private fun persistPendingTurn(turn: PendingConversationTurn) {
        getSharedPreferences(PREFERENCES, MODE_PRIVATE).edit()
            .putString(PENDING_TURN_REQUEST_ID, turn.requestId)
            .putString(PENDING_TURN_TEXT, turn.text)
            .putInt(PENDING_TURN_RETRY_ATTEMPT, turn.retryAttempt)
            .apply()
    }

    private fun restorePendingTurn(): PendingConversationTurn? {
        val preferences = getSharedPreferences(PREFERENCES, MODE_PRIVATE)
        val requestId = preferences.getString(PENDING_TURN_REQUEST_ID, null)?.takeIf(String::isNotBlank)
            ?: return null
        val text = preferences.getString(PENDING_TURN_TEXT, null)?.takeIf(String::isNotBlank)
            ?: return null
        return PendingConversationTurn(
            requestId = requestId,
            text = text,
            retryAttempt = preferences.getInt(PENDING_TURN_RETRY_ATTEMPT, 0).coerceAtLeast(0),
        )
    }

    private fun clearPendingTurn() {
        pendingTurn = null
        getSharedPreferences(PREFERENCES, MODE_PRIVATE).edit()
            .remove(PENDING_TURN_REQUEST_ID)
            .remove(PENDING_TURN_TEXT)
            .remove(PENDING_TURN_RETRY_ATTEMPT)
            .apply()
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
        stopReason: String? = null,
    ) {
        persistSnapshot(
            ConversationSnapshot(
                state = state,
                active = activeOverride,
                transcript = transcript,
                response = response,
                provider = provider,
                processingMillis = processingMillis,
                stopReason = stopReason,
            ),
        )
        sendBroadcast(Intent(ACTION_STATE).apply {
            setPackage(packageName)
            putExtra(EXTRA_STATE, state)
            putExtra(EXTRA_ACTIVE, activeOverride)
            putExtra(EXTRA_TRANSCRIPT, transcript)
            putExtra(EXTRA_RESPONSE, response)
            putExtra(EXTRA_PROVIDER, provider)
            putExtra(EXTRA_PROCESSING_MS, processingMillis)
            putExtra(EXTRA_DETAIL, detail)
            putExtra(EXTRA_STOP_REASON, stopReason)
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
            STATE_RECONNECTING -> "Conversation reconnecting"
            STATE_SPEAKING -> "VIC speaking"
            STATE_PAUSED -> "Conversation paused"
            STATE_ERROR -> "Conversation error"
            else -> "Ready"
        })
        val phase = when (state) {
            STATE_LISTENING, STATE_STARTING -> "listening"
            STATE_PROCESSING, STATE_RECONNECTING -> "processing"
            STATE_SPEAKING -> "speaking"
            else -> null
        }
        if (phase != null && activeOverride && floorLeaseId != null) {
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
        val expectedLeaseId = if (action == "claim") null else floorLeaseId
        GatewayClient.changeConversationFloor(
            baseUrl = GatewaySettings.baseUrl(this),
            request = ConversationFloorRequest(
                action = action,
                phase = phase,
                partialTranscript = transcript,
                responseText = response,
                expectedLeaseId = expectedLeaseId,
            ),
            deviceToken = DeviceCredentials.token(this),
        ) { result ->
            result.onSuccess { floor ->
                if (floor.revision >= floorRevision) {
                    floorRevision = floor.revision
                    floorLeaseId = floor.leaseId
                }
                if (action == "claim") floorReconnectAttempt = 0
            }.onFailure { error ->
                if (
                    action != "claim" && error is GatewayHttpException && error.status == 409 &&
                    (error.responseBody.contains("conversation_floor_lease_mismatch") ||
                        error.responseBody.contains("conversation_floor_not_owned"))
                ) {
                    handler.post { pauseForRemoteHandoff(null) }
                }
            }
            callback(result)
        }
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
                        floorReconnectAttempt = 0
                        floorCursor = cursor
                        startFloorEvents(token)
                    },
                    onFailure = {
                        floorReconnectAttempt += 1
                        val delay = ConversationRetryPolicy.delayMillis(floorReconnectAttempt)
                        Log.w(TAG, "event=floor_event_cursor_retry delay_ms=$delay", it)
                        handler.postDelayed({ bootstrapFloorEvents() }, delay)
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
            onConnected = { floorReconnectAttempt = 0 },
            onEvent = { event ->
                floorCursor = event.id
                when (event.type) {
                    "conversation.floor.changed" -> {
                        val value = event.payload.optJSONObject("floor") ?: return@streamEvents
                        val next = GatewayClient.parseConversationFloor(value)
                        if (next.revision <= floorRevision) return@streamEvents
                        floorRevision = next.revision
                        val thisDevice = DeviceCredentials.deviceId(this)
                        if (
                            active && next.active && next.holderDeviceId != thisDevice &&
                            next.leaseId != floorLeaseId
                        ) {
                            handler.post {
                                pauseForRemoteHandoff(next.holderDisplayName)
                            }
                        } else if (next.holderDeviceId == thisDevice) {
                            floorLeaseId = next.leaseId
                        }
                    }

                    "conversation.turn" -> if (
                        event.payload.optString("provider") == "hermes-subagent" &&
                        event.payload.optString("session_id") == sessionId
                    ) {
                        val report = event.payload.optString("response_text").trim()
                        if (report.isNotEmpty()) {
                            val stableId = BackgroundMessagePipeline.stableId(event)
                            backgroundConversationUpdates.enqueue(stableId, report)
                            if (BackgroundMessageStore(this@VICConversationService).add(stableId, report) &&
                                BackgroundMessagePipeline.shouldNotify(activeConversation = active, alreadyDelivered = false)) {
                                postBackgroundNotification(report)
                            }
                        }
                    }
                }
            },
            onClosed = { error ->
                if (active) {
                    floorReconnectAttempt += 1
                    val delay = ConversationRetryPolicy.delayMillis(floorReconnectAttempt)
                    Log.w(TAG, "event=floor_event_stream_closed delay_ms=$delay", error)
                    handler.postDelayed({ startFloorEvents(token) }, delay)
                }
            },
        )
    }


    private data class ConversationSnapshot(
        val state: String,
        val active: Boolean,
        val transcript: String? = null,
        val response: String? = null,
        val provider: String? = null,
        val processingMillis: Long = 0L,
        val stopReason: String? = null,
    )

    private fun persistSnapshot(snapshot: ConversationSnapshot) {
        getSharedPreferences(PREFERENCES, MODE_PRIVATE).edit()
            .putBoolean(SNAPSHOT_ACTIVE, snapshot.active)
            .putString(SNAPSHOT_STATE, snapshot.state)
            .apply {
                if (snapshot.transcript != null) putString(SNAPSHOT_TRANSCRIPT, snapshot.transcript)
                if (snapshot.response != null) putString(SNAPSHOT_RESPONSE, snapshot.response)
                if (snapshot.provider != null) putString(SNAPSHOT_PROVIDER, snapshot.provider)
                if (snapshot.processingMillis > 0) putLong(SNAPSHOT_PROCESSING_MS, snapshot.processingMillis)
                if (snapshot.stopReason != null) putString(SNAPSHOT_STOP_REASON, snapshot.stopReason)
                else if (snapshot.state != STATE_STOPPED) remove(SNAPSHOT_STOP_REASON)
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
        val stop = PendingIntent.getActivity(
            this,
            512,
            Intent(this, MainActivity::class.java).apply {
                action = MainActivity.ACTION_CONFIRM_END_CONVERSATION
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP
            },
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
            .addAction(Notification.Action.Builder(null, "End…", stop).build())
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
        val sound = Uri.parse("android.resource://$packageName/${R.raw.vic_checkin}")
        getSystemService(NotificationManager::class.java).createNotificationChannel(NotificationChannel("vic_messages", "VIC Messages", NotificationManager.IMPORTANCE_DEFAULT).apply {
            setSound(sound, AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_NOTIFICATION).build())
        })
    }

    private fun postBackgroundNotification(text: String) {
        val intent = PendingIntent.getActivity(this, 820, Intent(this, MainActivity::class.java).apply {
            action = MainActivity.ACTION_VIC_MESSAGES
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP
        }, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        getSystemService(NotificationManager::class.java).notify(820, Notification.Builder(this, "vic_messages").setSmallIcon(R.mipmap.ic_launcher).setContentTitle("VIC check-in").setContentText(text).setContentIntent(intent).setAutoCancel(true).build())
    }
    private fun playConversationTone(tone: Int) {
        runCatching {
            if (toneGenerator == null) toneGenerator = ToneGenerator(AudioManager.STREAM_NOTIFICATION, 70)
            toneGenerator?.startTone(tone, 120)
        }.onFailure { Log.w(TAG, "event=conversation_tone_failure", it) }
    }

    private fun acquireWakeLock() {
        if (wakeLock?.isHeld == true) return
        val manager = getSystemService(PowerManager::class.java)
        wakeLock = manager.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "$packageName:vic-conversation")
            .apply { acquire(SESSION_MAX_MILLIS + 30_000L) }
    }

    private fun releaseWakeLock() {
        wakeLock?.takeIf { it.isHeld }?.release()
        wakeLock = null
    }

    private fun resetRecognizer() {
        resettingRecognizer = true
        releaseRecognizer()
        recognizer = createRecognizer()
        handler.post { resettingRecognizer = false }
    }

    private fun createRecognizer(): SpeechRecognizer {
        val speechRecognizer = when (recognitionBackend) {
            RecognitionBackend.ON_DEVICE -> SpeechRecognizer.createOnDeviceSpeechRecognizer(this)
            RecognitionBackend.PLATFORM -> SpeechRecognizer.createSpeechRecognizer(this)
        }
        Log.i(TAG, "event=recognizer_created backend=${recognitionBackend.name.lowercase()}")
        return speechRecognizer.apply { setRecognitionListener(recognitionListener) }
    }

    private fun switchToPlatformRecognizer(reason: String) {
        recognitionBackend = recognitionBackend.afterStall()
        getSharedPreferences(PREFERENCES, MODE_PRIVATE).edit()
            .putString(RECOGNITION_BACKEND, recognitionBackend.name)
            .apply()
        Log.w(TAG, "event=recognizer_backend_fallback backend=platform reason=$reason")
    }

    private fun releaseRecognizer() {
        clearRecognitionWatchdog()
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
        SpeechRecognizer.ERROR_AUDIO -> "The microphone had a temporary problem. VIC will reopen it."
        SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS -> "Microphone permission is required for Conversation Mode."
        SpeechRecognizer.ERROR_LANGUAGE_NOT_SUPPORTED,
        SpeechRecognizer.ERROR_LANGUAGE_UNAVAILABLE -> "The offline English speech model is unavailable."
        SpeechRecognizer.ERROR_NETWORK,
        SpeechRecognizer.ERROR_NETWORK_TIMEOUT -> "Offline speech recognition is reconnecting."
        else -> "Speech recognition was interrupted. VIC will reopen the microphone."
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
        const val EXTRA_STOP_REASON = "stop_reason"
        const val EXTRA_APPROVAL_ID = "approval_id"
        const val EXTRA_APPROVAL_TOOL = "approval_tool"
        const val EXTRA_APPROVAL_EXPIRES = "approval_expires"
        const val EXTRA_APPROVAL_ARGUMENTS = "approval_arguments"

        const val STATE_STARTING = "STARTING"
        const val STATE_LISTENING = "LISTENING"
        const val STATE_PROCESSING = "PROCESSING"
        const val STATE_RECONNECTING = "RECONNECTING"
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
        const val SNAPSHOT_STOP_REASON = "snapshot_stop_reason"

        private const val SESSION_ID = "session_id"
        private const val PENDING_TURN_REQUEST_ID = "pending_turn_request_id"
        private const val PENDING_TURN_TEXT = "pending_turn_text"
        private const val PENDING_TURN_RETRY_ATTEMPT = "pending_turn_retry_attempt"
        private const val RESUMABLE_RESPONSE = "resumable_response"
        private const val RECOGNITION_BACKEND = "recognition_backend"
        private const val PLAYBACK_PREFERENCES = "voiceos_playback"
        private const val SPEECH_RATE_KEY = "speech_rate"
        private const val TTS_VOICE_KEY = "tts_voice_name"
        private const val CHANNEL_ID = "vic_conversation"
        private const val NOTIFICATION_ID = 12
        private const val UTTERANCE_ID = "vic-conversation-response"
        private const val SESSION_MAX_MILLIS = 30 * 60 * 1_000L
        private const val CONVERSATION_IDLE_TIMEOUT_MILLIS = 20_000L
        private const val SILENCE_RETRY_DELAY_MILLIS = 450L

        private const val LISTEN_AFTER_TTS_DELAY_MILLIS = 250L
        private const val INTERRUPTION_RESUME_DELAY_MILLIS = 900L
        private const val PHONE_AUDIO_RECHECK_MILLIS = 1_000L
        private const val TAG = "VICConversation"

        private val APPROVE_COMMANDS = setOf("approve", "approved", "yes approve", "confirm")
        private val DENY_COMMANDS = setOf("deny", "denied", "no deny", "reject", "cancel")
    }
}
