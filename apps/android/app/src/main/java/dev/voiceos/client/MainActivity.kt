package dev.voiceos.client

import android.Manifest
import android.annotation.SuppressLint
import android.app.AlertDialog
import android.app.Activity
import android.app.TimePickerDialog
import android.appwidget.AppWidgetManager
import android.content.BroadcastReceiver
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ComponentName
import android.content.Intent
import android.content.Context
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.Typeface
import android.media.AudioAttributes
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import android.speech.tts.Voice
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.widget.Button
import android.widget.CheckBox
import android.widget.EditText
import android.widget.ArrayAdapter
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Spinner
import android.widget.TextView
import android.widget.Toast
import java.io.ByteArrayOutputStream
import java.time.DayOfWeek
import java.util.Locale
import java.util.UUID

class MainActivity : Activity(), TextToSpeech.OnInitListener {
    private enum class VoiceState { READY, STARTING, LISTENING, PROCESSING, SPEAKING, ERROR }
    private enum class AppPage { FEED, COMMAND, TASKS, HISTORY, SYSTEM }
    private enum class TaskFilter { TODAY, PROJECTS, VIC_WORKING, REVIEW, WINS }

    private lateinit var statusView: TextView
    private lateinit var voiceTitleView: TextView
    private lateinit var gatewayView: TextView
    private lateinit var transcriptView: TextView
    private lateinit var memoryStatusView: TextView
    private lateinit var providerStatusView: TextView
    private lateinit var systemStatusView: TextView
    private lateinit var systemDetailView: TextView
    private lateinit var agentSummaryView: TextView
    private lateinit var agentActivityContainer: LinearLayout
    private lateinit var agentWorkerStatusView: TextView
    private lateinit var agentWorkerContainer: LinearLayout
    private lateinit var skillProposalStatusView: TextView
    private lateinit var skillProposalContainer: LinearLayout
    private lateinit var skillCatalogStatusView: TextView
    private lateinit var skillCatalogContainer: LinearLayout
    private lateinit var skillUsageContainer: LinearLayout
    private lateinit var historyView: TextView
    private lateinit var taskStatusView: TextView
    private lateinit var taskContainer: LinearLayout
    private lateinit var feedStatusView: TextView
    private lateinit var feedContainer: LinearLayout
    private lateinit var ttsStatusView: TextView
    private lateinit var voiceButton: Button
    private lateinit var rootScroll: ScrollView
    private lateinit var talkButton: HexTalkButton
    private lateinit var cancelButton: Button
    private lateinit var repeatButton: Button
    private lateinit var copyButton: Button
    private lateinit var correctButton: Button
    private lateinit var retryButton: Button
    private lateinit var speedButton: Button
    private lateinit var uploadButton: Button
    private lateinit var cameraButton: Button
    private lateinit var photoButton: Button
    private lateinit var approveButton: Button
    private lateinit var denyButton: Button
    private val stateTrackViews = mutableListOf<TextView>()
    private val pageViews = mutableMapOf<AppPage, View>()
    private val navViews = mutableMapOf<AppPage, TextView>()

    private var voiceState = VoiceState.READY
    private var currentPage = AppPage.FEED
    private var speechRecognizer: SpeechRecognizer? = null
    private var textToSpeech: TextToSpeech? = null
    private var textToSpeechReady = false
    private var textToSpeechInitializationComplete = false
    private var pendingSpeech: Pair<String, String>? = null
    private var ttsInitializationAttempts = 0
    private var availableVoices: List<Voice> = emptyList()
    private var selectedVoiceIndex = 0
    private var speechRate = PlaybackSpeed.DEFAULT
    private var correctionMode = false
    private var pendingCorrectionAfterSpeech = false
    private var pendingPermissionCorrection = false
    private var requestGeneration = 0
    private var latestPartialTranscript: String? = null
    private var pendingAttachment: AttachmentUploadResult? = null

    private val sessionId = UUID.randomUUID().toString()
    private var lastTranscript: String? = null
    private var lastResponse: String? = null
    private var failedTranscript: String? = null
    private var pendingApproval: ApprovalRequest? = null
    private var eventSubscription: EventSubscription? = null
    private var conversationActive = false
    private var conversationPaused = false
    private var conversationReceiverRegistered = false
    private var currentTaskFilter = TaskFilter.TODAY
    private var latestTasks: List<VoiceTask> = emptyList()
    private var latestAiUpdates: List<AiUpdate> = emptyList()
    private var aiUpdatesRefreshing = false
    private var latestProjects: List<VoiceProject> = emptyList()
    private val weeklyCreationInFlight = mutableSetOf<String>()
    private val agentVisibility = AgentVisibilityModel()
    private var agentRecoveryInFlight = false

    private val conversationReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if (intent?.action != VICConversationService.ACTION_STATE) return
            conversationActive = intent.getBooleanExtra(VICConversationService.EXTRA_ACTIVE, false)
            val conversationState = intent.getStringExtra(VICConversationService.EXTRA_STATE)
            conversationPaused = conversationState == VICConversationService.STATE_PAUSED
            val transcript = intent.getStringExtra(VICConversationService.EXTRA_TRANSCRIPT)
            val response = intent.getStringExtra(VICConversationService.EXTRA_RESPONSE)
            val provider = intent.getStringExtra(VICConversationService.EXTRA_PROVIDER)
            val processingMs = intent.getLongExtra(VICConversationService.EXTRA_PROCESSING_MS, 0L)
            val detail = intent.getStringExtra(VICConversationService.EXTRA_DETAIL).orEmpty()
            if (!transcript.isNullOrBlank()) {
                lastTranscript = transcript
                failedTranscript = null
            }
            if (!response.isNullOrBlank()) lastResponse = response
            when {
                !transcript.isNullOrBlank() && !response.isNullOrBlank() ->
                    transcriptView.text = "You: $transcript\n\nVIC: $response"
                !transcript.isNullOrBlank() -> transcriptView.text = "You: $transcript"
                !response.isNullOrBlank() -> {
                    val prior = lastTranscript?.let { "You: $it\n\n" }.orEmpty()
                    transcriptView.text = "${prior}VIC: $response"
                }
            }
            if (!provider.isNullOrBlank()) {
                providerStatusView.text = "${provider.uppercase(Locale.US)}  â€¢  ACTIVE"
                providerStatusView.setTextColor(CarbonPalette.teal)
            }
            val approvalId = intent.getStringExtra(VICConversationService.EXTRA_APPROVAL_ID)
            val approvalTool = intent.getStringExtra(VICConversationService.EXTRA_APPROVAL_TOOL)
            if (!approvalId.isNullOrBlank() && !approvalTool.isNullOrBlank()) {
                val arguments = runCatching {
                    org.json.JSONObject(
                        intent.getStringExtra(VICConversationService.EXTRA_APPROVAL_ARGUMENTS).orEmpty()
                    )
                }.getOrDefault(org.json.JSONObject())
                pendingApproval = ApprovalRequest(
                    approvalId,
                    approvalTool,
                    intent.getLongExtra(VICConversationService.EXTRA_APPROVAL_EXPIRES, 0L),
                    arguments,
                )
            }
            when (conversationState) {
                VICConversationService.STATE_STARTING -> renderState(VoiceState.STARTING, detail.ifBlank { "Starting conversation" })
                VICConversationService.STATE_LISTENING -> renderState(VoiceState.LISTENING, detail.ifBlank { "Listening" })
                VICConversationService.STATE_PROCESSING -> renderState(VoiceState.PROCESSING, detail.ifBlank { "VIC is thinking" })
                VICConversationService.STATE_RECONNECTING -> renderState(
                    VoiceState.STARTING,
                    detail.ifBlank { "Reconnecting automatically" },
                )
                VICConversationService.STATE_SPEAKING -> renderState(
                    VoiceState.SPEAKING,
                    if (processingMs > 0) "Speaking â€¢ ${processingMs} ms" else detail.ifBlank { "VIC is speaking" },
                )
                VICConversationService.STATE_PAUSED -> {
                    renderState(VoiceState.READY, "Conversation paused")
                    voiceTitleView.text = "Conversation paused"
                    statusView.text = "VOICE CHANNEL PAUSED"
                }
                VICConversationService.STATE_ERROR -> renderState(VoiceState.ERROR, detail.ifBlank { "Conversation error" })
                VICConversationService.STATE_STOPPED -> {
                    conversationPaused = false
                    renderState(VoiceState.READY, "Conversation ended")
                }
            }
            if (!response.isNullOrBlank()) {
                VoiceWidgetProvider.refreshTasks(this@MainActivity)
                if (currentPage == AppPage.TASKS) loadTasks()
                if (currentPage == AppPage.FEED) loadMomentumFeed()
            }
        }
    }

    private val recognitionListener = object : RecognitionListener {
        override fun onReadyForSpeech(params: Bundle?) {
            renderState(VoiceState.LISTENING, if (correctionMode) "Listening for correction" else "Listening")
        }

        override fun onBeginningOfSpeech() {
            statusView.text = "VOICE CHANNEL LISTENING"
        }

        override fun onRmsChanged(rmsdB: Float) = Unit
        override fun onBufferReceived(buffer: ByteArray?) = Unit

        override fun onEndOfSpeech() {
            renderState(VoiceState.PROCESSING, "Finishing transcript")
        }

        override fun onError(error: Int) {
            val message = recognitionErrorMessage(error)
            failedTranscript = null
            renderState(VoiceState.ERROR, message)
            transcriptView.text = message
            speak(message, ERROR_UTTERANCE_ID)
        }

        override fun onResults(results: Bundle?) {
            val finalText = recognitionText(results)
            val text = moreCompleteTranscript(finalText, latestPartialTranscript)
            latestPartialTranscript = null
            if (text.isNullOrBlank()) {
                onError(SpeechRecognizer.ERROR_NO_MATCH)
                return
            }
            correctionMode = false
            lastTranscript = text
            failedTranscript = null
            transcriptView.text = "You: $text\n\nVIC: Thinking…"
            if (handleSpeechRateCommand(text)) return
            if (handlePendingApprovalSpeech(text)) return
            if (handleInterestCommand(text)) return
            submitText(text)
        }

        override fun onPartialResults(partialResults: Bundle?) {
            val partial = recognitionText(partialResults) ?: return
            latestPartialTranscript = partial
            transcriptView.text = "You: $partial"
            changeFloor("update", "listening", partial)
        }

        override fun onEvent(eventType: Int, params: Bundle?) = Unit
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        speechRate = getSharedPreferences(PLAYBACK_PREFERENCES, MODE_PRIVATE)
            .getFloat(SPEECH_RATE_KEY, PlaybackSpeed.DEFAULT)
            .let(PlaybackSpeed::clamp)
        setContentView(createContentView())
        initializeTextToSpeech()
        val enrollment = GatewaySettings.enrollFromIntent(this, intent)
        renderState(VoiceState.READY, "Ready")
        handleEnrollment(enrollment)
        startSharedEventStream()
        DailyCheckinScheduler.schedule(this)
        if (
            android.os.Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), REQUEST_NOTIFICATIONS)
        } else {
            startVicOutreachConnection()
        }

        loadMomentumFeed()
        startFromWidgetIfRequested(intent)
    }

    override fun onNewIntent(intent: Intent?) {
        super.onNewIntent(intent)
        setIntent(intent)
        val enrollment = GatewaySettings.enrollFromIntent(this, intent)
        handleEnrollment(enrollment)
        startFromWidgetIfRequested(intent)
    }

    @SuppressLint("UnspecifiedRegisterReceiverFlag")
    override fun onStart() {
        super.onStart()
        val filter = IntentFilter(VICConversationService.ACTION_STATE)
        if (Build.VERSION.SDK_INT >= 33) {
            registerReceiver(conversationReceiver, filter, RECEIVER_NOT_EXPORTED)
        } else {
            @Suppress("DEPRECATION")
            registerReceiver(conversationReceiver, filter)
        }
        conversationReceiverRegistered = true
        restoreConversationSnapshot()
    }

    override fun onStop() {
        if (conversationReceiverRegistered) {
            unregisterReceiver(conversationReceiver)
            conversationReceiverRegistered = false
        }
        super.onStop()
    }

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (resultCode != RESULT_OK) return
        when (requestCode) {
            REQUEST_DOCUMENT -> {
                val uri = data?.data ?: return
                val filename = DocumentInput.filename(contentResolver, uri)
                val mediaType = contentResolver.getType(uri) ?: DocumentInput.mediaTypeForFilename(filename)
                AlertDialog.Builder(this)
                    .setTitle("How should VIC use this file?")
                    .setItems(arrayOf("About me — always available", "Reference — retrieve when relevant")) { _, which ->
                        uploadDocument(uri, filename, mediaType, if (which == 0) "profile" else "reference")
                    }
                    .setNegativeButton("Cancel", null)
                    .show()
            }
            REQUEST_IMAGE -> data?.data?.let(::uploadImageFromUri)
            REQUEST_CAMERA -> (data?.extras?.get("data") as? Bitmap)?.let(::uploadCameraPreview)
            REQUEST_BRAIN_DUMP -> {
                showPage(AppPage.FEED)
                loadMomentumFeed()
                VoiceWidgetProvider.refreshTasks(this)
            }
        }
    }

    override fun onDestroy() {
        requestGeneration += 1
        eventSubscription?.close()
        eventSubscription = null
        speechRecognizer?.cancel()
        speechRecognizer?.destroy()
        speechRecognizer = null
        textToSpeech?.stop()
        textToSpeech?.shutdown()
        textToSpeech = null
        super.onDestroy()
    }

    override fun onInit(status: Int) {
        textToSpeechInitializationComplete = true
        textToSpeechReady = status == TextToSpeech.SUCCESS
        if (!textToSpeechReady) {
            if (::ttsStatusView.isInitialized) {
                ttsStatusView.text = "Speech engine unavailable"
                ttsStatusView.setTextColor(CarbonPalette.red)
            }
            if (pendingSpeech != null && ttsInitializationAttempts < 2) initializeTextToSpeech()
            return
        }

        val languageResult = textToSpeech?.setLanguage(Locale.US) ?: TextToSpeech.ERROR
        textToSpeech?.setAudioAttributes(
            AudioAttributes.Builder()
                .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                .setUsage(AudioAttributes.USAGE_ASSISTANT)
                .build(),
        )
        textToSpeech?.setSpeechRate(speechRate)
        configureVoices()
        if (::ttsStatusView.isInitialized) {
            val languageReady = languageResult != TextToSpeech.LANG_MISSING_DATA &&
                languageResult != TextToSpeech.LANG_NOT_SUPPORTED
            ttsStatusView.text = if (languageReady) "Google speech services ready" else "English voice data unavailable"
            ttsStatusView.setTextColor(if (languageReady) CarbonPalette.teal else CarbonPalette.red)
        }
        textToSpeech?.setOnUtteranceProgressListener(object : UtteranceProgressListener() {
            override fun onStart(utteranceId: String?) = Unit

            override fun onDone(utteranceId: String?) {
                runOnUiThread {
                    if (::ttsStatusView.isInitialized) {
                        ttsStatusView.text = "Google speech services ready"
                        ttsStatusView.setTextColor(CarbonPalette.teal)
                    }
                    when {
                        utteranceId == CORRECTION_PROMPT_ID && pendingCorrectionAfterSpeech -> {
                            pendingCorrectionAfterSpeech = false
                            ensurePermissionAndStart(correction = true)
                        }
                        utteranceId == RESPONSE_UTTERANCE_ID && voiceState == VoiceState.SPEAKING -> {
                            if (pendingApproval != null) {
                                renderState(VoiceState.READY, "Awaiting approval")
                                ensurePermissionAndStart(correction = false)
                            } else {
                                renderState(VoiceState.READY, "Ready")
                            }
                        }
                    }
                }
            }

            @Deprecated("Deprecated in Java")
            override fun onError(utteranceId: String?) {
                runOnUiThread {
                    if (::ttsStatusView.isInitialized) {
                        ttsStatusView.text = "Playback failed — tap Repeat to retry"
                        ttsStatusView.setTextColor(CarbonPalette.red)
                    }
                    if (voiceState == VoiceState.SPEAKING) renderState(VoiceState.READY, "Ready")
                }
            }
        })
        pendingSpeech?.also { (text, utteranceId) ->
            pendingSpeech = null
            speak(text, utteranceId)
        }
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == REQUEST_NOTIFICATIONS) {
            if (grantResults.isNotEmpty() && grantResults[0] == PackageManager.PERMISSION_GRANTED) {
                startVicOutreachConnection()
            }
            return
        }
        if (requestCode != REQUEST_MICROPHONE) return
        if (grantResults.isNotEmpty() && grantResults[0] == PackageManager.PERMISSION_GRANTED) {
            if (pendingPermissionCorrection) startRecognition(correction = true)
            else startConversationMode()
        } else {
            showRecoverableError("Microphone permission is required for voice requests.", speakError = true)
        }
    }

    private fun createContentView(): ScrollView {
        window.statusBarColor = CarbonPalette.black
        window.navigationBarColor = CarbonPalette.black
        window.decorView.systemUiVisibility = 0

        fun kicker(text: String) = TextView(this).apply {
            this.text = text.uppercase(Locale.US)
            textSize = 10f
            typeface = Typeface.DEFAULT_BOLD
            letterSpacing = 0.17f
            setTextColor(CarbonPalette.teal)
        }

        fun heading(text: String, size: Float = 24f) = TextView(this).apply {
            this.text = text
            textSize = size
            typeface = Typeface.create("sans-serif", Typeface.NORMAL)
            setTextColor(CarbonPalette.white)
        }

        fun panel(padding: Int = 20) = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(padding), dp(padding), dp(padding), dp(padding))
            background = carbonPanel(this@MainActivity)
        }

        fun navChip(label: String, active: Boolean) = TextView(this).apply {
            text = label
            textSize = 11f
            typeface = Typeface.DEFAULT_BOLD
            gravity = Gravity.CENTER
            setTextColor(if (active) CarbonPalette.teal else CarbonPalette.muted)
            background = carbonControl(
                this@MainActivity,
                if (active) CarbonPalette.teal else CarbonPalette.line,
            )
            setPadding(dp(12), dp(11), dp(12), dp(11))
            alpha = if (active) 1f else 0.72f
        }

        fun utilityRow(label: String, control: Button) = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(TextView(this@MainActivity).apply {
                text = label
                textSize = 10f
                typeface = Typeface.DEFAULT_BOLD
                letterSpacing = 0.13f
                setTextColor(CarbonPalette.muted)
            }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            addView(control, LinearLayout.LayoutParams(dp(126), ViewGroup.LayoutParams.WRAP_CONTENT))
        }

        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(18), dp(16), dp(34))
            background = CarbonBackgroundDrawable(this@MainActivity)
            setOnApplyWindowInsetsListener { view, insets ->
                val bars = insets.getInsets(WindowInsets.Type.systemBars())
                view.setPadding(
                    dp(16) + bars.left,
                    dp(18) + bars.top,
                    dp(16) + bars.right,
                    dp(34) + bars.bottom,
                )
                insets
            }
        }

        val brandRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        brandRow.addView(HexMarkView(this), LinearLayout.LayoutParams(dp(42), dp(38)))
        brandRow.addView(heading("VIC", 25f).apply {
            setPadding(dp(9), 0, 0, 0)
        }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        gatewayView = TextView(this).apply {
            text = "CHECKING"
            textSize = 10f
            typeface = Typeface.DEFAULT_BOLD
            maxLines = 1
            setTextColor(CarbonPalette.muted)
            gravity = Gravity.CENTER
            background = carbonControl(this@MainActivity, CarbonPalette.line)
            setPadding(dp(12), dp(9), dp(12), dp(9))
        }
        brandRow.addView(gatewayView)
        content.addView(brandRow, fullWidthWrap())

        val navigation = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
        }
        val feedNav = navChip("FEED", true)
        val commandNav = navChip("TALK", false)
        val tasksNav = navChip("TASKS", false)
        val historyNav = navChip("HISTORY", false)
        val systemNav = navChip("SYSTEM", false)
        navViews.clear()
        navViews[AppPage.FEED] = feedNav
        navViews[AppPage.COMMAND] = commandNav
        navViews[AppPage.TASKS] = tasksNav
        navViews[AppPage.HISTORY] = historyNav
        navViews[AppPage.SYSTEM] = systemNav
        navigation.addView(feedNav, weightedButton())
        navigation.addView(commandNav, weightedButton().apply { marginStart = dp(5) })
        navigation.addView(tasksNav, weightedButton().apply { marginStart = dp(5) })
        navigation.addView(historyNav, weightedButton().apply { marginStart = dp(5) })
        navigation.addView(systemNav, weightedButton().apply { marginStart = dp(5) })
        content.addView(navigation, fullWidthWrap().apply { topMargin = dp(18) })

        val feedPage = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(kicker("Your private momentum feed"), fullWidthWrap().apply { topMargin = dp(24) })
            addView(heading("For you, from your life", 30f), fullWidthWrap().apply { topMargin = dp(5) })
            addView(panel(17).apply {
                addView(kicker("Finite by design"), fullWidthWrap())
                addView(heading("Tasks, interests, and AI updates", 22f).apply { setPadding(0, dp(5), 0, 0) }, fullWidthWrap())
                addView(TextView(this@MainActivity).apply {
                    text = "No ads, strangers, likes, or endless scroll. Your priority stays first, followed by a few official AI releases, videos, and reports."
                    textSize = 13f
                    setTextColor(CarbonPalette.muted)
                    setPadding(0, dp(8), 0, 0)
                }, fullWidthWrap())
                addView(LinearLayout(this@MainActivity).apply {
                    orientation = LinearLayout.HORIZONTAL
                    addView(secondaryButton("+ INTEREST") { showFollowInterestDialog() }, weightedButton())
                    addView(secondaryButton("SOCIAL SHIELD") { showSocialShieldSetup() }, weightedButton().apply { marginStart = dp(7) })
                }, fullWidthWrap().apply { topMargin = dp(12) })
                addView(actionButton("BRAIN DUMP • LET VIC SORT IT").apply {
                    setOnClickListener {
                        startActivityForResult(
                            Intent(this@MainActivity, BrainDumpActivity::class.java),
                            REQUEST_BRAIN_DUMP,
                        )
                    }
                }, fullWidthWrap().apply { topMargin = dp(10) })
                addView(secondaryButton("10-MIN RESET • SCRIPTURE + FOCUS") {
                    startActivity(Intent(this@MainActivity, FocusResetActivity::class.java))
                }, fullWidthWrap().apply { topMargin = dp(9) })
                feedStatusView = TextView(this@MainActivity).apply {
                    text = "Loading your private cards…"
                    textSize = 13f
                    setTextColor(CarbonPalette.muted)
                    setPadding(0, dp(12), 0, 0)
                }
                addView(feedStatusView, fullWidthWrap())
            }, fullWidthWrap().apply { topMargin = dp(18) })
            feedContainer = LinearLayout(this@MainActivity).apply { orientation = LinearLayout.VERTICAL }
            addView(feedContainer, fullWidthWrap())
        }

        val commandPage = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            visibility = View.GONE
        }
        commandPage.addView(kicker("Carbon Command"), fullWidthWrap().apply { topMargin = dp(24) })
        commandPage.addView(heading("Command center", 30f), fullWidthWrap().apply { topMargin = dp(5) })

        val voicePanel = panel(20)
        statusView = kicker("Voice channel ready")
        voicePanel.addView(statusView, fullWidthWrap())
        voiceTitleView = heading("What can I help with?", 29f).apply {
            setPadding(0, dp(7), 0, 0)
        }
        voicePanel.addView(voiceTitleView, fullWidthWrap())
        voicePanel.addView(TextView(this).apply {
            text = "Tap the control and speak. VIC keeps the conversation connected through VoiceOS across your enrolled devices."
            textSize = 14f
            setTextColor(CarbonPalette.muted)
            setLineSpacing(0f, 1.18f)
            setPadding(0, dp(8), 0, 0)
        }, fullWidthWrap())
        talkButton = HexTalkButton(this).apply { setOnClickListener { handlePrimaryAction() } }
        voicePanel.addView(
            talkButton,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(304)).apply {
                topMargin = dp(4)
            },
        )
        val stateTrack = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
        }
        stateTrackViews.clear()
        VoiceState.entries.filter { it != VoiceState.STARTING && it != VoiceState.ERROR }.forEach { state ->
            val item = TextView(this).apply {
                text = state.name.lowercase(Locale.US)
                textSize = 9f
                typeface = Typeface.DEFAULT_BOLD
                letterSpacing = 0.08f
                gravity = Gravity.CENTER
                setPadding(dp(3), dp(10), dp(3), dp(10))
                tag = state
            }
            stateTrackViews += item
            stateTrack.addView(item, weightedButton().apply { marginStart = dp(3) })
        }
        voicePanel.addView(stateTrack, fullWidthWrap())
        cancelButton = actionButton("CANCEL").apply { setOnClickListener { cancelCurrentAction() } }
        voicePanel.addView(cancelButton, fullWidthWrap().apply { topMargin = dp(9) })
        commandPage.addView(voicePanel, fullWidthWrap().apply { topMargin = dp(18) })

        val conversationPanel = panel(18)
        val conversationHeader = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.TOP
        }
        conversationHeader.addView(LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(kicker("Continuous conversation"))
            addView(heading("Current thread", 23f).apply { setPadding(0, dp(5), 0, 0) })
        }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        memoryStatusView = TextView(this).apply {
            text = "MEMORY READY"
            textSize = 9f
            typeface = Typeface.DEFAULT_BOLD
            setTextColor(CarbonPalette.teal)
            gravity = Gravity.CENTER
            background = carbonControl(this@MainActivity, CarbonPalette.teal)
            setPadding(dp(10), dp(8), dp(10), dp(8))
        }
        conversationHeader.addView(memoryStatusView)
        conversationPanel.addView(conversationHeader, fullWidthWrap())
        transcriptView = TextView(this).apply {
            text = "VIC\nTap Talk and speak. Your conversation will continue here."
            textSize = 16f
            setTextColor(CarbonPalette.white)
            setLineSpacing(dp(3).toFloat(), 1.18f)
            setPadding(dp(16), dp(15), dp(16), dp(15))
            background = carbonControl(this@MainActivity, CarbonPalette.teal)
            setTextIsSelectable(true)
        }
        conversationPanel.addView(transcriptView, fullWidthWrap().apply { topMargin = dp(18) })
        val conversationActions = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
        }
        repeatButton = secondaryButton("↻  REPEAT") { repeatLastResponse() }
        copyButton = secondaryButton("⧉  COPY") { copyLastResponse() }
        correctButton = secondaryButton("✎  CORRECT") { beginCorrection() }
        retryButton = secondaryButton("RETRY") { retryLastRequest() }
        conversationActions.addView(repeatButton, weightedButton())
        conversationActions.addView(copyButton, weightedButton().apply { marginStart = dp(6) })
        conversationActions.addView(correctButton, weightedButton().apply { marginStart = dp(6) })
        conversationActions.addView(retryButton, weightedButton().apply { marginStart = dp(6) })
        conversationPanel.addView(conversationActions, fullWidthWrap().apply { topMargin = dp(12) })
        commandPage.addView(conversationPanel, fullWidthWrap().apply { topMargin = dp(14) })

        val approvals = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
        }
        approveButton = actionButton("APPROVE").apply {
            setOnClickListener { decidePendingApproval(approve = true) }
        }
        denyButton = actionButton("DENY").apply {
            background = carbonControl(this@MainActivity, CarbonPalette.red, filled = true)
            setOnClickListener { decidePendingApproval(approve = false) }
        }
        approvals.addView(approveButton, weightedButton())
        approvals.addView(denyButton, weightedButton().apply { marginStart = dp(8) })
        commandPage.addView(approvals, fullWidthWrap().apply { topMargin = dp(12) })

        commandPage.addView(panel(17).apply {
            addView(kicker("Live execution"), fullWidthWrap())
            addView(heading("VIC agent", 22f).apply { setPadding(0, dp(5), 0, 0) }, fullWidthWrap())
            agentSummaryView = TextView(this@MainActivity).apply {
                text = "WAITING FOR LIVE ACTIVITY"
                textSize = 12f
                typeface = Typeface.DEFAULT_BOLD
                letterSpacing = 0.08f
                setTextColor(CarbonPalette.muted)
                setPadding(0, dp(12), 0, 0)
            }
            addView(agentSummaryView, fullWidthWrap())
            addView(TextView(this@MainActivity).apply {
                text = "Safe execution status only. Private reasoning is never displayed."
                textSize = 12f
                setTextColor(CarbonPalette.muted)
                setPadding(0, dp(7), 0, 0)
            }, fullWidthWrap())
            agentActivityContainer = LinearLayout(this@MainActivity).apply {
                orientation = LinearLayout.VERTICAL
            }
            addView(agentActivityContainer, fullWidthWrap().apply { topMargin = dp(10) })
            addView(secondaryButton("REFRESH AGENT ACTIVITY") { refreshAgentVisibility() }, fullWidthWrap().apply { topMargin = dp(14) })
        }, fullWidthWrap().apply { topMargin = dp(14) })
        commandPage.addView(panel(17).apply {
            addView(kicker("Delegated work"), fullWidthWrap())
            addView(heading("Hermes subagents", 22f).apply { setPadding(0, dp(5), 0, 0) }, fullWidthWrap())
            agentWorkerStatusView = TextView(this@MainActivity).apply {
                text = "NO SUBAGENTS OBSERVED"
                textSize = 12f
                typeface = Typeface.DEFAULT_BOLD
                letterSpacing = 0.08f
                setTextColor(CarbonPalette.muted)
                setPadding(0, dp(12), 0, 0)
            }
            addView(agentWorkerStatusView, fullWidthWrap())
            agentWorkerContainer = LinearLayout(this@MainActivity).apply {
                orientation = LinearLayout.VERTICAL
            }
            addView(agentWorkerContainer, fullWidthWrap().apply { topMargin = dp(10) })
        }, fullWidthWrap().apply { topMargin = dp(14) })

        val utilityPanel = panel(16)
        utilityPanel.addView(kicker("Voice controls"), fullWidthWrap())
        speedButton = secondaryButton(speechRateButtonLabel()) { cycleSpeechRate() }.apply {
            contentDescription = "Voice playback speed ${speechRateLabel()}. Tap to increase."
        }
        utilityPanel.addView(utilityRow("VOICE PLAYBACK", speedButton), fullWidthWrap().apply { topMargin = dp(10) })
        uploadButton = secondaryButton("ADD FILE") { openDocumentPicker() }.apply {
            contentDescription = "Add a private knowledge file"
        }
        utilityPanel.addView(utilityRow("PRIVATE KNOWLEDGE", uploadButton), fullWidthWrap().apply { topMargin = dp(6) })
        cameraButton = secondaryButton("CAMERA") { openCamera() }.apply {
            contentDescription = "Take a photo and send it with your next request to VIC"
        }
        utilityPanel.addView(utilityRow("SEND A PHOTO", cameraButton), fullWidthWrap().apply { topMargin = dp(6) })
        photoButton = secondaryButton("PHOTO LIBRARY") { openImagePicker() }.apply {
            contentDescription = "Choose a photo and send it with your next request to VIC"
        }
        utilityPanel.addView(utilityRow("CHOOSE AN IMAGE", photoButton), fullWidthWrap().apply { topMargin = dp(6) })
        commandPage.addView(utilityPanel, fullWidthWrap().apply { topMargin = dp(14) })

        val providerPanel = panel(17)
        providerPanel.addView(kicker("Reasoning fabric"), fullWidthWrap())
        providerPanel.addView(heading("Model provider", 22f).apply { setPadding(0, dp(5), 0, 0) }, fullWidthWrap())
        providerStatusView = TextView(this).apply {
            text = "Checking configured provider…"
            textSize = 14f
            typeface = Typeface.DEFAULT_BOLD
            setTextColor(CarbonPalette.green)
            setPadding(dp(14), dp(14), dp(14), dp(14))
            background = carbonControl(this@MainActivity, CarbonPalette.green)
        }
        providerPanel.addView(providerStatusView, fullWidthWrap().apply { topMargin = dp(14) })
        commandPage.addView(providerPanel, fullWidthWrap().apply { topMargin = dp(14) })

        val healthPanel = panel(17)
        healthPanel.addView(kicker("Live infrastructure"), fullWidthWrap())
        healthPanel.addView(heading("System health", 22f).apply { setPadding(0, dp(5), 0, 0) }, fullWidthWrap())
        systemStatusView = TextView(this).apply {
            text = "Gateway checking\nTailnet private"
            textSize = 14f
            setTextColor(CarbonPalette.white)
            setLineSpacing(dp(3).toFloat(), 1.15f)
            setPadding(dp(14), dp(14), dp(14), dp(14))
            background = carbonControl(this@MainActivity, CarbonPalette.line)
        }
        healthPanel.addView(systemStatusView, fullWidthWrap().apply { topMargin = dp(14) })
        commandPage.addView(healthPanel, fullWidthWrap().apply { topMargin = dp(14) })

        commandPage.addView(TextView(this).apply {
            text = "PRIVATE  •  TAILSCALE  •  MEMORY ACTIVE"
            textSize = 9f
            letterSpacing = 0.11f
            setTextColor(CarbonPalette.muted)
            gravity = Gravity.CENTER
            setPadding(0, dp(24), 0, 0)
        }, fullWidthWrap())

        val tasksPage = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            visibility = View.GONE
            addView(kicker("ADHD-aware execution"), fullWidthWrap().apply { topMargin = dp(24) })
            addView(heading("Projects & momentum", 30f), fullWidthWrap().apply { topMargin = dp(5) })
            addView(panel(17).apply {
                val header = LinearLayout(this@MainActivity).apply {
                    orientation = LinearLayout.HORIZONTAL
                    gravity = Gravity.CENTER_VERTICAL
                }
                header.addView(LinearLayout(this@MainActivity).apply {
                    orientation = LinearLayout.VERTICAL
                    addView(kicker("One clear next action"))
                    addView(heading("Move something forward", 22f).apply { setPadding(0, dp(5), 0, 0) })
                }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
                addView(header, fullWidthWrap())
                addView(LinearLayout(this@MainActivity).apply {
                    orientation = LinearLayout.HORIZONTAL
                    addView(secondaryButton("+ TASK") { showTaskCreationDialog() }, weightedButton())
                    addView(secondaryButton("+ PROJECT") { showProjectCreationDialog() }, weightedButton().apply { marginStart = dp(7) })
                }, fullWidthWrap().apply { topMargin = dp(12) })
                addView(
                    secondaryButton("ADD FOCUS WIDGET") { requestFocusWidget() },
                    fullWidthWrap().apply { topMargin = dp(7) },
                )
                taskStatusView = TextView(this@MainActivity).apply {
                    text = "Loading tasks…"
                    textSize = 13f
                    setTextColor(CarbonPalette.muted)
                    setPadding(0, dp(12), 0, 0)
                }
                addView(taskStatusView, fullWidthWrap())
                val primaryFilters = LinearLayout(this@MainActivity).apply {
                    orientation = LinearLayout.HORIZONTAL
                    addView(secondaryButton("ALL TASKS") { setTaskFilter(TaskFilter.TODAY) }, weightedButton())
                    addView(secondaryButton("PROJECTS") { setTaskFilter(TaskFilter.PROJECTS) }, weightedButton().apply { marginStart = dp(5) })
                    addView(secondaryButton("VIC") { setTaskFilter(TaskFilter.VIC_WORKING) }, weightedButton().apply { marginStart = dp(5) })
                }
                addView(primaryFilters, fullWidthWrap().apply { topMargin = dp(11) })
                addView(LinearLayout(this@MainActivity).apply {
                    orientation = LinearLayout.HORIZONTAL
                    addView(secondaryButton("REVIEW") { setTaskFilter(TaskFilter.REVIEW) }, weightedButton())
                    addView(secondaryButton("WINS") { setTaskFilter(TaskFilter.WINS) }, weightedButton().apply { marginStart = dp(5) })
                }, fullWidthWrap().apply { topMargin = dp(6) })
                taskContainer = LinearLayout(this@MainActivity).apply {
                    orientation = LinearLayout.VERTICAL
                }
                addView(taskContainer, fullWidthWrap().apply { topMargin = dp(8) })
                addView(secondaryButton("REFRESH TASKS") { loadTasks() }, fullWidthWrap().apply { topMargin = dp(14) })
            }, fullWidthWrap().apply { topMargin = dp(18) })
        }

        val historyPage = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            visibility = View.GONE
            addView(kicker("Conversation memory"), fullWidthWrap().apply { topMargin = dp(24) })
            addView(heading("History", 30f), fullWidthWrap().apply { topMargin = dp(5) })
            addView(panel(17).apply {
                addView(kicker("Recent turns"), fullWidthWrap())
                historyView = EditText(this@MainActivity).apply {
                    setText("Loading conversation history…")
                    textSize = 15f
                    setTextColor(CarbonPalette.white)
                    setLineSpacing(dp(4).toFloat(), 1.18f)
                    setPadding(0, dp(14), 0, 0)
                    setTextIsSelectable(true)
                    keyListener = null
                    isCursorVisible = false
                    showSoftInputOnFocus = false
                    background = null
                    contentDescription = "Selectable conversation history. Long press text to copy."
                }
                addView(historyView, fullWidthWrap())
                addView(secondaryButton("REFRESH HISTORY") { loadHistory() }, fullWidthWrap().apply { topMargin = dp(16) })
            }, fullWidthWrap().apply { topMargin = dp(18) })
        }

        val systemPage = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            visibility = View.GONE
            addView(kicker("VoiceOS infrastructure"), fullWidthWrap().apply { topMargin = dp(24) })
            addView(heading("System", 30f), fullWidthWrap().apply { topMargin = dp(5) })
            addView(panel(17).apply {
                addView(kicker("Audio playback"), fullWidthWrap())
                addView(heading("Speech engine", 22f).apply { setPadding(0, dp(5), 0, 0) }, fullWidthWrap())
                ttsStatusView = TextView(this@MainActivity).apply {
                    text = "Speech engine initializing…"
                    textSize = 14f
                    setTextColor(CarbonPalette.amber)
                    setPadding(dp(14), dp(14), dp(14), dp(14))
                    background = carbonControl(this@MainActivity, CarbonPalette.line)
                }
                addView(ttsStatusView, fullWidthWrap().apply { topMargin = dp(14) })
                voiceButton = secondaryButton("VOICE: DEFAULT") { cycleVoice() }
                addView(voiceButton, fullWidthWrap().apply { topMargin = dp(12) })
                addView(secondaryButton("TEST VOICE") {
                    renderState(VoiceState.SPEAKING, "Testing voice")
                    speak("VIC audio playback is working.", RESPONSE_UTTERANCE_ID)
                }, fullWidthWrap().apply { topMargin = dp(12) })
            }, fullWidthWrap().apply { topMargin = dp(18) })
            addView(panel(17).apply {
                addView(kicker("Connectivity"), fullWidthWrap())
                addView(heading("Gateway and memory", 22f).apply { setPadding(0, dp(5), 0, 0) }, fullWidthWrap())
                systemDetailView = TextView(this@MainActivity).apply {
                    text = "Checking gateway, provider, tailnet, and memory…"
                    textSize = 14f
                    setTextColor(CarbonPalette.white)
                    setLineSpacing(dp(3).toFloat(), 1.15f)
                    setPadding(dp(14), dp(14), dp(14), dp(14))
                    background = carbonControl(this@MainActivity, CarbonPalette.line)
                }
                addView(systemDetailView, fullWidthWrap().apply { topMargin = dp(14) })
                addView(secondaryButton("REFRESH SYSTEM STATUS") { checkGatewayHealth(justEnrolled = false) }, fullWidthWrap().apply { topMargin = dp(14) })
                addView(secondaryButton("SEND TEST VIC CHECK-IN") { sendTestVicCheckIn() }, fullWidthWrap().apply { topMargin = dp(12) })
            }, fullWidthWrap().apply { topMargin = dp(14) })
            addView(panel(17).apply {
                addView(kicker("Reviewed self-improvement"), fullWidthWrap())
                addView(heading("Skill proposals", 22f).apply { setPadding(0, dp(5), 0, 0) }, fullWidthWrap())
                skillProposalStatusView = TextView(this@MainActivity).apply {
                    text = "Loading evidence-backed proposalsâ€¦"
                    textSize = 13f
                    setTextColor(CarbonPalette.muted)
                    setPadding(0, dp(12), 0, 0)
                }
                addView(skillProposalStatusView, fullWidthWrap())
                skillProposalContainer = LinearLayout(this@MainActivity).apply {
                    orientation = LinearLayout.VERTICAL
                }
                addView(skillProposalContainer, fullWidthWrap().apply { topMargin = dp(8) })
                addView(secondaryButton("REFRESH SKILLS") { loadSkillProposals() }, fullWidthWrap().apply { topMargin = dp(14) })
                addView(kicker("Active capability library").apply { setPadding(0, dp(20), 0, 0) }, fullWidthWrap())
                skillCatalogStatusView = TextView(this@MainActivity).apply {
                    text = "Loading approved skills…"
                    textSize = 13f
                    setTextColor(CarbonPalette.muted)
                    setPadding(0, dp(10), 0, 0)
                }
                addView(skillCatalogStatusView, fullWidthWrap())
                skillCatalogContainer = LinearLayout(this@MainActivity).apply { orientation = LinearLayout.VERTICAL }
                addView(skillCatalogContainer, fullWidthWrap().apply { topMargin = dp(8) })
                addView(kicker("Recent VIC skill use").apply { setPadding(0, dp(20), 0, 0) }, fullWidthWrap())
                skillUsageContainer = LinearLayout(this@MainActivity).apply { orientation = LinearLayout.VERTICAL }
                addView(skillUsageContainer, fullWidthWrap().apply { topMargin = dp(8) })
            }, fullWidthWrap().apply { topMargin = dp(14) })
        }

        pageViews.clear()
        pageViews[AppPage.FEED] = feedPage
        pageViews[AppPage.COMMAND] = commandPage
        pageViews[AppPage.TASKS] = tasksPage
        pageViews[AppPage.HISTORY] = historyPage
        pageViews[AppPage.SYSTEM] = systemPage
        content.addView(feedPage, fullWidthWrap())
        content.addView(commandPage, fullWidthWrap())
        content.addView(tasksPage, fullWidthWrap())
        content.addView(historyPage, fullWidthWrap())
        content.addView(systemPage, fullWidthWrap())

        feedNav.setOnClickListener { showPage(AppPage.FEED) }
        commandNav.setOnClickListener { showPage(AppPage.COMMAND) }
        tasksNav.setOnClickListener { showPage(AppPage.TASKS) }
        historyNav.setOnClickListener { showPage(AppPage.HISTORY) }
        systemNav.setOnClickListener { showPage(AppPage.SYSTEM) }

        rootScroll = ScrollView(this).apply {
            isFillViewport = true
            setBackgroundColor(CarbonPalette.black)
            addView(content)
        }
        return rootScroll
    }

    private fun actionButton(label: String) = Button(this).apply {
        text = label
        textSize = 12f
        typeface = Typeface.DEFAULT_BOLD
        minHeight = dp(52)
        setTextColor(CarbonPalette.black)
        background = carbonControl(this@MainActivity, CarbonPalette.teal, filled = true)
        stateListAnimator = null
    }

    private fun secondaryButton(label: String, action: () -> Unit) = Button(this).apply {
        text = label
        textSize = 11f
        typeface = Typeface.DEFAULT_BOLD
        minHeight = dp(50)
        setTextColor(CarbonPalette.white)
        background = carbonControl(this@MainActivity, CarbonPalette.line)
        stateListAnimator = null
        setOnClickListener { action() }
    }

    private fun fullWidthWrap() = LinearLayout.LayoutParams(
        ViewGroup.LayoutParams.MATCH_PARENT,
        ViewGroup.LayoutParams.WRAP_CONTENT,
    )

    private fun weightedButton() = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)

    private fun taskPanel(padding: Int = 20) = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(dp(padding), dp(padding), dp(padding), dp(padding))
        background = carbonPanel(this@MainActivity)
    }

    private fun taskKicker(text: String) = TextView(this).apply {
        this.text = text.uppercase(Locale.US)
        textSize = 10f
        typeface = Typeface.DEFAULT_BOLD
        letterSpacing = 0.17f
        setTextColor(CarbonPalette.teal)
    }

    private fun taskHeading(text: String, size: Float = 24f) = TextView(this).apply {
        this.text = text
        textSize = size
        typeface = Typeface.create("sans-serif", Typeface.NORMAL)
        setTextColor(CarbonPalette.white)
    }

    private fun handlePrimaryAction() {
        if (conversationActive) {
            if (conversationPaused) resumeConversationMode() else pauseConversationMode()
            return
        }
        when (voiceState) {
            VoiceState.LISTENING, VoiceState.STARTING -> {
                speechRecognizer?.stopListening()
                renderState(VoiceState.PROCESSING, "Finishing transcript")
            }
            VoiceState.PROCESSING -> cancelCurrentAction()
            VoiceState.SPEAKING -> {
                textToSpeech?.stop()
                ensurePermissionAndStart(correction = false)
            }
            VoiceState.READY, VoiceState.ERROR -> ensurePermissionAndStart(correction = false)
        }
    }

    private fun startFromWidgetIfRequested(intent: Intent?) {
        if (intent?.action == ACTION_CONFIRM_END_CONVERSATION) {
            intent.action = null
            showPage(AppPage.COMMAND)
            if (!conversationActive) restoreConversationSnapshot()
            if (conversationActive) showEndConversationConfirmation()
            return
        }
        if (intent?.action == ACTION_SOCIAL_SHIELD) {
            val openedPackage = intent.getStringExtra(EXTRA_BLOCKED_PACKAGE)
            intent.action = null
            showPage(AppPage.FEED)
            if (!openedPackage.isNullOrBlank()) showSocialShieldPrompt(openedPackage)
            return
        }
        if (intent?.action == ACTION_WIDGET_OPEN_FEED) {
            intent.action = null
            showPage(AppPage.FEED)
            return
        }
        if (intent?.action == ACTION_PIN_WIDGET) {
            intent.action = null
            showPage(AppPage.TASKS)
            requestFocusWidget()
            return
        }
        if (intent?.action == ACTION_WIDGET_OPEN_TASK) {
            intent.action = null
            currentTaskFilter = TaskFilter.TODAY
            showPage(AppPage.TASKS)
            loadTasks()
            return
        }
        if (BuildConfig.DEBUG && intent?.action == ACTION_VIC_TEST_CHECKIN) {
            intent.action = null
            sendTestVicCheckIn()
            return
        }
        if (intent?.action == ACTION_VIC_TALK) {
            intent.action = null
            showPage(AppPage.COMMAND)
            intent.getStringExtra(VicOutreachNotifications.EXTRA_BODY)?.takeIf { it.isNotBlank() }?.let {
                transcriptView.text = "VIC reached out:\n\n$it"
            }
            ensurePermissionAndStart(correction = false)
            return
        }
        if (intent?.action == ACTION_VIC_SHOW_PROGRESS) {
            intent.action = null
            showPage(AppPage.TASKS)
            loadTasks()
            return
        }
        if (intent?.action == ACTION_SCRIPTURE_REFLECTION) {
            intent.action = null
            val reference = intent.getStringExtra(EXTRA_PASSAGE_REFERENCE)
                ?.takeIf { it.isNotBlank() }
                ?: ScriptureResetModel.passageFor().reference
            val thoughts = intent.getStringExtra(EXTRA_SCRIPTURE_THOUGHTS).orEmpty().trim()
            showPage(AppPage.COMMAND)
            val display = if (thoughts.isBlank()) {
                "I read $reference in the CSB and want to talk through what stood out."
            } else {
                "My reflection on $reference: $thoughts"
            }
            submitText(ScriptureResetModel.conversationPrompt(reference, thoughts), display)
            return
        }
        if (intent?.action == ACTION_DAILY_CHECKIN) {
            intent.action = null
            startActivity(Intent(this, FocusResetActivity::class.java))
            return
        }
        if (intent?.action == ACTION_WIDGET_ADD_TASK) {
            intent.action = null
            showTaskCreationDialog()
            return
        }
        val requested = intent?.action == ACTION_WIDGET_TALK ||
            intent?.getBooleanExtra(EXTRA_AUTO_LISTEN, false) == true
        if (!requested) return
        intent.action = null
        intent.removeExtra(EXTRA_AUTO_LISTEN)
        ensurePermissionAndStart(correction = false)
    }

    private fun requestFocusWidget() {
        val manager = getSystemService(AppWidgetManager::class.java)
        if (!manager.isRequestPinAppWidgetSupported) {
            Toast.makeText(
                this,
                "Open your launcher widgets and choose VIC Focus.",
                Toast.LENGTH_LONG,
            ).show()
            return
        }
        val provider = ComponentName(this, VoiceWidgetProvider::class.java)
        val requested = manager.requestPinAppWidget(provider, null, null)
        if (!requested) {
            Toast.makeText(
                this,
                "Open your launcher widgets and choose VIC Focus.",
                Toast.LENGTH_LONG,
            ).show()
        }
    }

    private fun handleInterestCommand(text: String): Boolean {
        val topic = InterestCommands.followTopic(text) ?: return false
        val interest = InterestStore.follow(this, topic)
        val response = "Following ${interest.topic}. I added it to your private feed."
        lastTranscript = text
        lastResponse = response
        transcriptView.text = "You: $text\n\nVIC: $response"
        renderState(VoiceState.SPEAKING, "Interest followed")
        VoiceWidgetProvider.updateStatus(this, "Interest followed")
        if (currentPage == AppPage.FEED) loadMomentumFeed()
        speak(response, RESPONSE_UTTERANCE_ID)
        return true
    }

    private fun ensurePermissionAndStart(correction: Boolean) {
        pendingPermissionCorrection = correction
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
            if (correction) startRecognition(correction = true)
            else startConversationMode()
        } else {
            requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), REQUEST_MICROPHONE)
        }
    }

    private fun startRecognition(correction: Boolean) {
        textToSpeech?.stop()
        failedTranscript = null
        correctionMode = correction
        latestPartialTranscript = null

        if (!SpeechRecognizer.isOnDeviceRecognitionAvailable(this)) {
            showRecoverableError(
                "On-device speech recognition is unavailable. Install the offline English speech model in Android settings.",
                speakError = true,
            )
            return
        }

        if (speechRecognizer == null) {
            speechRecognizer = SpeechRecognizer.createOnDeviceSpeechRecognizer(this).apply {
                setRecognitionListener(recognitionListener)
            }
        }

        val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
            putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
            putExtra(RecognizerIntent.EXTRA_LANGUAGE, Locale.US.toLanguageTag())
            putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
            putExtra(RecognizerIntent.EXTRA_PREFER_OFFLINE, true)
            putExtra(RecognizerIntent.EXTRA_MAX_RESULTS, 3)
            putExtra(RecognizerIntent.EXTRA_SPEECH_INPUT_MINIMUM_LENGTH_MILLIS, 1_200L)
            putExtra(RecognizerIntent.EXTRA_SPEECH_INPUT_COMPLETE_SILENCE_LENGTH_MILLIS, 2_000L)
            putExtra(RecognizerIntent.EXTRA_SPEECH_INPUT_POSSIBLY_COMPLETE_SILENCE_LENGTH_MILLIS, 1_200L)
        }
        renderState(VoiceState.STARTING, "Starting on-device recognition")
        speechRecognizer?.startListening(intent)
    }

    private fun cancelCurrentAction() {
        if (conversationActive) {
            showEndConversationConfirmation()
            return
        }
        requestGeneration += 1
        correctionMode = false
        pendingCorrectionAfterSpeech = false
        speechRecognizer?.cancel()
        textToSpeech?.stop()
        renderState(VoiceState.READY, "Cancelled")
        VoiceWidgetProvider.updateStatus(this, "Ready")
    }

    private fun submitText(text: String, displayTranscript: String? = null) {
        val generation = ++requestGeneration
        val attachment = pendingAttachment
        renderState(VoiceState.PROCESSING, "Contacting gateway")
        VoiceWidgetProvider.updateStatus(this, "Processing")
        changeFloor("claim", "processing", text)

        GatewayClient.submitText(
            GatewaySettings.baseUrl(this),
            sessionId,
            text,
            DeviceCredentials.token(this),
            attachment?.let { listOf(it.id) } ?: emptyList(),
        ) { result ->
            runOnUiThread {
                if (generation != requestGeneration || isFinishing || isDestroyed) return@runOnUiThread
                result.fold(
                    onSuccess = { turn ->
                        if (pendingAttachment?.id == attachment?.id) pendingAttachment = null
                        val visibleTranscript = displayTranscript ?: turn.transcript
                        lastTranscript = visibleTranscript
                        lastResponse = turn.responseText
                        failedTranscript = null
                        pendingApproval = turn.approval
                        transcriptView.text = "You: $visibleTranscript\n\nVIC: ${turn.responseText}"
                        providerStatusView.text = "${turn.provider.uppercase(Locale.US)}  •  ACTIVE"
                        providerStatusView.setTextColor(CarbonPalette.teal)
                        memoryStatusView.text = "MEMORY ACTIVE"
                        memoryStatusView.setTextColor(CarbonPalette.teal)
                        if (
                            turn.provider == "deterministic-checkin" &&
                            turn.responseText.contains("all twelve answers", ignoreCase = true)
                        ) {
                            DailyCheckinScheduler.scheduleTomorrow(this)
                        }
                        renderState(
                            VoiceState.SPEAKING,
                            "Speaking ${speechRateLabel()} • ${turn.provider} • ${turn.processingMs} ms",
                        )
                        VoiceWidgetProvider.updateStatus(this, "Speaking")
                        VoiceWidgetProvider.refreshTasks(this)
                        if (currentPage == AppPage.TASKS) loadTasks()
                        if (currentPage == AppPage.FEED) loadMomentumFeed()
                        changeFloor("update", "speaking", turn.transcript, turn.responseText)
                        speak(turn.responseText, RESPONSE_UTTERANCE_ID)
                    },
                    onFailure = { error ->
                        failedTranscript = text
                        val message = "I couldn't reach the gateway. Your request is saved. Tap Retry when the connection is back."
                        transcriptView.text = "You: $text\n\nVIC: $message\n\n${error.message.orEmpty()}"
                        renderState(VoiceState.ERROR, "Gateway unavailable")
                        VoiceWidgetProvider.updateStatus(this, "Offline")
                        speak(message, ERROR_UTTERANCE_ID)
                    },
                )
            }
        }
    }

    private fun handlePendingApprovalSpeech(text: String): Boolean {
        val approval = pendingApproval ?: return false
        if (approval.tool == "rig.root_command") {
            val message = "Administrative actions require you to use the on-screen approval card. Voice approval is disabled."
            transcriptView.text = "You: $text\n\nVIC: $message"
            renderState(VoiceState.SPEAKING, "Physical approval required")
            speak(message, RESPONSE_UTTERANCE_ID)
            return true
        }
        val normalized = text.lowercase(Locale.US).replace(Regex("[^a-z ]"), " ").trim()
        val approve = normalized in setOf("approve", "approved", "yes", "yes approve", "confirm")
        val deny = normalized in setOf("deny", "denied", "no", "no deny", "reject", "cancel")
        when {
            approve -> decidePendingApproval(approve = true)
            deny -> decidePendingApproval(approve = false)
            else -> {
                val message = "A tool approval is pending. Say approve or deny."
                transcriptView.text = "You: $text\n\nVIC: $message"
                renderState(VoiceState.SPEAKING, "Awaiting approval")
                speak(message, RESPONSE_UTTERANCE_ID)
            }
        }
        return true
    }

    private fun startSharedEventStream() {
        val token = DeviceCredentials.token(this) ?: return
        eventSubscription?.close()
        val preferences = getSharedPreferences("voiceos_shared_events", MODE_PRIVATE)
        eventSubscription = GatewayClient.streamEvents(
            GatewaySettings.baseUrl(this),
            token,
            preferences.getLong("cursor", 0),
            onEvent = { event ->
                preferences.edit().putLong("cursor", event.id).apply()
                runOnUiThread {
                    if (isFinishing || isDestroyed) return@runOnUiThread
                    when (event.type) {
                        "conversation.turn" -> if (currentPage == AppPage.HISTORY) loadHistory()
                        "conversation.floor.changed" -> {
                            val floorValue = event.payload.optJSONObject("floor")
                            if (floorValue != null) {
                                val floor = GatewayClient.parseConversationFloor(floorValue)
                                val thisDevice = DeviceCredentials.deviceId(this)
                                if (floor.active && floor.holderDeviceId != thisDevice) {
                                    speechRecognizer?.cancel()
                                    textToSpeech?.stop()
                                    renderState(
                                        VoiceState.READY,
                                        "Conversation active on ${floor.holderDisplayName ?: "another device"}",
                                    )
                                }
                            }
                        }
                        "task.changed", "task.progress.updated", "daily_plan.proposed" -> {
                            VoiceWidgetProvider.refreshTasks(this)
                            if (currentPage == AppPage.TASKS) loadTasks()
                        }
                        "task.initiative.updated" -> {
                            VoiceWidgetProvider.refreshTasks(this)
                            if (currentPage == AppPage.TASKS) loadTasks()
                            val response = event.payload.optString("response_text")
                            val status = event.payload.optString("status", "updated")
                            if (response.isNotBlank()) {
                                transcriptView.text = "VIC proactive task work:\n\n$response"
                            }
                            renderState(VoiceState.READY, "Task work $status")
                        }
                        "agent.activity.updated", "agent.worker.updated" -> consumeAgentEvent(event)
                        "approval.proposed" -> {
                            pendingApproval = ApprovalRequest(
                                requestId = event.payload.optString("request_id"),
                                tool = event.payload.optString("tool"),
                                expiresAtUnix = event.payload.optLong("expires_at_unix", 0),
                                arguments = event.payload.optJSONObject("arguments") ?: org.json.JSONObject(),
                            )
                            if (pendingApproval?.tool == "rig.root_command") {
                                val arguments = pendingApproval!!.arguments
                                val command = arguments.optJSONArray("argv")?.toString() ?: "[]"
                                val rollback = arguments.optString("rollback", "No rollback information supplied")
                                transcriptView.text =
                                    "ADMINISTRATIVE APPROVAL\n\nExact command: $command\n\nRollback: $rollback\n\nThis grant expires and can only be used once."
                            }
                            renderState(VoiceState.READY, "Approval waiting")
                        }
                        "approval.decided" -> pendingApproval = null
                        "status.changed" -> checkGatewayHealth(justEnrolled = false)
                    }
                }
            },
            onClosed = { error ->
                if (error != null && !isFinishing && !isDestroyed) {
                    runOnUiThread {
                        gatewayView.text = "● RECONNECTING"
                        gatewayView.postDelayed(
                            { if (!isFinishing && !isDestroyed) startSharedEventStream() },
                            2_000,
                        )
                    }
                }
            },
        )
    }

    private fun changeFloor(
        action: String,
        phase: String,
        transcript: String? = null,
        response: String? = null,
    ) {
        GatewayClient.changeConversationFloor(
            baseUrl = GatewaySettings.baseUrl(this),
            request = ConversationFloorRequest(
                action = action,
                phase = phase,
                partialTranscript = transcript,
                responseText = response,
            ),
            deviceToken = DeviceCredentials.token(this),
        )
    }

    private fun decidePendingApproval(approve: Boolean) {
        val approval = pendingApproval ?: return
        val generation = ++requestGeneration
        speechRecognizer?.cancel()
        textToSpeech?.stop()
        renderState(
            VoiceState.PROCESSING,
            if (approve) "Approving ${approval.tool}" else "Denying ${approval.tool}",
        )
        GatewayClient.decideApproval(
            GatewaySettings.baseUrl(this),
            approval.requestId,
            approve,
            DeviceCredentials.token(this),
        ) { result ->
            runOnUiThread {
                if (generation != requestGeneration || isFinishing || isDestroyed) return@runOnUiThread
                result.fold(
                    onSuccess = { decision ->
                        pendingApproval = null
                        lastResponse = decision.responseText
                        transcriptView.append(
                            "\n\nDecision: ${if (approve) "Approved" else "Denied"}" +
                                "\n\nVIC: ${decision.responseText}"
                        )
                        renderState(VoiceState.SPEAKING, "Approval ${decision.status}")
                        speak(decision.responseText, RESPONSE_UTTERANCE_ID)
                    },
                    onFailure = { error ->
                        pendingApproval = null
                        val message = "The approval could not be completed. Request the action again."
                        transcriptView.append("\n\nVIC: $message\n${error.message.orEmpty()}")
                        renderState(VoiceState.ERROR, "Approval unavailable")
                        speak(message, ERROR_UTTERANCE_ID)
                    },
                )
            }
        }
    }

    private fun refreshAgentVisibility() {
        if (!::agentSummaryView.isInitialized || agentRecoveryInFlight) return
        val token = DeviceCredentials.token(this)
        if (token.isNullOrBlank()) {
            agentSummaryView.text = "PAIR THIS DEVICE TO VIEW LIVE ACTIVITY"
            agentSummaryView.setTextColor(CarbonPalette.amber)
            return
        }
        agentRecoveryInFlight = true
        agentSummaryView.text = "SYNCING RECENT AGENT ACTIVITY"
        agentSummaryView.setTextColor(CarbonPalette.amber)
        GatewayClient.getRecentEvents(GatewaySettings.baseUrl(this), token) { result ->
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                agentRecoveryInFlight = false
                result.fold(
                    onSuccess = { recovery ->
                        recovery.events.forEach(::consumeAgentEvent)
                        renderAgentVisibility()
                    },
                    onFailure = {
                        renderAgentVisibility()
                        if (agentVisibility.activities.isEmpty()) {
                            agentSummaryView.text = "ACTIVITY HISTORY UNAVAILABLE • LIVE STREAM RETRYING"
                            agentSummaryView.setTextColor(CarbonPalette.amber)
                        }
                    },
                )
            }
        }
    }

    private fun consumeAgentEvent(event: ClientEvent) {
        when (event.type) {
            "agent.activity.updated" -> {
                val phase = event.payload.optString("phase", "activity")
                agentVisibility.updateActivity(
                    AgentActivityItem(
                        eventId = event.id,
                        id = "${event.id}-$phase",
                        phase = phase,
                        label = event.payload.optString("label", "VIC is working").ifBlank { "VIC is working" },
                        detail = event.payload.optString("detail").takeIf { it.isNotBlank() }?.take(500),
                        sessionId = event.payload.optString("session_id").takeIf { it.isNotBlank() },
                    )
                )
            }
            "agent.worker.updated" -> {
                agentVisibility.updateWorker(
                    AgentWorkerItem(
                        eventId = event.id,
                        id = event.payload.optString("worker_id", event.id.toString()).ifBlank { event.id.toString() },
                        status = event.payload.optString("status", "running").ifBlank { "running" },
                        label = event.payload.optString("label", "Hermes background worker").ifBlank { "Hermes background worker" },
                        detail = event.payload.optString("detail").takeIf { it.isNotBlank() }?.take(500),
                        sessionId = event.payload.optString("session_id").takeIf { it.isNotBlank() },
                    )
                )
            }
            else -> return
        }
        renderAgentVisibility()
    }

    private fun renderAgentVisibility() {
        if (!::agentActivityContainer.isInitialized || !::agentWorkerContainer.isInitialized) return
        val activities = agentVisibility.activities
        val workers = agentVisibility.workers
        val runningWorkers = agentVisibility.runningWorkerCount()
        val latest = activities.firstOrNull()

        agentSummaryView.text = when {
            runningWorkers > 0 -> "VIC WORKING • $runningWorkers HERMES ${if (runningWorkers == 1) "SUBAGENT" else "SUBAGENTS"} ACTIVE"
            latest != null -> "LIVE FEED • ${latest.label.uppercase(Locale.US)}"
            else -> "READY • WAITING FOR WORK"
        }
        agentSummaryView.setTextColor(if (latest != null || runningWorkers > 0) CarbonPalette.teal else CarbonPalette.muted)
        agentActivityContainer.removeAllViews()
        if (activities.isEmpty()) {
            agentActivityContainer.addView(agentEventCard("READY", "No recent agent execution", "New work will appear here automatically.", CarbonPalette.muted))
        } else {
            activities.forEachIndexed { index, activity ->
                agentActivityContainer.addView(
                    agentEventCard(
                        activity.phase.replace('.', ' ').uppercase(Locale.US),
                        activity.label,
                        activity.detail,
                        CarbonPalette.teal,
                    ),
                    fullWidthWrap().apply { if (index > 0) topMargin = dp(7) },
                )
            }
        }

        agentWorkerStatusView.text = when {
            runningWorkers > 0 -> "$runningWorkers ${if (runningWorkers == 1) "SUBAGENT" else "SUBAGENTS"} RUNNING"
            workers.isNotEmpty() -> "${workers.size} RECENT ${if (workers.size == 1) "SUBAGENT" else "SUBAGENTS"}"
            else -> "NO SUBAGENTS OBSERVED"
        }
        agentWorkerStatusView.setTextColor(if (runningWorkers > 0) CarbonPalette.amber else CarbonPalette.muted)
        agentWorkerContainer.removeAllViews()
        if (workers.isEmpty()) {
            agentWorkerContainer.addView(agentEventCard("IDLE", "No delegated work", "Hermes workers will appear when VIC delegates a task.", CarbonPalette.muted))
        } else {
            workers.forEachIndexed { index, worker ->
                val accent = when (worker.status.lowercase(Locale.US)) {
                    "running" -> CarbonPalette.amber
                    "completed" -> CarbonPalette.teal
                    "failed" -> CarbonPalette.red
                    else -> CarbonPalette.muted
                }
                agentWorkerContainer.addView(
                    agentEventCard(worker.status.uppercase(Locale.US), worker.label, worker.detail, accent),
                    fullWidthWrap().apply { if (index > 0) topMargin = dp(7) },
                )
            }
        }
    }

    private fun agentEventCard(overline: String, title: String, detail: String?, accent: Int) =
        LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(13), dp(12), dp(13), dp(12))
            background = carbonControl(this@MainActivity, accent)
            addView(TextView(this@MainActivity).apply {
                text = overline
                textSize = 9f
                typeface = Typeface.DEFAULT_BOLD
                letterSpacing = 0.12f
                setTextColor(accent)
            }, fullWidthWrap())
            addView(TextView(this@MainActivity).apply {
                text = title
                textSize = 14f
                typeface = Typeface.DEFAULT_BOLD
                setTextColor(CarbonPalette.white)
                setPadding(0, dp(5), 0, 0)
            }, fullWidthWrap())
            detail?.takeIf { it.isNotBlank() }?.let { visibleDetail ->
                addView(TextView(this@MainActivity).apply {
                    text = visibleDetail
                    textSize = 12f
                    setTextColor(CarbonPalette.muted)
                    setPadding(0, dp(5), 0, 0)
                }, fullWidthWrap())
            }
        }

    private fun checkGatewayHealth(justEnrolled: Boolean) {
        val baseUrl = GatewaySettings.baseUrl(this)
        gatewayView.text = if (justEnrolled) "● ENROLLED" else "● CHECKING"
        gatewayView.setTextColor(CarbonPalette.amber)
        GatewayClient.getHealth(baseUrl, DeviceCredentials.token(this)) { result ->
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                result.fold(
                    onSuccess = { health ->
                        gatewayView.text = "● ONLINE"
                        gatewayView.setTextColor(CarbonPalette.teal)
                        providerStatusView.text = "${health.languageModel.uppercase(Locale.US)}  •  READY"
                        providerStatusView.setTextColor(CarbonPalette.teal)
                        memoryStatusView.text = "MEMORY ACTIVE"
                        memoryStatusView.setTextColor(CarbonPalette.teal)
                        systemStatusView.text = "Gateway active\nTailnet private\nMemory connected"
                        systemStatusView.setTextColor(CarbonPalette.white)
                        systemDetailView.text = "Gateway  •  ONLINE\nProvider  •  ${health.languageModel.uppercase(Locale.US)}\nTailnet  •  PRIVATE\nMemory  •  CONNECTED"
                        systemDetailView.setTextColor(CarbonPalette.white)
                        VoiceWidgetProvider.updateStatus(this, "Online")
                    },
                    onFailure = {
                        gatewayView.text = "● OFFLINE"
                        gatewayView.setTextColor(CarbonPalette.red)
                        providerStatusView.text = "Provider unavailable"
                        providerStatusView.setTextColor(CarbonPalette.red)
                        memoryStatusView.text = "MEMORY OFFLINE"
                        memoryStatusView.setTextColor(CarbonPalette.red)
                        systemStatusView.text = "Gateway offline\nTailnet unavailable"
                        systemStatusView.setTextColor(CarbonPalette.red)
                        systemDetailView.text = "Gateway  •  OFFLINE\nProvider  •  UNAVAILABLE\nTailnet  •  CHECK CONNECTION\nMemory  •  UNAVAILABLE"
                        systemDetailView.setTextColor(CarbonPalette.red)
                        VoiceWidgetProvider.updateStatus(this, "Offline")
                    },
                )
            }
        }
    }

    private fun handleEnrollment(enrollment: GatewayEnrollment?) {
        val code = enrollment?.code
        if (code == null) {
            checkGatewayHealth(enrollment != null)
            return
        }
        val baseUrl = enrollment.baseUrl
        val deviceName = "${Build.MANUFACTURER} ${Build.MODEL}".trim()
        gatewayView.text = "● PAIRING"
        gatewayView.setTextColor(CarbonPalette.amber)
        systemStatusView.text = "Pairing securely with ${GatewaySettings.displayName(this)}"
        GatewayClient.enroll(baseUrl, code, deviceName) { result ->
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                result.fold(
                    onSuccess = { credential ->
                        DeviceCredentials.save(
                            this,
                            credential.deviceId,
                            credential.deviceToken,
                        )
                        startSharedEventStream()
                        refreshAgentVisibility()
                        startVicOutreachConnection()
                        checkGatewayHealth(justEnrolled = true)
                    },
                    onFailure = { error ->
                        gatewayView.text = "● PAIRING FAILED"
                        gatewayView.setTextColor(CarbonPalette.red)
                        showRecoverableError(
                            "Secure device pairing failed. The enrollment code may have expired. ${error.message.orEmpty()}",
                            speakError = true,
                        )
                    },
                )
            }
        }
    }

    private fun startVicOutreachConnection() {
        if (DeviceCredentials.token(this).isNullOrBlank()) return
        try {
            startForegroundService(
                Intent(this, VicOutreachService::class.java)
                    .setAction(VicOutreachService.ACTION_START)
            )
        } catch (error: RuntimeException) {
            Toast.makeText(this, "VIC check-ins could not connect: ${error.message.orEmpty()}", Toast.LENGTH_LONG).show()
        }
    }

    private fun sendTestVicCheckIn() {
        val token = DeviceCredentials.token(this)
        if (token.isNullOrBlank()) {
            Toast.makeText(this, "Enroll this phone before testing VIC check-ins.", Toast.LENGTH_LONG).show()
            return
        }
        startVicOutreachConnection()
        Toast.makeText(this, "Asking the rig to send a VIC check-in…", Toast.LENGTH_SHORT).show()
        GatewayClient.createTestOutreach(GatewaySettings.baseUrl(this), token) { result ->
            runOnUiThread {
                result.fold(
                    onSuccess = { renderState(VoiceState.READY, "VIC check-in sent") },
                    onFailure = { error -> showRecoverableError("VIC check-in failed: ${error.message.orEmpty()}", speakError = false) },
                )
            }
        }
    }

    private fun openImagePicker() {
        startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "image/*"
            putExtra(Intent.EXTRA_MIME_TYPES, arrayOf("image/jpeg", "image/png", "image/webp"))
        }, REQUEST_IMAGE)
    }

    private fun openCamera() {
        val intent = Intent(android.provider.MediaStore.ACTION_IMAGE_CAPTURE)
        if (intent.resolveActivity(packageManager) == null) {
            Toast.makeText(this, "No camera app is available on this phone.", Toast.LENGTH_LONG).show()
            return
        }
        startActivityForResult(intent, REQUEST_CAMERA)
    }

    private fun uploadImageFromUri(uri: Uri) {
        val filename = DocumentInput.filename(contentResolver, uri)
        val mediaType = AttachmentInput.acceptedMediaType(filename, contentResolver.getType(uri))
            ?: run {
                showRecoverableError("Choose a JPEG, PNG, or WebP image.", speakError = false)
                return
            }
        val bytes = runCatching { DocumentInput.readBytes(contentResolver, uri) }.getOrElse { error ->
            showRecoverableError(error.message ?: "The image could not be read.", speakError = false)
            return
        }
        uploadImage(filename, mediaType, bytes)
    }

    private fun uploadCameraPreview(bitmap: Bitmap) {
        val bytes = ByteArrayOutputStream().use { output ->
            if (!bitmap.compress(Bitmap.CompressFormat.JPEG, 90, output)) {
                showRecoverableError("The camera image could not be encoded.", speakError = false)
                return
            }
            output.toByteArray()
        }
        uploadImage("camera-${System.currentTimeMillis()}.jpg", "image/jpeg", bytes)
    }

    private fun uploadImage(filename: String, mediaType: String, bytes: ByteArray) {
        try {
            AttachmentInput.requireUploadSize(bytes.size)
        } catch (error: IllegalArgumentException) {
            showRecoverableError(error.message ?: "Image is too large.", speakError = false)
            return
        }
        renderState(VoiceState.PROCESSING, "Uploading $filename")
        transcriptView.text = "Uploading image: $filename"
        GatewayClient.uploadAttachment(
            GatewaySettings.baseUrl(this),
            filename,
            mediaType,
            bytes,
            DeviceCredentials.token(this),
        ) { result ->
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                result.fold(
                    onSuccess = { attachment ->
                        pendingAttachment = attachment
                        val message = "${attachment.filename} is ready. Ask VIC about it in your next voice request."
                        transcriptView.text = "VIC: $message"
                        renderState(VoiceState.READY, "Image ready to send")
                    },
                    onFailure = { error ->
                        transcriptView.text = "VIC: I couldn't upload $filename. ${error.message.orEmpty()}"
                        renderState(VoiceState.ERROR, "Image upload failed")
                    },
                )
            }
        }
    }

    private fun openDocumentPicker() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "*/*"
            putExtra(
                Intent.EXTRA_MIME_TYPES,
                arrayOf(
                    "text/plain",
                    "text/markdown",
                    "text/csv",
                    "application/csv",
                    "application/json",
                ),
            )
        }
        startActivityForResult(intent, REQUEST_DOCUMENT)
    }

    private fun uploadDocument(uri: Uri, filename: String, mediaType: String, mode: String) {
        val bytes = runCatching { DocumentInput.readBytes(contentResolver, uri) }.getOrElse { error ->
            showRecoverableError(
                error.message ?: "The selected file could not be read.",
                speakError = true,
            )
            return
        }
        renderState(VoiceState.PROCESSING, "Uploading $filename")
        transcriptView.text = "Adding private knowledge: $filename"
        GatewayClient.uploadDocument(
            GatewaySettings.baseUrl(this),
            filename,
            mediaType,
            mode,
            bytes,
            DeviceCredentials.token(this),
        ) { result ->
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                result.fold(
                    onSuccess = { document ->
                        val purpose = if (document.mode == "profile") {
                            "available in every conversation"
                        } else {
                            "searched when relevant"
                        }
                        val message = "${document.filename} was added and will be $purpose."
                        transcriptView.text = "VIC: $message"
                        renderState(VoiceState.SPEAKING, "Knowledge added")
                        speak(message, RESPONSE_UTTERANCE_ID)
                    },
                    onFailure = { error ->
                        val message = "I couldn't add $filename. ${error.message.orEmpty()}"
                        transcriptView.text = "VIC: $message"
                        renderState(VoiceState.ERROR, "Upload failed")
                        speak(message, ERROR_UTTERANCE_ID)
                    },
                )
            }
        }
    }

    private fun repeatLastResponse() {
        val response = lastResponse ?: return
        textToSpeech?.stop()
        renderState(VoiceState.SPEAKING, "Repeating at ${speechRateLabel()}")
        speak(response, RESPONSE_UTTERANCE_ID)
    }

    private fun copyLastResponse() {
        val response = lastResponse ?: return
        val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        clipboard.setPrimaryClip(ClipData.newPlainText("VIC response", response))
        Toast.makeText(this, "VIC response copied", Toast.LENGTH_SHORT).show()
    }

    private fun cycleSpeechRate() {
        setSpeechRate(
            PlaybackSpeed.next(speechRate),
            announce = false,
            restartCurrentReply = voiceState == VoiceState.SPEAKING && !conversationActive,
        )
    }

    private fun startConversationMode() {
        conversationActive = true
        conversationPaused = false
        renderState(VoiceState.STARTING, "Starting Conversation Mode")
        try {
            startForegroundService(
                Intent(this, VICConversationService::class.java)
                    .setAction(VICConversationService.ACTION_START)
            )
        } catch (error: RuntimeException) {
            conversationActive = false
            showRecoverableError(
                "Conversation Mode could not start: ${error.message.orEmpty()}",
                speakError = false,
            )
        }
    }

    private fun stopConversationMode() {
        startService(
            Intent(this, VICConversationService::class.java)
                .setAction(VICConversationService.ACTION_STOP)
                .putExtra(
                    VICConversationService.EXTRA_STOP_REASON,
                    ConversationStopReason.USER_UI.wireValue,
                )
        )
        conversationActive = false
        conversationPaused = false
        renderState(VoiceState.READY, "Conversation ended")
    }

    private fun pauseConversationMode() {
        startService(
            Intent(this, VICConversationService::class.java)
                .setAction(VICConversationService.ACTION_PAUSE)
        )
        conversationPaused = true
        renderState(VoiceState.READY, "Conversation paused")
    }

    private fun resumeConversationMode() {
        startService(
            Intent(this, VICConversationService::class.java)
                .setAction(VICConversationService.ACTION_RESUME)
        )
        conversationPaused = false
        renderState(VoiceState.STARTING, "Resuming conversation")
    }

    private fun showEndConversationConfirmation() {
        AlertDialog.Builder(this)
            .setTitle("End this conversation?")
            .setMessage("Pause keeps the current session ready to resume. End closes it and clears any queued turn.")
            .setPositiveButton("End conversation") { _, _ -> stopConversationMode() }
            .setNeutralButton(if (conversationPaused) "Keep paused" else "Pause") { _, _ ->
                if (!conversationPaused) pauseConversationMode()
            }
            .setNegativeButton("Keep talking", null)
            .show()
    }

    private fun restoreConversationSnapshot() {
        val preferences = getSharedPreferences(VICConversationService.PREFERENCES, MODE_PRIVATE)
        conversationActive = preferences.getBoolean(VICConversationService.SNAPSHOT_ACTIVE, false)
        if (!conversationActive) return
        conversationPaused = preferences.getString(VICConversationService.SNAPSHOT_STATE, null) ==
            VICConversationService.STATE_PAUSED
        val transcript = preferences.getString(VICConversationService.SNAPSHOT_TRANSCRIPT, null)
        val response = preferences.getString(VICConversationService.SNAPSHOT_RESPONSE, null)
        val provider = preferences.getString(VICConversationService.SNAPSHOT_PROVIDER, null)
        if (!transcript.isNullOrBlank()) lastTranscript = transcript
        if (!response.isNullOrBlank()) lastResponse = response
        if (!transcript.isNullOrBlank() || !response.isNullOrBlank()) {
            transcriptView.text = listOfNotNull(
                transcript?.let { "You: $it" },
                response?.let { "VIC: $it" },
            ).joinToString("\n\n")
        }
        if (!provider.isNullOrBlank()) {
            providerStatusView.text = "${provider.uppercase(Locale.US)}  â€¢  ACTIVE"
        }
        when (preferences.getString(VICConversationService.SNAPSHOT_STATE, null)) {
            VICConversationService.STATE_LISTENING -> renderState(VoiceState.LISTENING, "Conversation listening")
            VICConversationService.STATE_PROCESSING -> renderState(VoiceState.PROCESSING, "VIC is thinking")
            VICConversationService.STATE_RECONNECTING -> renderState(VoiceState.STARTING, "Reconnecting automatically")
            VICConversationService.STATE_SPEAKING -> renderState(VoiceState.SPEAKING, "VIC is speaking")
            VICConversationService.STATE_PAUSED -> renderState(VoiceState.READY, "Conversation paused")
            else -> renderState(VoiceState.STARTING, "Conversation active")
        }
    }

    private fun handleSpeechRateCommand(text: String): Boolean {
        val requestedRate = PlaybackSpeed.resolveCommand(text, speechRate) ?: return false
        setSpeechRate(requestedRate, announce = true, restartCurrentReply = false)
        return true
    }

    private fun setSpeechRate(rate: Float, announce: Boolean, restartCurrentReply: Boolean) {
        speechRate = PlaybackSpeed.clamp(rate)
        getSharedPreferences(PLAYBACK_PREFERENCES, MODE_PRIVATE)
            .edit()
            .putFloat(SPEECH_RATE_KEY, speechRate)
            .commit()
        textToSpeech?.setSpeechRate(speechRate)
        speedButton.text = speechRateButtonLabel()
        speedButton.contentDescription =
            "Voice playback speed ${speechRateLabel()}. Tap to increase."

        val message = "Voice playback speed is now ${speechRateLabel()}."
        if (announce) {
            transcriptView.text = "You: Playback speed command\n\nVIC: $message"
            renderState(VoiceState.SPEAKING, "Playback ${speechRateLabel()}")
            speak(message, RESPONSE_UTTERANCE_ID)
        } else if (restartCurrentReply) {
            val response = lastResponse ?: return
            textToSpeech?.stop()
            renderState(VoiceState.SPEAKING, "Speaking ${speechRateLabel()}")
            speak(response, RESPONSE_UTTERANCE_ID)
        } else {
            statusView.text = "Playback ${speechRateLabel()}"
        }
    }

    private fun speechRateLabel(): String = PlaybackSpeed.label(speechRate)

    private fun speechRateButtonLabel() = PlaybackSpeed.buttonLabel(speechRate)

    private fun beginCorrection() {
        if (lastTranscript == null) return
        pendingCorrectionAfterSpeech = true
        if (textToSpeechReady) {
            renderState(VoiceState.SPEAKING, "Correction")
            speak("Say the complete corrected request.", CORRECTION_PROMPT_ID)
        } else {
            pendingCorrectionAfterSpeech = false
            ensurePermissionAndStart(correction = true)
        }
    }

    private fun retryLastRequest() {
        val text = failedTranscript ?: return
        transcriptView.text = "You: $text\n\nVIC: Retrying…"
        submitText(text)
    }

    private fun showPage(page: AppPage) {
        currentPage = page
        pageViews.forEach { (candidate, view) -> view.visibility = if (candidate == page) View.VISIBLE else View.GONE }
        navViews.forEach { (candidate, view) ->
            val active = candidate == page
            view.setTextColor(if (active) CarbonPalette.teal else CarbonPalette.muted)
            view.background = carbonControl(this, if (active) CarbonPalette.teal else CarbonPalette.line)
            view.alpha = if (active) 1f else 0.72f
        }
        rootScroll.post { rootScroll.smoothScrollTo(0, 0) }
        if (page == AppPage.FEED) loadMomentumFeed()
        if (page == AppPage.COMMAND) refreshAgentVisibility()
        if (page == AppPage.TASKS) loadTasks()
        if (page == AppPage.HISTORY) loadHistory()
        if (page == AppPage.SYSTEM) {
            checkGatewayHealth(justEnrolled = false)
            loadSkillProposals()
        }
    }

    private fun loadMomentumFeed() {
        if (!::feedStatusView.isInitialized) return
        val cached = TaskWidgetStore.load(this)
        latestAiUpdates = AiUpdateStore.load(this)
        renderMomentumFeed(cached)
        feedStatusView.text = if (cached.isEmpty()) "Refreshing your private cards…" else "Refreshing • showing saved tasks"
        feedStatusView.setTextColor(CarbonPalette.muted)
        refreshAiUpdates()
        GatewayClient.getTasks(
            GatewaySettings.baseUrl(this),
            DeviceCredentials.token(this),
            limit = 100,
        ) { result ->
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                result.fold(
                    onSuccess = { tasks ->
                        latestTasks = tasks
                        TaskWidgetStore.save(this, tasks)
                        renderMomentumFeed(tasks)
                    },
                    onFailure = {
                        renderMomentumFeed(cached)
                        feedStatusView.text = if (cached.isEmpty()) {
                            "Connect VIC to load your tasks. Your followed interests are still available."
                        } else {
                            "Offline • using your saved private feed"
                        }
                        feedStatusView.setTextColor(CarbonPalette.amber)
                    },
                )
            }
        }
    }

    private fun renderMomentumFeed(tasks: List<VoiceTask>) {
        if (!::feedContainer.isInitialized) return
        latestTasks = tasks
        val interests = InterestStore.list(this)
        val cards = MomentumFeedModel.build(tasks, interests)
        val aiUpdates = AiUpdateModel.select(latestAiUpdates)
        feedContainer.removeAllViews()
        feedStatusView.text = when {
            cards.isEmpty() && aiUpdates.isEmpty() -> "Your feed is clear. Follow an interest or add one small task."
            else -> "${cards.size} focus cards • ${aiUpdates.size} official AI updates"
        }
        feedStatusView.setTextColor(if (cards.isEmpty() && aiUpdates.isEmpty()) CarbonPalette.green else CarbonPalette.teal)
        var aiUpdatesRendered = false
        cards.forEachIndexed { index, card ->
            renderMomentumCard(index, card, tasks, interests)
            if (index == 0) {
                renderAiUpdates(aiUpdates)
                aiUpdatesRendered = true
            }
        }
        if (!aiUpdatesRendered) renderAiUpdates(aiUpdates)
        feedContainer.addView(taskPanel(14).apply {
            addView(taskKicker("YOU'RE CAUGHT UP"), fullWidthWrap())
            addView(taskHeading("The feed ends here", 20f).apply { setPadding(0, dp(5), 0, 0) }, fullWidthWrap())
            addView(TextView(this@MainActivity).apply {
                text = "You saw the latest official AI updates. Start a priority, talk with VIC, or leave the phone—there is no endless feed waiting below."
                textSize = 13f
                setTextColor(CarbonPalette.muted)
                setPadding(0, dp(7), 0, 0)
            }, fullWidthWrap())
        }, fullWidthWrap().apply { topMargin = dp(10) })
    }

    private fun refreshAiUpdates() {
        if (aiUpdatesRefreshing) return
        if (latestAiUpdates.isNotEmpty() && AiUpdateStore.isFresh(this)) return
        aiUpdatesRefreshing = true
        AiUpdateRepository.refresh { result ->
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                aiUpdatesRefreshing = false
                result.onSuccess { updates ->
                    latestAiUpdates = updates
                    AiUpdateStore.save(this, updates)
                }
                renderMomentumFeed(latestTasks)
                if (result.isFailure && latestAiUpdates.isEmpty()) {
                    feedStatusView.text = "Official AI sources are unavailable • your focus cards still work"
                    feedStatusView.setTextColor(CarbonPalette.amber)
                }
            }
        }
    }

    private fun renderAiUpdates(updates: List<AiUpdate>) {
        feedContainer.addView(taskPanel(14).apply {
            addView(taskKicker("AI UPDATES • OFFICIAL SOURCES"), fullWidthWrap())
            addView(taskHeading("What changed in AI", 20f).apply { setPadding(0, dp(5), 0, 0) }, fullWidthWrap())
            addView(TextView(this@MainActivity).apply {
                text = if (updates.isEmpty() && aiUpdatesRefreshing) {
                    "Checking OpenAI, Google DeepMind, Hugging Face, and official release videos…"
                } else if (updates.isEmpty()) {
                    "No fresh official updates are available right now."
                } else {
                    "Four fresh items, then it stops. Every article can be read aloud inside OV."
                }
                textSize = 12f
                setTextColor(CarbonPalette.muted)
                setPadding(0, dp(7), 0, 0)
            }, fullWidthWrap())
        }, fullWidthWrap().apply { topMargin = dp(10) })
        updates.forEach { update -> renderAiUpdateCard(update) }
    }

    private fun renderAiUpdateCard(update: AiUpdate) {
        val kind = when (update.kind) {
            AiUpdateKind.VIDEO -> "AI VIDEO"
            AiUpdateKind.LAUNCH -> "AI LAUNCH"
            AiUpdateKind.REPORT -> "AI REPORT"
            AiUpdateKind.NEWS -> "AI NEWS"
        }
        feedContainer.addView(taskPanel(15).apply {
            addView(taskKicker("$kind • ${update.source.uppercase()} • ${AiUpdateModel.ageLabel(update.publishedEpochSeconds)}"), fullWidthWrap())
            addView(taskHeading(update.title, 20f).apply { setPadding(0, dp(5), 0, 0) }, fullWidthWrap())
            addView(TextView(this@MainActivity).apply {
                text = update.summary
                textSize = 13f
                setTextColor(CarbonPalette.muted)
                setLineSpacing(dp(2).toFloat(), 1.12f)
                setPadding(0, dp(7), 0, 0)
            }, fullWidthWrap())
            addView(secondaryButton(if (update.kind == AiUpdateKind.VIDEO) "WATCH OR LISTEN IN OV" else "READ OR LISTEN IN OV") {
                startActivity(Intent(this@MainActivity, AiReaderActivity::class.java).apply {
                    putExtra(AiReaderActivity.EXTRA_URL, update.readerUrl)
                    putExtra(AiReaderActivity.EXTRA_TITLE, update.title)
                    putExtra(AiReaderActivity.EXTRA_SOURCE, update.source)
                    putExtra(AiReaderActivity.EXTRA_SUMMARY, update.summary)
                })
            }, fullWidthWrap().apply { topMargin = dp(10) })
        }, fullWidthWrap().apply { topMargin = dp(8) })
    }

    private fun renderMomentumCard(
        index: Int,
        card: MomentumCard,
        tasks: List<VoiceTask>,
        interests: List<VicInterest>,
    ) {
        val task = card.taskId?.let { id -> tasks.firstOrNull { it.id == id } }
        val interest = card.interestId?.let { id -> interests.firstOrNull { it.id == id } }
        val label = when (card.kind) {
            MomentumCardKind.PRIORITY -> "DO THIS NOW"
            MomentumCardKind.TASK -> "NEXT SMALL MOVE"
            MomentumCardKind.VIC_PREPARED -> "VIC IS WORKING"
            MomentumCardKind.REVIEW -> "READY FOR YOU"
            MomentumCardKind.INTEREST -> "YOUR INTEREST"
            MomentumCardKind.WIN -> "WIN • MOMENTUM KEPT"
        }
        val cardView = taskPanel(if (index == 0) 20 else 16).apply {
            addView(taskKicker(label), fullWidthWrap())
            addView(taskHeading(card.title, if (index == 0) 26f else 21f).apply { setPadding(0, dp(6), 0, 0) }, fullWidthWrap())
            addView(TextView(this@MainActivity).apply {
                text = card.body
                textSize = if (index == 0) 15f else 13f
                setTextColor(if (card.kind == MomentumCardKind.INTEREST) CarbonPalette.teal else CarbonPalette.muted)
                setLineSpacing(dp(2).toFloat(), 1.13f)
                setPadding(0, dp(8), 0, 0)
            }, fullWidthWrap())
            when (card.kind) {
                MomentumCardKind.PRIORITY, MomentumCardKind.TASK -> if (task != null) {
                    addView(actionButton(if (task.status == "active") "OPEN TASK" else "START 5").apply {
                        setOnClickListener {
                            if (task.status == "active") {
                                currentTaskFilter = TaskFilter.TODAY
                                showPage(AppPage.TASKS)
                            } else {
                                updateTaskStatus(task, "active")
                            }
                        }
                    }, fullWidthWrap().apply { topMargin = dp(11) })
                }
                MomentumCardKind.VIC_PREPARED -> addView(secondaryButton("SHOW PROGRESS") {
                    currentTaskFilter = TaskFilter.VIC_WORKING
                    showPage(AppPage.TASKS)
                }, fullWidthWrap().apply { topMargin = dp(11) })
                MomentumCardKind.REVIEW -> addView(actionButton("REVIEW WITH VIC").apply {
                    setOnClickListener {
                        currentTaskFilter = TaskFilter.REVIEW
                        showPage(AppPage.TASKS)
                    }
                }, fullWidthWrap().apply { topMargin = dp(11) })
                MomentumCardKind.INTEREST -> if (interest != null) {
                    val actions = LinearLayout(this@MainActivity).apply { orientation = LinearLayout.HORIZONTAL }
                    actions.addView(actionButton("ASK VIC").apply {
                        setOnClickListener {
                            showPage(AppPage.COMMAND)
                            submitText("Give me one useful, concise idea about my interest in ${interest.topic}, connected to my current priorities.")
                        }
                    }, weightedButton())
                    actions.addView(secondaryButton("UNFOLLOW") {
                        InterestStore.unfollow(this@MainActivity, interest.id)
                        loadMomentumFeed()
                    }, weightedButton().apply { marginStart = dp(7) })
                    addView(actions, fullWidthWrap().apply { topMargin = dp(11) })
                }
                MomentumCardKind.WIN -> Unit
            }
        }
        feedContainer.addView(cardView, fullWidthWrap().apply { topMargin = dp(10) })
    }

    private fun showFollowInterestDialog() {
        val input = EditText(this).apply {
            hint = "Example: woodworking, restaurant design, AI agents"
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
            setSingleLine(true)
            setPadding(dp(20), dp(10), dp(20), dp(10))
        }
        AlertDialog.Builder(this)
            .setTitle("Follow an interest")
            .setMessage("Only topics you choose appear in your private feed.")
            .setView(input)
            .setNegativeButton("Cancel", null)
            .setPositiveButton("Follow") { _, _ ->
                val topic = input.text.toString().trim()
                if (topic.isBlank()) {
                    Toast.makeText(this, "Enter an interest to follow", Toast.LENGTH_SHORT).show()
                } else {
                    InterestStore.follow(this, topic)
                    loadMomentumFeed()
                }
            }
            .show()
    }

    @Suppress("DEPRECATION")
    private fun showSocialShieldSetup() {
        val launcherIntent = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
        val knownSignals = listOf(
            "instagram", "facebook", "tiktok", "youtube", "reddit", "twitter",
            "threads", "snapchat", "linkedin", "pinterest", "tumblr",
        )
        val installed = packageManager.queryIntentActivities(launcherIntent, 0)
            .map { info ->
                val packageName = info.activityInfo.packageName
                Triple(packageName, info.loadLabel(packageManager).toString(), info)
            }
            .filter { (packageName, label, _) ->
                packageName != this.packageName && knownSignals.any {
                    packageName.contains(it, ignoreCase = true) || label.contains(it, ignoreCase = true)
                }
            }
            .distinctBy { it.first }
            .sortedBy { it.second.lowercase(Locale.US) }
        if (installed.isEmpty()) {
            Toast.makeText(this, "No common social apps were found.", Toast.LENGTH_LONG).show()
            return
        }
        val selected = SocialShieldStore.packages(this).toMutableSet()
        val labels = installed.map { it.second }.toTypedArray()
        val checked = installed.map { it.first in selected }.toBooleanArray()
        AlertDialog.Builder(this)
            .setTitle("Choose social apps")
            .setMessage("When a priority is open, VIC will offer Start 5 before these apps. Continue anyway always remains available.")
            .setMultiChoiceItems(labels, checked) { _, which, isChecked ->
                val packageName = installed[which].first
                if (isChecked) selected += packageName else selected -= packageName
            }
            .setNegativeButton("Cancel", null)
            .setPositiveButton("Save and enable") { _, _ ->
                SocialShieldStore.setPackages(this, selected)
                AlertDialog.Builder(this)
                    .setTitle("Enable VIC Social Shield")
                    .setMessage("Android requires you to turn on VIC Social Shield in Accessibility. It reads only which app opens; it does not read screen content.")
                    .setNegativeButton("Later", null)
                    .setPositiveButton("Open settings") { _, _ ->
                        startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))
                    }
                    .show()
            }
            .show()
    }

    @Suppress("DEPRECATION")
    private fun showSocialShieldPrompt(openedPackage: String) {
        val task = FocusWidgetModel.select(TaskWidgetStore.load(this)).primary ?: return
        val label = runCatching {
            val info = packageManager.getApplicationInfo(openedPackage, 0)
            packageManager.getApplicationLabel(info).toString()
        }.getOrDefault("that app")
        AlertDialog.Builder(this)
            .setTitle("Before $label")
            .setMessage("You chose “${task.title}.” Give it five minutes first?")
            .setNegativeButton("Stay with VIC", null)
            .setPositiveButton("Start 5") { _, _ -> updateTaskStatus(task, "active") }
            .setNeutralButton("Continue 10 min") { _, _ ->
                SocialShieldStore.allowTemporarily(this, openedPackage)
                packageManager.getLaunchIntentForPackage(openedPackage)?.let(::startActivity)
            }
            .show()
    }

    private fun loadTasks() {
        if (!::taskStatusView.isInitialized) return
        taskStatusView.text = "Loading projects, ownership, and next actions…"
        taskStatusView.setTextColor(CarbonPalette.muted)
        GatewayClient.getProjects(
            GatewaySettings.baseUrl(this),
            DeviceCredentials.token(this),
        ) { projectResult ->
            latestProjects = projectResult.getOrDefault(emptyList())
            GatewayClient.getTasks(
                GatewaySettings.baseUrl(this),
                DeviceCredentials.token(this),
                limit = 100,
            ) { result ->
                runOnUiThread {
                    if (isFinishing || isDestroyed) return@runOnUiThread
                    result.fold(
                        onSuccess = { tasks ->
                            TaskWidgetStore.save(this, tasks)
                            VoiceWidgetProvider.updateStatus(this, "Online")
                            renderTasks(tasks)
                            ensureWeeklyTaskInstances(tasks)
                        },
                        onFailure = { error ->
                            val cached = TaskWidgetStore.load(this)
                            if (cached.isNotEmpty()) {
                                taskStatusView.text = "Showing cached tasks • sync unavailable"
                                taskStatusView.setTextColor(CarbonPalette.amber)
                                renderTasks(cached, preserveStatus = true)
                            } else {
                                taskContainer.removeAllViews()
                                taskStatusView.text = "Tasks are unavailable.\n${error.message.orEmpty()}"
                                taskStatusView.setTextColor(CarbonPalette.red)
                            }
                        },
                    )
                }
            }
        }
    }

    private fun renderTasks(tasks: List<VoiceTask>, preserveStatus: Boolean = false) {
        latestTasks = tasks
        taskContainer.removeAllViews()
        val openTasks = tasks.filter { it.status !in setOf("completed", "cancelled") }
        if (currentTaskFilter == TaskFilter.PROJECTS) {
            renderProjects(tasks)
            if (!preserveStatus) {
                taskStatusView.text = "${latestProjects.count { it.status == "active" }} active projects • ${openTasks.size} open tasks"
                taskStatusView.setTextColor(CarbonPalette.teal)
            }
            return
        }
        if (currentTaskFilter == TaskFilter.WINS) {
            renderWins(tasks)
            if (!preserveStatus) {
                val completed = tasks.count { it.status == "completed" }
                taskStatusView.text = if (completed == 0) "No pressure. Starting and returning both build momentum." else "$completed recent wins • progress never resets"
                taskStatusView.setTextColor(if (completed == 0) CarbonPalette.muted else CarbonPalette.green)
            }
            return
        }

        val visibleTasks = when (currentTaskFilter) {
            TaskFilter.TODAY -> openTasks.sortedWith(
                compareBy<VoiceTask> { if (it.status == "active") 0 else 1 }
                    .thenBy { if (it.progressLane == "needs_me") 0 else 1 }
                    .thenBy { when (it.importance) { "high" -> 0; "normal" -> 1; else -> 2 } }
                    .thenBy { it.estimatedMinutes },
            )
            TaskFilter.VIC_WORKING -> openTasks.filter { it.progressLane == "vic_working" }
            TaskFilter.REVIEW -> openTasks.filter { it.progressLane == "review" }
            TaskFilter.PROJECTS, TaskFilter.WINS -> emptyList()
        }
        if (!preserveStatus) {
            taskStatusView.text = if (openTasks.isEmpty()) {
                "You’re clear. Capture one small win or let VIC prepare the next project."
            } else {
                when (currentTaskFilter) {
                    TaskFilter.TODAY -> "All ${openTasks.size} unfinished tasks • VIC recommends the first one"
                    TaskFilter.VIC_WORKING -> "${visibleTasks.size} work packets with VIC • you can keep your focus"
                    TaskFilter.REVIEW -> "${visibleTasks.size} decisions waiting • approve only when ready"
                    else -> "${visibleTasks.size} shown • ${openTasks.size} open"
                }
            }
            taskStatusView.setTextColor(if (openTasks.isEmpty()) CarbonPalette.green else CarbonPalette.teal)
        }
        if (visibleTasks.isEmpty() && openTasks.isNotEmpty()) {
            taskContainer.addView(taskPanel(13).apply {
                addView(taskKicker("Nothing waiting here"), fullWidthWrap())
                addView(TextView(this@MainActivity).apply {
                    text = if (currentTaskFilter == TaskFilter.VIC_WORKING) "VIC has no active work packets." else "You have no reviews waiting. Keep your attention on Today."
                    textSize = 14f
                    setTextColor(CarbonPalette.muted)
                    setPadding(0, dp(8), 0, 0)
                }, fullWidthWrap())
            }, fullWidthWrap().apply { topMargin = dp(9) })
        }
        visibleTasks.forEachIndexed { index, task ->
            renderTaskCard(task, featured = currentTaskFilter == TaskFilter.TODAY && index == 0)
        }
    }

    private fun renderTaskCard(task: VoiceTask, featured: Boolean = false) {
        val card = taskPanel(if (featured) 17 else 13)
        val projectTitle = latestProjects.firstOrNull { it.id == task.projectId }?.title
        val weeklyLabel = WeeklyTaskStore.labelForTask(this, task.id)
        val marker = when {
            featured -> "DO THIS NOW • ${task.estimatedMinutes} MIN"
            task.status == "active" -> "▶ ACTIVE"
            task.status == "blocked" -> "! BLOCKED"
            else -> "○ READY"
        }
        card.addView(taskKicker(listOfNotNull(marker, projectTitle).joinToString(" • ")), fullWidthWrap())
        card.addView(taskHeading(task.title, if (featured) 22f else 19f).apply { setPadding(0, dp(5), 0, 0) }, fullWidthWrap())
        if (weeklyLabel != null) card.addView(TextView(this).apply {
            text = "↻  $weeklyLabel"
            textSize = 12f
            typeface = Typeface.DEFAULT_BOLD
            setTextColor(CarbonPalette.purple)
            setPadding(0, dp(7), 0, 0)
        }, fullWidthWrap())
        card.addView(TextView(this).apply {
            val stepProgress = if (task.totalSteps > 0) "${task.completedSteps}/${task.totalSteps} steps" else "VIC is preparing the breakdown"
            val blockerText = if (task.openBlockers > 0) " • ${task.openBlockers} blocked" else ""
            text = "${task.observableOutcome}\n$stepProgress$blockerText • VIC ${task.vicStatus.replace('_', ' ')}"
            textSize = 13f
            setTextColor(CarbonPalette.muted)
            setLineSpacing(dp(2).toFloat(), 1.12f)
            setPadding(0, dp(7), 0, 0)
        }, fullWidthWrap())
        val handoffText = when (task.progressLane) {
            "needs_me" -> "YOUR NEXT ACTION\n${task.nextUserAction.ifBlank { "Review this task with VIC" }}"
            "vic_working" -> "VIC IS WORKING\n${task.nextVicAction.ifBlank { "Continue safe preparation work" }}"
            "review" -> "READY FOR REVIEW\n${task.nextUserAction.ifBlank { "Review VIC's latest work" }}"
            else -> "SHARED WORK\n${task.nextUserAction.ifBlank { "Ask VIC to choose and prepare the first physical step" }}"
        }
        card.addView(TextView(this).apply {
            text = handoffText
            textSize = 12f
            setTextColor(if (task.progressLane == "review") CarbonPalette.amber else CarbonPalette.teal)
            setPadding(dp(11), dp(10), dp(11), dp(10))
            background = carbonControl(this@MainActivity, CarbonPalette.line)
        }, fullWidthWrap().apply { topMargin = dp(9) })
        task.steps.take(5).forEach { step ->
            card.addView(TextView(this).apply {
                val mark = if (step.status == "completed") "✓" else if (step.status == "active") "▶" else "○"
                text = "$mark  ${step.title}  •  ${step.owner.uppercase(Locale.US)}"
                textSize = 11f
                setTextColor(if (step.status == "completed") CarbonPalette.green else CarbonPalette.muted)
                setPadding(0, dp(6), 0, 0)
            }, fullWidthWrap())
        }
        val actions = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
        }
        if (task.status != "active") {
            actions.addView(secondaryButton(if (task.estimatedMinutes <= 5) "START 5" else "START") { updateTaskStatus(task, "active") }, weightedButton())
        }
        actions.addView(
            actionButton("DONE + WIN").apply { setOnClickListener { confirmTaskCompletion(task) } },
            weightedButton().apply { if (task.status != "active") marginStart = dp(7) },
        )
        card.addView(actions, fullWidthWrap().apply { topMargin = dp(11) })
        taskContainer.addView(card, fullWidthWrap().apply { topMargin = dp(9) })
    }

    private fun renderProjects(tasks: List<VoiceTask>) {
        val activeProjects = latestProjects.filter { it.status == "active" }
        if (activeProjects.isEmpty()) {
            taskContainer.addView(taskPanel(15).apply {
                addView(taskKicker("Turn outcomes into projects"), fullWidthWrap())
                addView(taskHeading("Give the work a finish line", 20f).apply { setPadding(0, dp(6), 0, 0) }, fullWidthWrap())
                addView(TextView(this@MainActivity).apply {
                    text = "A project holds the outcome. VIC breaks it into owned steps and brings the pieces back together."
                    textSize = 13f
                    setTextColor(CarbonPalette.muted)
                    setPadding(0, dp(8), 0, 0)
                }, fullWidthWrap())
                addView(actionButton("CREATE FIRST PROJECT").apply { setOnClickListener { showProjectCreationDialog() } }, fullWidthWrap().apply { topMargin = dp(12) })
            }, fullWidthWrap().apply { topMargin = dp(9) })
        }
        activeProjects.forEach { project ->
            val projectTasks = tasks.filter { it.projectId == project.id && it.status != "cancelled" }
            val completed = projectTasks.count { it.status == "completed" }
            val total = projectTasks.size
            val percent = if (total == 0) 0 else (completed * 100 / total)
            taskContainer.addView(taskPanel(15).apply {
                addView(taskKicker("PROJECT • $percent% COMPLETE"), fullWidthWrap())
                addView(taskHeading(project.title, 21f).apply { setPadding(0, dp(5), 0, 0) }, fullWidthWrap())
                addView(TextView(this@MainActivity).apply {
                    text = if (total == 0) "No tasks yet. Let VIC make the first small move." else "$completed of $total tasks complete • ${projectTasks.count { it.status !in setOf("completed", "cancelled") }} open"
                    textSize = 12f
                    setTextColor(if (percent == 100 && total > 0) CarbonPalette.green else CarbonPalette.muted)
                    setPadding(0, dp(7), 0, 0)
                }, fullWidthWrap())
                projectTasks.filter { it.status !in setOf("completed", "cancelled") }.take(4).forEach { task ->
                    addView(TextView(this@MainActivity).apply {
                        val owner = when (task.progressLane) {
                            "needs_me" -> "YOU"
                            "vic_working" -> "VIC"
                            "review" -> "REVIEW"
                            else -> "SHARED"
                        }
                        text = "○  ${task.title}  •  $owner"
                        textSize = 12f
                        setTextColor(CarbonPalette.white)
                        setPadding(0, dp(7), 0, 0)
                    }, fullWidthWrap())
                }
                addView(secondaryButton("+ TASK IN THIS PROJECT") { showTaskCreationDialog(project) }, fullWidthWrap().apply { topMargin = dp(12) })
            }, fullWidthWrap().apply { topMargin = dp(9) })
        }
        val looseTasks = tasks.filter { it.projectId == null && it.status !in setOf("completed", "cancelled") }
        if (looseTasks.isNotEmpty()) {
            taskContainer.addView(taskPanel(13).apply {
                addView(taskKicker("INBOX • ${looseTasks.size} UNFILED"), fullWidthWrap())
                addView(TextView(this@MainActivity).apply {
                    text = "These tasks are safe, but assigning them to a project will give VIC better context and a finish line."
                    textSize = 12f
                    setTextColor(CarbonPalette.muted)
                    setPadding(0, dp(7), 0, 0)
                }, fullWidthWrap())
            }, fullWidthWrap().apply { topMargin = dp(9) })
        }
    }

    private fun renderWins(tasks: List<VoiceTask>) {
        val completed = tasks.filter { it.status == "completed" }
        val momentum = completed.sumOf { 2 + it.completedSteps.coerceAtMost(3) }
        taskContainer.addView(taskPanel(17).apply {
            addView(taskKicker("MOMENTUM • NEVER RESETS"), fullWidthWrap())
            addView(taskHeading("$momentum points", 28f).apply { setPadding(0, dp(5), 0, 0) }, fullWidthWrap())
            addView(TextView(this@MainActivity).apply {
                text = "Starting, returning, unblocking, and finishing count. A difficult day never erases your progress."
                textSize = 13f
                setTextColor(CarbonPalette.muted)
                setPadding(0, dp(8), 0, 0)
            }, fullWidthWrap())
        }, fullWidthWrap().apply { topMargin = dp(9) })
        completed.take(10).forEach { task ->
            taskContainer.addView(taskPanel(11).apply {
                addView(taskKicker("✓ WIN • +${2 + task.completedSteps.coerceAtMost(3)}"), fullWidthWrap())
                addView(taskHeading(task.title, 17f).apply { setPadding(0, dp(4), 0, 0) }, fullWidthWrap())
                addView(TextView(this@MainActivity).apply {
                    text = task.observableOutcome
                    textSize = 12f
                    setTextColor(CarbonPalette.muted)
                    setPadding(0, dp(5), 0, 0)
                }, fullWidthWrap())
            }, fullWidthWrap().apply { topMargin = dp(7) })
        }
    }

    private fun setTaskFilter(filter: TaskFilter) {
        currentTaskFilter = filter
        renderTasks(latestTasks)
    }

    private fun updateTaskStatus(task: VoiceTask, status: String) {
        taskStatusView.text = if (status == "completed") "Completing ${task.title}…" else "Starting ${task.title}…"
        taskStatusView.setTextColor(CarbonPalette.amber)
        if (::feedStatusView.isInitialized) {
            feedStatusView.text = if (status == "completed") "Saving your win…" else "Starting five minutes…"
            feedStatusView.setTextColor(CarbonPalette.amber)
        }
        GatewayClient.updateTaskStatus(
            GatewaySettings.baseUrl(this),
            task.id,
            status,
            DeviceCredentials.token(this),
        ) { result ->
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                result.fold(
                    onSuccess = { updated ->
                        TaskWidgetStore.replace(this, updated)
                        val weekly = if (status == "completed") WeeklyTaskStore.forTask(this, task.id) else null
                        if (weekly != null) {
                            createNextWeeklyOccurrence(weekly, completedNow = true)
                        } else {
                            Toast.makeText(this, if (status == "completed") "Task completed" else "Task started", Toast.LENGTH_SHORT).show()
                            VoiceWidgetProvider.refreshTasks(this)
                            if (currentPage == AppPage.FEED) loadMomentumFeed() else loadTasks()
                        }
                    },
                    onFailure = { error ->
                        taskStatusView.text = "Task update failed: ${error.message.orEmpty()}"
                        taskStatusView.setTextColor(CarbonPalette.red)
                        if (::feedStatusView.isInitialized) {
                            feedStatusView.text = "Task update failed. Your feed is unchanged."
                            feedStatusView.setTextColor(CarbonPalette.red)
                        }
                    },
                )
            }
        }
    }

    private fun confirmTaskCompletion(task: VoiceTask) {
        val weekly = WeeklyTaskStore.forTask(this, task.id)
        AlertDialog.Builder(this)
            .setTitle("Complete this task?")
            .setMessage(
                if (weekly == null) {
                    "${task.title}\n\nThis removes it from the open task list. You can cancel and keep working on it."
                } else {
                    "${task.title}\n\nThis occurrence will be completed, then OV will create the next ${WeeklyTaskModel.scheduleLabel(weekly.dayOfWeek, weekly.hour, weekly.minute).lowercase(Locale.US)} occurrence."
                },
            )
            .setNegativeButton("Keep open", null)
            .setPositiveButton("Mark complete") { _, _ -> updateTaskStatus(task, "completed") }
            .show()
    }

    private fun showTaskCreationDialog(preselectedProject: VoiceProject? = null) {
        val form = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(22), dp(6), dp(22), 0)
        }
        val titleInput = EditText(this).apply {
            hint = "Task title"
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
            setSingleLine(true)
        }
        val outcomeInput = EditText(this).apply {
            hint = "What does done look like?"
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
            maxLines = 2
        }
        val minutesInput = EditText(this).apply {
            hint = "Minutes"
            inputType = InputType.TYPE_CLASS_NUMBER
            setText("20")
            setSingleLine(true)
        }
        val repeatWeeklyInput = CheckBox(this).apply {
            text = "Repeat every week"
            setTextColor(CarbonPalette.white)
            typeface = Typeface.DEFAULT_BOLD
            setPadding(0, dp(10), 0, dp(4))
        }
        val weekdayNames = DayOfWeek.entries.map { day ->
            day.name.lowercase(Locale.US).replaceFirstChar(Char::uppercase)
        }
        val weekdayInput = Spinner(this).apply {
            adapter = ArrayAdapter(this@MainActivity, android.R.layout.simple_spinner_dropdown_item, weekdayNames)
            setSelection(DayOfWeek.MONDAY.value - 1)
        }
        var selectedHour = 13
        var selectedMinute = 0
        lateinit var timeInput: Button
        timeInput = secondaryButton("DUE 1:00 PM") {
            TimePickerDialog(this, { _, hour, minute ->
                selectedHour = hour
                selectedMinute = minute
                timeInput.text = "DUE ${WeeklyTaskModel.scheduleLabel(DayOfWeek.MONDAY.value, hour, minute).substringAfter("DUE ")}"
            }, selectedHour, selectedMinute, false).show()
        }
        val weeklyFields = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            visibility = View.GONE
            addView(TextView(this@MainActivity).apply {
                text = "WEEKLY DEADLINE"
                textSize = 10f
                typeface = Typeface.DEFAULT_BOLD
                setTextColor(CarbonPalette.purple)
                setPadding(0, dp(6), 0, dp(3))
            }, fullWidthWrap())
            addView(weekdayInput, fullWidthWrap())
            addView(timeInput, fullWidthWrap().apply { topMargin = dp(5) })
            addView(TextView(this@MainActivity).apply {
                text = "OV will keep one open occurrence at a time and create the next after you finish it."
                textSize = 11f
                setTextColor(CarbonPalette.muted)
                setPadding(0, dp(6), 0, 0)
            }, fullWidthWrap())
        }
        repeatWeeklyInput.setOnCheckedChangeListener { _, checked ->
            weeklyFields.visibility = if (checked) View.VISIBLE else View.GONE
        }
        val activeProjects = (listOfNotNull(preselectedProject) + latestProjects.filter { it.status == "active" })
            .distinctBy { it.id }
        val projectChoices = listOf<VoiceProject?>(null) + activeProjects
        val projectInput = Spinner(this).apply {
            adapter = ArrayAdapter(
                this@MainActivity,
                android.R.layout.simple_spinner_dropdown_item,
                projectChoices.map { it?.title ?: "No project • task inbox" },
            )
            setSelection(projectChoices.indexOfFirst { it?.id == preselectedProject?.id }.coerceAtLeast(0))
        }
        form.addView(titleInput, fullWidthWrap())
        form.addView(outcomeInput, fullWidthWrap())
        form.addView(minutesInput, fullWidthWrap())
        form.addView(repeatWeeklyInput, fullWidthWrap())
        form.addView(weeklyFields, fullWidthWrap())
        form.addView(TextView(this).apply {
            text = "PROJECT"
            textSize = 10f
            typeface = Typeface.DEFAULT_BOLD
            setTextColor(CarbonPalette.muted)
            setPadding(0, dp(12), 0, dp(4))
        }, fullWidthWrap())
        form.addView(projectInput, fullWidthWrap())
        AlertDialog.Builder(this)
            .setTitle("Add a task or weekly commitment")
            .setView(form)
            .setNegativeButton("Cancel", null)
            .setPositiveButton("Add") { _, _ ->
                val title = titleInput.text.toString().trim()
                if (title.isBlank()) {
                    Toast.makeText(this, "Task title is required", Toast.LENGTH_SHORT).show()
                    return@setPositiveButton
                }
                val outcome = outcomeInput.text.toString().trim().ifBlank { "$title is complete" }
                val minutes = minutesInput.text.toString().toIntOrNull()?.coerceIn(1, 1440) ?: 20
                val project = projectChoices.getOrNull(projectInput.selectedItemPosition)
                val weekly = if (repeatWeeklyInput.isChecked) WeeklyTaskDraft(
                    dayOfWeek = weekdayInput.selectedItemPosition + 1,
                    hour = selectedHour,
                    minute = selectedMinute,
                ) else null
                createTask(title, outcome, minutes, project?.id, weekly)
            }
            .show()
    }

    private fun createTask(
        title: String,
        outcome: String,
        minutes: Int,
        projectId: String?,
        weekly: WeeklyTaskDraft? = null,
    ) {
        if (::taskStatusView.isInitialized) {
            taskStatusView.text = "Adding $title…"
            taskStatusView.setTextColor(CarbonPalette.amber)
        }
        val dueAt = weekly?.let(WeeklyTaskModel::firstDue)
        GatewayClient.createTask(
            GatewaySettings.baseUrl(this),
            title,
            outcome,
            minutes,
            projectId,
            dueAt,
            if (weekly == null) "normal" else "high",
            DeviceCredentials.token(this),
        ) { result ->
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                result.fold(
                    onSuccess = { task ->
                        if (weekly != null && dueAt != null) {
                            WeeklyTaskStore.create(this, title, outcome, minutes, projectId, weekly, task.id, dueAt)
                        }
                        val message = if (weekly != null) {
                            "Weekly task added • ${WeeklyTaskModel.scheduleLabel(weekly.dayOfWeek, weekly.hour, weekly.minute)}"
                        } else task.vicSummary.ifBlank { "Task added. VIC is analyzing it." }
                        Toast.makeText(this, message, Toast.LENGTH_LONG).show()
                        showPage(AppPage.TASKS)
                    },
                    onFailure = { error ->
                        Toast.makeText(this, "Task could not be added: ${error.message.orEmpty()}", Toast.LENGTH_LONG).show()
                    },
                )
            }
        }
    }

    private fun ensureWeeklyTaskInstances(tasks: List<VoiceTask>) {
        WeeklyTaskStore.needingNextInstance(this, tasks).forEach { template ->
            createNextWeeklyOccurrence(template, completedNow = false)
        }
    }

    private fun createNextWeeklyOccurrence(template: WeeklyTaskTemplate, completedNow: Boolean) {
        if (!weeklyCreationInFlight.add(template.id)) return
        val dueAt = WeeklyTaskModel.nextDue(template)
        GatewayClient.createTask(
            GatewaySettings.baseUrl(this),
            template.title,
            template.observableOutcome,
            template.estimatedMinutes,
            template.projectId,
            dueAt,
            "high",
            DeviceCredentials.token(this),
        ) { result ->
            runOnUiThread {
                weeklyCreationInFlight.remove(template.id)
                if (isFinishing || isDestroyed) return@runOnUiThread
                result.fold(
                    onSuccess = { next ->
                        WeeklyTaskStore.updateActive(this, template.id, next.id, dueAt)
                        TaskWidgetStore.replace(this, next)
                        val label = WeeklyTaskModel.scheduleLabel(template.dayOfWeek, template.hour, template.minute)
                        Toast.makeText(
                            this,
                            if (completedNow) "Completed • next occurrence created • $label" else "Weekly task restored • $label",
                            Toast.LENGTH_LONG,
                        ).show()
                        VoiceWidgetProvider.refreshTasks(this)
                        if (currentPage == AppPage.FEED) loadMomentumFeed() else loadTasks()
                    },
                    onFailure = { error ->
                        if (completedNow) {
                            Toast.makeText(
                                this,
                                "This occurrence is complete. OV will retry creating next week’s task when it reconnects.",
                                Toast.LENGTH_LONG,
                            ).show()
                            renderTasks(latestTasks.map { if (it.id == template.activeTaskId) it.copy(status = "completed") else it })
                        } else {
                            taskStatusView.text = "Weekly task could not renew yet • ${error.message.orEmpty()}"
                            taskStatusView.setTextColor(CarbonPalette.amber)
                        }
                    },
                )
            }
        }
    }

    private fun showProjectCreationDialog() {
        val titleInput = EditText(this).apply {
            hint = "Project outcome, such as Launch the new task system"
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
            setSingleLine(false)
            maxLines = 2
            setPadding(dp(22), dp(8), dp(22), dp(8))
        }
        AlertDialog.Builder(this)
            .setTitle("Create an outcome-based project")
            .setMessage("Use a project when the result needs more than one focused work session. VIC will help break it into owned steps.")
            .setView(titleInput)
            .setNegativeButton("Cancel", null)
            .setPositiveButton("Create") { _, _ ->
                val title = titleInput.text.toString().trim()
                if (title.isBlank()) {
                    Toast.makeText(this, "Project outcome is required", Toast.LENGTH_SHORT).show()
                    return@setPositiveButton
                }
                taskStatusView.text = "Creating $title…"
                taskStatusView.setTextColor(CarbonPalette.amber)
                GatewayClient.createProject(
                    GatewaySettings.baseUrl(this),
                    title,
                    DeviceCredentials.token(this),
                ) { result ->
                    runOnUiThread {
                        if (isFinishing || isDestroyed) return@runOnUiThread
                        result.fold(
                            onSuccess = { project ->
                                currentTaskFilter = TaskFilter.PROJECTS
                                Toast.makeText(this, "Project created. Add the first small task.", Toast.LENGTH_LONG).show()
                                loadTasks()
                                showTaskCreationDialog(project)
                            },
                            onFailure = { error ->
                                taskStatusView.text = "Project could not be created: ${error.message.orEmpty()}"
                                taskStatusView.setTextColor(CarbonPalette.red)
                            },
                        )
                    }
                }
            }
            .show()
    }

    private fun loadSkillProposals() {
        if (!::skillProposalStatusView.isInitialized) return
        skillProposalStatusView.text = "Loading evidence-backed proposalsâ€¦"
        skillProposalStatusView.setTextColor(CarbonPalette.muted)
        GatewayClient.getSkillProposals(
            GatewaySettings.baseUrl(this),
            DeviceCredentials.token(this),
        ) { result ->
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                result.fold(
                    onSuccess = { proposals -> renderSkillProposals(proposals) },
                    onFailure = { error ->
                        skillProposalContainer.removeAllViews()
                        skillProposalStatusView.text =
                            "Skill proposals are unavailable.\n${error.message.orEmpty()}"
                        skillProposalStatusView.setTextColor(CarbonPalette.red)
                    },
                )
            }
        }
        loadSkillCatalog()
        loadSkillUsages()
    }

    private fun loadSkillCatalog() {
        if (!::skillCatalogContainer.isInitialized) return
        GatewayClient.getSkills(GatewaySettings.baseUrl(this), DeviceCredentials.token(this)) { result ->
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                result.fold(onSuccess = { skills ->
                    skillCatalogContainer.removeAllViews()
                    skillCatalogStatusView.text = "${skills.size} active skill${if (skills.size == 1) "" else "s"}. Newest approved version is used."
                    skillCatalogStatusView.setTextColor(CarbonPalette.green)
                    skills.forEach { skill ->
                        val card = LinearLayout(this).apply {
                            orientation = LinearLayout.VERTICAL
                            setPadding(dp(14), dp(12), dp(14), dp(12))
                            background = carbonControl(this@MainActivity, CarbonPalette.green)
                        }
                        card.addView(TextView(this).apply {
                            text = "${skill.name}  •  VERSION ${skill.version}"
                            textSize = 15f
                            typeface = Typeface.DEFAULT_BOLD
                            setTextColor(CarbonPalette.white)
                        }, fullWidthWrap())
                        card.addView(TextView(this).apply {
                            text = skill.requiredCapabilities.joinToString().ifBlank { "Coordination procedure" }
                            textSize = 11f
                            setTextColor(CarbonPalette.muted)
                            setPadding(0, dp(6), 0, 0)
                        }, fullWidthWrap())
                        card.addView(secondaryButton("DISABLE") { setSkillEnabled(skill, false) }, fullWidthWrap().apply { topMargin = dp(10) })
                        skillCatalogContainer.addView(card, fullWidthWrap().apply { topMargin = dp(8) })
                    }
                }, onFailure = { error ->
                    skillCatalogStatusView.text = "Active skills unavailable. ${error.message.orEmpty()}"
                    skillCatalogStatusView.setTextColor(CarbonPalette.red)
                })
            }
        }
    }

    private fun setSkillEnabled(skill: SkillProposal, enabled: Boolean) {
        GatewayClient.setSkillEnabled(GatewaySettings.baseUrl(this), skill.id, enabled, DeviceCredentials.token(this)) { result ->
            runOnUiThread {
                result.fold(onSuccess = {
                    Toast.makeText(this, "${skill.name} ${if (enabled) "enabled" else "disabled"}.", Toast.LENGTH_LONG).show()
                    loadSkillProposals()
                }, onFailure = { Toast.makeText(this, it.message.orEmpty(), Toast.LENGTH_LONG).show() })
            }
        }
    }

    private fun loadSkillUsages() {
        if (!::skillUsageContainer.isInitialized) return
        GatewayClient.getSkillUsages(GatewaySettings.baseUrl(this), DeviceCredentials.token(this)) { result ->
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                skillUsageContainer.removeAllViews()
                result.onSuccess { usages ->
                    if (usages.isEmpty()) {
                        skillUsageContainer.addView(TextView(this).apply {
                            text = "VIC has not used an approved typed workflow since tracking was enabled."
                            setTextColor(CarbonPalette.muted)
                        }, fullWidthWrap())
                    }
                    usages.take(10).forEach { usage ->
                        val row = LinearLayout(this).apply {
                            orientation = LinearLayout.VERTICAL
                            setPadding(dp(12), dp(10), dp(12), dp(10))
                            background = carbonControl(this@MainActivity, CarbonPalette.line)
                        }
                        row.addView(TextView(this).apply {
                            text = "${usage.skillName} v${usage.skillVersion}  •  ${usage.outcome}"
                            typeface = Typeface.DEFAULT_BOLD
                            setTextColor(CarbonPalette.white)
                        }, fullWidthWrap())
                        if (usage.feedback == null) {
                            val actions = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
                            actions.addView(secondaryButton("USED CORRECTLY") { reviewSkillUsage(usage, true) }, weightedButton())
                            actions.addView(secondaryButton("USED INCORRECTLY") { reviewSkillUsage(usage, false) }, weightedButton().apply { marginStart = dp(8) })
                            row.addView(actions, fullWidthWrap().apply { topMargin = dp(8) })
                        } else {
                            row.addView(TextView(this).apply {
                                text = "REVIEWED: ${usage.feedback.uppercase()}"
                                setTextColor(if (usage.feedback == "correct") CarbonPalette.green else CarbonPalette.red)
                            }, fullWidthWrap())
                        }
                        skillUsageContainer.addView(row, fullWidthWrap().apply { topMargin = dp(8) })
                    }
                }.onFailure { error ->
                    skillUsageContainer.addView(TextView(this).apply {
                        text = "Skill history unavailable. ${error.message.orEmpty()}"
                        setTextColor(CarbonPalette.red)
                    }, fullWidthWrap())
                }
            }
        }
    }

    private fun reviewSkillUsage(usage: SkillUsage, correct: Boolean) {
        GatewayClient.reviewSkillUsage(GatewaySettings.baseUrl(this), usage.id, correct, DeviceCredentials.token(this)) { result ->
            runOnUiThread {
                result.fold(onSuccess = { loadSkillUsages() }, onFailure = { Toast.makeText(this, it.message.orEmpty(), Toast.LENGTH_LONG).show() })
            }
        }
    }

    private fun renderSkillProposals(proposals: List<SkillProposal>) {
        skillProposalContainer.removeAllViews()
        skillProposalStatusView.text = if (proposals.isEmpty()) {
            "No proposals are waiting for review. VoiceOS will never enable a generated skill silently."
        } else {
            "${proposals.size} proposal${if (proposals.size == 1) "" else "s"} waiting. Review the procedure, capabilities, and evidence before deciding."
        }
        skillProposalStatusView.setTextColor(if (proposals.isEmpty()) CarbonPalette.green else CarbonPalette.amber)
        proposals.forEachIndexed { index, proposal ->
            val card = LinearLayout(this).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(dp(14), dp(14), dp(14), dp(14))
                background = carbonPanel(this@MainActivity, CarbonPalette.amber)
            }
            card.addView(TextView(this).apply {
                text = "${proposal.name}  â€¢  VERSION ${proposal.version}"
                textSize = 16f
                typeface = Typeface.DEFAULT_BOLD
                setTextColor(CarbonPalette.white)
            }, fullWidthWrap())
            card.addView(TextView(this).apply {
                text = "REQUIRES  â€¢  ${proposal.requiredCapabilities.joinToString().ifBlank { "NO CAPABILITIES" }}\nEVIDENCE  â€¢  ${proposal.evidenceCount} SUCCESSFUL AUDIT TURNS"
                textSize = 10f
                letterSpacing = 0.08f
                setTextColor(CarbonPalette.amber)
                setPadding(0, dp(8), 0, 0)
            }, fullWidthWrap())
            card.addView(TextView(this).apply {
                text = proposal.content
                textSize = 13f
                setTextColor(CarbonPalette.white)
                setLineSpacing(dp(3).toFloat(), 1.12f)
                setPadding(0, dp(12), 0, 0)
                setTextIsSelectable(true)
            }, fullWidthWrap())
            card.addView(TextView(this).apply {
                text = "EVIDENCE\n${proposal.evidenceJson}"
                textSize = 11f
                setTextColor(CarbonPalette.muted)
                setLineSpacing(dp(2).toFloat(), 1.1f)
                setPadding(0, dp(12), 0, 0)
                setTextIsSelectable(true)
            }, fullWidthWrap())
            val decisions = LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER
            }
            decisions.addView(secondaryButton("REJECT") {
                decideSkillProposal(proposal, approve = false)
            }, weightedButton())
            decisions.addView(actionButton("APPROVE").apply {
                setOnClickListener { decideSkillProposal(proposal, approve = true) }
            }, weightedButton().apply { marginStart = dp(8) })
            card.addView(decisions, fullWidthWrap().apply { topMargin = dp(14) })
            skillProposalContainer.addView(
                card,
                fullWidthWrap().apply { topMargin = if (index == 0) dp(6) else dp(12) },
            )
        }
    }

    private fun decideSkillProposal(proposal: SkillProposal, approve: Boolean) {
        skillProposalStatusView.text =
            if (approve) "Approving ${proposal.name}â€¦" else "Rejecting ${proposal.name}â€¦"
        skillProposalStatusView.setTextColor(CarbonPalette.amber)
        GatewayClient.decideSkillProposal(
            GatewaySettings.baseUrl(this),
            proposal.id,
            approve,
            DeviceCredentials.token(this),
        ) { result ->
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                result.fold(
                    onSuccess = { decided ->
                        val message = "${decided.name} was ${decided.status}. No skill content was executed."
                        Toast.makeText(this, message, Toast.LENGTH_LONG).show()
                        skillProposalStatusView.text = message
                        skillProposalStatusView.setTextColor(CarbonPalette.green)
                        loadSkillProposals()
                    },
                    onFailure = { error ->
                        skillProposalStatusView.text =
                            "The decision was not recorded.\n${error.message.orEmpty()}"
                        skillProposalStatusView.setTextColor(CarbonPalette.red)
                    },
                )
            }
        }
    }

    private fun loadHistory() {
        historyView.text = "Loading conversation history…"
        historyView.setTextColor(CarbonPalette.muted)
        GatewayClient.getHistory(
            GatewaySettings.baseUrl(this),
            DeviceCredentials.token(this),
        ) { result ->
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                result.fold(
                    onSuccess = { turns ->
                        historyView.setTextColor(CarbonPalette.white)
                        historyView.text = if (turns.isEmpty()) {
                            "No recorded turns yet. Your next conversation will appear here."
                        } else {
                            turns.joinToString("\n\n────────────\n\n") { turn ->
                                val provider = turn.provider.uppercase(Locale.US)
                                val timing = if (turn.processingMs > 0) " • ${turn.processingMs} ms" else ""
                                "YOU\n${turn.transcript}\n\nVIC  •  $provider$timing\n${turn.responseText}"
                            }
                        }
                    },
                    onFailure = { error ->
                        historyView.setTextColor(CarbonPalette.red)
                        historyView.text = "History is unavailable right now.\n\n${error.message.orEmpty()}"
                    },
                )
            }
        }
    }

    private fun initializeTextToSpeech() {
        textToSpeech?.shutdown()
        textToSpeechReady = false
        textToSpeechInitializationComplete = false
        ttsInitializationAttempts += 1
        textToSpeech = TextToSpeech(this, this)
    }

    private fun configureVoices() {
        val engine = textToSpeech ?: return
        availableVoices = engine.voices.orEmpty()
            .filter { it.locale.language.equals(Locale.US.language, ignoreCase = true) }
            .sortedWith(
                compareByDescending<Voice> { it.quality }
                    .thenByDescending { it.isNetworkConnectionRequired }
                    .thenBy { it.name },
            )
        if (availableVoices.isEmpty()) {
            if (::voiceButton.isInitialized) voiceButton.text = "VOICE: SYSTEM DEFAULT"
            return
        }
        val savedName = getSharedPreferences(PLAYBACK_PREFERENCES, MODE_PRIVATE)
            .getString(TTS_VOICE_KEY, null)
        selectedVoiceIndex = availableVoices.indexOfFirst { it.name == savedName }
            .takeIf { it >= 0 }
            ?: 0
        engine.voice = availableVoices[selectedVoiceIndex]
        updateVoiceButton()
    }

    private fun cycleVoice() {
        if (availableVoices.isEmpty()) {
            initializeTextToSpeech()
            return
        }
        selectedVoiceIndex = (selectedVoiceIndex + 1) % availableVoices.size
        val voice = availableVoices[selectedVoiceIndex]
        textToSpeech?.voice = voice
        getSharedPreferences(PLAYBACK_PREFERENCES, MODE_PRIVATE)
            .edit()
            .putString(TTS_VOICE_KEY, voice.name)
            .apply()
        updateVoiceButton()
        renderState(VoiceState.SPEAKING, "Testing selected voice")
        speak("This is VIC, the Voice Interface Controller. You can keep tapping the voice button until this voice sounds right.", RESPONSE_UTTERANCE_ID)
    }

    private fun updateVoiceButton() {
        if (!::voiceButton.isInitialized || availableVoices.isEmpty()) return
        val voice = availableVoices[selectedVoiceIndex]
        val quality = when {
            voice.quality >= Voice.QUALITY_VERY_HIGH -> "very high"
            voice.quality >= Voice.QUALITY_HIGH -> "high"
            voice.quality <= Voice.QUALITY_LOW -> "basic"
            else -> "normal"
        }
        val connection = if (voice.isNetworkConnectionRequired) "online" else "offline"
        voiceButton.text = "VOICE ${selectedVoiceIndex + 1}/${availableVoices.size}  •  $quality  •  $connection"
        voiceButton.contentDescription =
            "Selected voice ${selectedVoiceIndex + 1} of ${availableVoices.size}, $quality quality, $connection. Tap for next voice."
    }

    private fun speak(text: String, utteranceId: String) {
        if (!textToSpeechReady) {
            pendingSpeech = text to utteranceId
            if (::ttsStatusView.isInitialized) {
                ttsStatusView.text = "Preparing speech engine…"
                ttsStatusView.setTextColor(CarbonPalette.amber)
            }
            if (textToSpeechInitializationComplete && ttsInitializationAttempts < 2) {
                initializeTextToSpeech()
            }
            return
        }
        textToSpeech?.setSpeechRate(speechRate)
        val result = textToSpeech?.speak(text, TextToSpeech.QUEUE_FLUSH, null, utteranceId)
        if (result == TextToSpeech.ERROR) {
            pendingSpeech = text to utteranceId
            if (ttsInitializationAttempts < 2) {
                initializeTextToSpeech()
            } else {
                pendingSpeech = null
                if (::ttsStatusView.isInitialized) {
                    ttsStatusView.text = "Playback failed — tap Test Voice to retry"
                    ttsStatusView.setTextColor(CarbonPalette.red)
                }
                if (voiceState == VoiceState.SPEAKING) renderState(VoiceState.READY, "Ready")
            }
        } else if (::ttsStatusView.isInitialized) {
            ttsStatusView.text = "Speaking through Android media audio"
            ttsStatusView.setTextColor(CarbonPalette.teal)
        }
    }

    private fun showRecoverableError(message: String, speakError: Boolean) {
        renderState(VoiceState.ERROR, "Voice unavailable")
        transcriptView.text = message
        if (speakError) speak(message, ERROR_UTTERANCE_ID)
    }

    private fun renderState(state: VoiceState, label: String) {
        voiceState = state
        statusView.text = when (state) {
            VoiceState.READY -> "VOICE CHANNEL READY"
            VoiceState.STARTING -> "VOICE CHANNEL STARTING"
            VoiceState.LISTENING -> "VOICE CHANNEL LISTENING"
            VoiceState.PROCESSING -> "VOICE CHANNEL PROCESSING"
            VoiceState.SPEAKING -> "VOICE CHANNEL SPEAKING"
            VoiceState.ERROR -> "VOICE CHANNEL NEEDS ATTENTION"
        }
        voiceTitleView.text = when (state) {
            VoiceState.READY -> "What can I help with?"
            VoiceState.STARTING -> "Getting ready…"
            VoiceState.LISTENING -> "I’m listening"
            VoiceState.PROCESSING -> "Thinking through that"
            VoiceState.SPEAKING -> "Here’s what I found"
            VoiceState.ERROR -> "Let’s try that again"
        }
        VoiceWidgetProvider.updateStatus(this, when (state) {
            VoiceState.READY -> "Ready"
            VoiceState.STARTING, VoiceState.LISTENING -> "Listening"
            VoiceState.PROCESSING -> "Processing"
            VoiceState.SPEAKING -> "Speaking"
            VoiceState.ERROR -> "Error"
        })
        val talkLabel = if (conversationActive) {
            if (conversationPaused) "RESUME" else "PAUSE"
        } else when (state) {
            VoiceState.LISTENING, VoiceState.STARTING -> "DONE"
            VoiceState.PROCESSING -> "STOP"
            VoiceState.SPEAKING -> "INTERRUPT"
            VoiceState.READY, VoiceState.ERROR -> "TALK"
        }
        val accent = when (state) {
            VoiceState.STARTING, VoiceState.LISTENING, VoiceState.PROCESSING -> CarbonPalette.amber
            VoiceState.SPEAKING -> CarbonPalette.purple
            VoiceState.ERROR -> CarbonPalette.red
            VoiceState.READY -> CarbonPalette.teal
        }
        val trackedState = if (state == VoiceState.STARTING) VoiceState.LISTENING else state
        stateTrackViews.forEach { view ->
            val active = view.tag == trackedState
            view.setTextColor(if (active) accent else CarbonPalette.muted)
            view.background = carbonControl(this, if (active) accent else CarbonPalette.line)
        }
        talkButton.setVoiceState(talkLabel, label, accent)
        val canCancel = conversationActive || state in setOf(
            VoiceState.STARTING,
            VoiceState.LISTENING,
            VoiceState.PROCESSING,
            VoiceState.SPEAKING,
        )
        cancelButton.isEnabled = canCancel
        cancelButton.text = if (conversationActive) "END SESSION" else "CANCEL"
        cancelButton.visibility = if (canCancel) View.VISIBLE else View.GONE
        repeatButton.isEnabled = !conversationActive && lastResponse != null && state !in setOf(VoiceState.STARTING, VoiceState.LISTENING)
        copyButton.isEnabled = lastResponse != null
        correctButton.isEnabled = !conversationActive && lastTranscript != null && state !in setOf(VoiceState.STARTING, VoiceState.LISTENING)
        retryButton.isEnabled = failedTranscript != null && state == VoiceState.ERROR
        repeatButton.visibility = if (repeatButton.isEnabled) View.VISIBLE else View.GONE
        copyButton.visibility = if (copyButton.isEnabled) View.VISIBLE else View.GONE
        correctButton.visibility = if (correctButton.isEnabled) View.VISIBLE else View.GONE
        retryButton.visibility = if (retryButton.isEnabled) View.VISIBLE else View.GONE
        speedButton.isEnabled = state !in setOf(
            VoiceState.STARTING,
            VoiceState.LISTENING,
            VoiceState.PROCESSING,
        )
        uploadButton.isEnabled = state !in setOf(
            VoiceState.STARTING,
            VoiceState.LISTENING,
            VoiceState.PROCESSING,
            VoiceState.SPEAKING,
        )
        val canDecide = pendingApproval != null && state in setOf(
            VoiceState.READY,
            VoiceState.SPEAKING,
            VoiceState.ERROR,
        )
        approveButton.isEnabled = canDecide
        denyButton.isEnabled = canDecide
        approveButton.visibility = if (canDecide) View.VISIBLE else View.GONE
        denyButton.visibility = if (canDecide) View.VISIBLE else View.GONE
    }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    private fun recognitionText(results: Bundle?): String? =
        results?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull()?.trim()

    private fun moreCompleteTranscript(finalText: String?, partialText: String?): String? {
        if (finalText.isNullOrBlank()) return partialText
        if (partialText.isNullOrBlank()) return finalText
        val finalWords = finalText.split(Regex("\\s+")).size
        val partialWords = partialText.split(Regex("\\s+")).size
        return if (partialWords > finalWords) partialText else finalText
    }

    private fun recognitionErrorMessage(error: Int): String = when (error) {
        SpeechRecognizer.ERROR_NO_MATCH, SpeechRecognizer.ERROR_SPEECH_TIMEOUT ->
            "I didn't catch that. Tap Talk and try again."
        SpeechRecognizer.ERROR_AUDIO ->
            "The microphone had a problem. Check whether another app is using it and try again."
        SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS ->
            "Microphone permission is required for voice requests."
        SpeechRecognizer.ERROR_RECOGNIZER_BUSY ->
            "The speech recognizer is busy. Wait a moment and try again."
        SpeechRecognizer.ERROR_LANGUAGE_NOT_SUPPORTED, SpeechRecognizer.ERROR_LANGUAGE_UNAVAILABLE ->
            "The offline English speech model is unavailable. Install it in Android settings and try again."
        SpeechRecognizer.ERROR_NETWORK, SpeechRecognizer.ERROR_NETWORK_TIMEOUT ->
            "Offline speech recognition could not start. Check the installed speech model and try again."
        else -> "Speech recognition stopped unexpectedly. Tap Talk and try again."
    }

    companion object {
        const val ACTION_WIDGET_TALK = "dev.voiceos.client.action.WIDGET_TALK"
        const val ACTION_WIDGET_ADD_TASK = "dev.voiceos.client.action.WIDGET_ADD_TASK"
        const val ACTION_WIDGET_OPEN_TASK = "dev.voiceos.client.action.WIDGET_OPEN_TASK"
        const val ACTION_WIDGET_OPEN_FEED = "dev.voiceos.client.action.WIDGET_OPEN_FEED"
        const val ACTION_PIN_WIDGET = "dev.voiceos.client.action.PIN_WIDGET"
        const val ACTION_SOCIAL_SHIELD = "dev.voiceos.client.action.SOCIAL_SHIELD"
        const val ACTION_DAILY_CHECKIN = "dev.voiceos.client.action.DAILY_CHECKIN"
        const val ACTION_SCRIPTURE_REFLECTION = "dev.voiceos.client.action.SCRIPTURE_REFLECTION"
        const val ACTION_VIC_TALK = "dev.voiceos.client.action.VIC_TALK"
        const val ACTION_VIC_SHOW_PROGRESS = "dev.voiceos.client.action.VIC_SHOW_PROGRESS"
        const val ACTION_VIC_TEST_CHECKIN = "dev.voiceos.client.action.VIC_TEST_CHECKIN"
        const val ACTION_CONFIRM_END_CONVERSATION =
            "dev.voiceos.client.action.CONFIRM_END_CONVERSATION"
        const val EXTRA_AUTO_LISTEN = "dev.voiceos.client.extra.AUTO_LISTEN"
        const val EXTRA_TASK_ID = "dev.voiceos.client.extra.TASK_ID"
        const val EXTRA_BLOCKED_PACKAGE = "dev.voiceos.client.extra.BLOCKED_PACKAGE"
        const val EXTRA_PASSAGE_REFERENCE = "dev.voiceos.client.extra.PASSAGE_REFERENCE"
        const val EXTRA_SCRIPTURE_THOUGHTS = "dev.voiceos.client.extra.SCRIPTURE_THOUGHTS"
        private const val REQUEST_MICROPHONE = 42
        private const val REQUEST_DOCUMENT = 43
        private const val REQUEST_NOTIFICATIONS = 44
        private const val REQUEST_IMAGE = 45
        private const val REQUEST_CAMERA = 46
        private const val REQUEST_BRAIN_DUMP = 47
        private const val RESPONSE_UTTERANCE_ID = "voiceos-response"
        private const val CORRECTION_PROMPT_ID = "voiceos-correction-prompt"
        private const val ERROR_UTTERANCE_ID = "voiceos-error"
        private const val PLAYBACK_PREFERENCES = "voiceos_playback"
        private const val SPEECH_RATE_KEY = "speech_rate"
        private const val TTS_VOICE_KEY = "tts_voice_name"
    }
}
