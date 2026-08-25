package dev.voiceos.client

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Typeface
import android.os.Bundle
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import android.text.InputType
import android.view.Gravity
import android.view.ViewGroup
import android.view.WindowInsets
import android.widget.Button
import android.widget.CheckBox
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import java.util.Locale
import java.util.concurrent.atomic.AtomicInteger

class BrainDumpActivity : Activity() {
    private lateinit var content: LinearLayout
    private lateinit var dumpInput: EditText
    private lateinit var captureButton: Button
    private lateinit var statusView: TextView
    private lateinit var reviewContainer: LinearLayout
    private var speechRecognizer: SpeechRecognizer? = null
    private var collecting = false
    private var committedText = ""
    private var openTasks: List<VoiceTask> = emptyList()
    private var currentReview: BrainDumpReview? = null
    private val answerInputs = mutableMapOf<String, EditText>()
    private val proposalChecks = mutableMapOf<String, CheckBox>()

    private val recognitionListener = object : RecognitionListener {
        override fun onReadyForSpeech(params: Bundle?) {
            statusView.text = "Listening • keep rattling it out"
            statusView.setTextColor(CarbonPalette.teal)
        }

        override fun onBeginningOfSpeech() = Unit
        override fun onRmsChanged(rmsdB: Float) = Unit
        override fun onBufferReceived(buffer: ByteArray?) = Unit
        override fun onEndOfSpeech() { statusView.text = "Saving this chunk…" }

        override fun onError(error: Int) {
            if (collecting && error in setOf(SpeechRecognizer.ERROR_NO_MATCH, SpeechRecognizer.ERROR_SPEECH_TIMEOUT)) {
                restartListening()
            } else {
                stopCapture("Voice paused • your words are still here")
            }
        }

        override fun onResults(results: Bundle?) {
            val final = results?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull().orEmpty().trim()
            if (final.isNotBlank()) {
                committedText = listOf(committedText.trim(), final).filter(String::isNotBlank).joinToString(". ")
                dumpInput.setText(committedText)
                dumpInput.setSelection(dumpInput.text.length)
            }
            if (collecting) restartListening()
        }

        override fun onPartialResults(partialResults: Bundle?) {
            val partial = partialResults?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull().orEmpty()
            val visible = listOf(committedText.trim(), partial.trim()).filter(String::isNotBlank).joinToString(". ")
            dumpInput.setText(visible)
            dumpInput.setSelection(dumpInput.text.length)
        }

        override fun onEvent(eventType: Int, params: Bundle?) = Unit
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.statusBarColor = CarbonPalette.black
        window.navigationBarColor = CarbonPalette.black
        setContentView(buildView())
    }

    private fun buildView(): ScrollView {
        content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(18), dp(16), dp(36))
            background = CarbonBackgroundDrawable(this@BrainDumpActivity)
            setOnApplyWindowInsetsListener { view, insets ->
                val bars = insets.getInsets(WindowInsets.Type.systemBars())
                view.setPadding(dp(16) + bars.left, dp(18) + bars.top, dp(16) + bars.right, dp(36) + bars.bottom)
                insets
            }
        }
        content.addView(text("OV • BRAIN DUMP", 11f, CarbonPalette.teal, bold = true))
        content.addView(text("Empty your head. VIC will sort it.", 27f, CarbonPalette.white, bold = true).apply {
            setPadding(0, dp(7), 0, 0)
        })
        content.addView(panel().apply {
            addView(text("PRIVATE REVIEW", 10f, CarbonPalette.teal, bold = true))
            addView(text(
                "Talk without organizing. Pause whenever you need. Your raw dump stays on this phone; only changes you approve are sent to your private task system.",
                13f,
                CarbonPalette.muted,
            ).apply { setPadding(0, dp(7), 0, 0) })
            dumpInput = EditText(this@BrainDumpActivity).apply {
                hint = "Everything in my head…"
                minLines = 7
                gravity = Gravity.TOP
                inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES or InputType.TYPE_TEXT_FLAG_MULTI_LINE
                setTextColor(CarbonPalette.white)
                setHintTextColor(CarbonPalette.muted)
                background = carbonControl(this@BrainDumpActivity, CarbonPalette.line)
                setPadding(dp(14), dp(12), dp(14), dp(12))
            }
            addView(dumpInput, fullWidth().apply { topMargin = dp(12) })
            captureButton = button("START TALKING", primary = true) { toggleCapture() }
            addView(captureButton, fullWidth().apply { topMargin = dp(10) })
            statusView = text("Ready • you can also type", 12f, CarbonPalette.muted)
            addView(statusView.apply { setPadding(0, dp(8), 0, 0) })
            addView(button("LET VIC SORT THIS", primary = false) { analyze() }, fullWidth().apply { topMargin = dp(10) })
        }, fullWidth().apply { topMargin = dp(18) })
        reviewContainer = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        content.addView(reviewContainer, fullWidth())
        content.addView(button("← BACK TO OV", primary = false) { finish() }, fullWidth().apply { topMargin = dp(18) })
        return ScrollView(this).apply { addView(content) }
    }

    private fun toggleCapture() {
        if (collecting) {
            stopCapture("Voice capture finished • review the transcript")
            return
        }
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), REQUEST_MICROPHONE)
            return
        }
        if (!SpeechRecognizer.isOnDeviceRecognitionAvailable(this)) {
            Toast.makeText(this, "Install Android’s offline English speech model first.", Toast.LENGTH_LONG).show()
            return
        }
        committedText = dumpInput.text.toString().trim()
        collecting = true
        captureButton.text = "FINISH TALKING"
        if (speechRecognizer == null) {
            speechRecognizer = SpeechRecognizer.createOnDeviceSpeechRecognizer(this).apply {
                setRecognitionListener(recognitionListener)
            }
        }
        startListening()
    }

    private fun startListening() {
        if (!collecting) return
        val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
            putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
            putExtra(RecognizerIntent.EXTRA_LANGUAGE, Locale.US.toLanguageTag())
            putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
            putExtra(RecognizerIntent.EXTRA_PREFER_OFFLINE, true)
            putExtra(RecognizerIntent.EXTRA_MAX_RESULTS, 1)
            putExtra(RecognizerIntent.EXTRA_SPEECH_INPUT_MINIMUM_LENGTH_MILLIS, 1_500L)
            putExtra(RecognizerIntent.EXTRA_SPEECH_INPUT_COMPLETE_SILENCE_LENGTH_MILLIS, 2_200L)
        }
        speechRecognizer?.startListening(intent)
    }

    private fun restartListening() {
        content.postDelayed({ if (collecting) startListening() }, 300)
    }

    private fun stopCapture(message: String) {
        collecting = false
        speechRecognizer?.stopListening()
        committedText = dumpInput.text.toString().trim()
        captureButton.text = "ADD MORE BY VOICE"
        statusView.text = message
        statusView.setTextColor(CarbonPalette.muted)
    }

    private fun analyze() {
        stopCapture("Comparing with unfinished tasks…")
        val transcript = dumpInput.text.toString().trim()
        if (transcript.isBlank()) {
            Toast.makeText(this, "Talk or type your brain dump first.", Toast.LENGTH_SHORT).show()
            return
        }
        statusView.text = "VIC is checking repeats, task size, and importance…"
        GatewayClient.getTasks(
            GatewaySettings.baseUrl(this),
            DeviceCredentials.token(this),
            limit = 100,
        ) { result ->
            runOnUiThread {
                if (isFinishing || isDestroyed) return@runOnUiThread
                openTasks = result.getOrElse { TaskWidgetStore.load(this) }
                    .filter { it.status !in setOf("completed", "cancelled") }
                currentReview = BrainDumpModel.review(transcript, openTasks)
                renderReview(currentReview!!, emptyMap())
                statusView.text = if (result.isSuccess) "Private task comparison complete" else "Offline • compared with saved tasks"
                statusView.setTextColor(if (result.isSuccess) CarbonPalette.green else CarbonPalette.amber)
            }
        }
    }

    private fun renderReview(review: BrainDumpReview, answers: Map<String, String>) {
        reviewContainer.removeAllViews()
        answerInputs.clear()
        proposalChecks.clear()
        reviewContainer.addView(panel().apply {
            addView(text("VIC’S READ", 10f, CarbonPalette.teal, bold = true))
            addView(text(review.summary, 17f, CarbonPalette.white, bold = true).apply { setPadding(0, dp(6), 0, 0) })
        }, fullWidth().apply { topMargin = dp(12) })
        review.callouts.forEach { callout ->
            reviewContainer.addView(panel(accent = CarbonPalette.amber).apply {
                addView(text("VIC PUSHBACK", 10f, CarbonPalette.amber, bold = true))
                addView(text(callout, 14f, CarbonPalette.white).apply { setPadding(0, dp(7), 0, 0) })
            }, fullWidth().apply { topMargin = dp(9) })
        }
        if (review.questions.isNotEmpty()) {
            reviewContainer.addView(panel().apply {
                addView(text("PRIORITY QUESTIONS", 10f, CarbonPalette.teal, bold = true))
                review.questions.forEach { question ->
                    addView(text(question.question, 14f, CarbonPalette.white, bold = true).apply { setPadding(0, dp(10), 0, 0) })
                    addView(text(question.reason, 11f, CarbonPalette.muted).apply { setPadding(0, dp(3), 0, 0) })
                    val input = EditText(this@BrainDumpActivity).apply {
                        hint = "Your answer"
                        setText(answers[question.id].orEmpty())
                        setTextColor(CarbonPalette.white)
                        setHintTextColor(CarbonPalette.muted)
                        background = carbonControl(this@BrainDumpActivity, CarbonPalette.line)
                        setPadding(dp(12), dp(9), dp(12), dp(9))
                    }
                    answerInputs[question.id] = input
                    addView(input, fullWidth().apply { topMargin = dp(6) })
                }
                addView(button("RECHECK MY PRIORITIES", primary = true) { recheckPriorities() }, fullWidth().apply { topMargin = dp(12) })
            }, fullWidth().apply { topMargin = dp(9) })
        }
        review.proposals.forEach { proposal ->
            val accent = when (proposal.action) {
                BrainDumpAction.DUPLICATE -> CarbonPalette.purple
                BrainDumpAction.UPDATE -> CarbonPalette.cyan
                BrainDumpAction.CREATE -> CarbonPalette.teal
                BrainDumpAction.IGNORE -> CarbonPalette.muted
            }
            reviewContainer.addView(panel(accent = accent).apply {
                val checkbox = CheckBox(this@BrainDumpActivity).apply {
                    text = when (proposal.action) {
                        BrainDumpAction.DUPLICATE -> "REPEATED • ALREADY OPEN"
                        BrainDumpAction.UPDATE -> "UPDATE EXISTING TASK"
                        BrainDumpAction.CREATE -> "CREATE NEW TASK"
                        BrainDumpAction.IGNORE -> "THOUGHT • NO TASK YET"
                    }
                    isChecked = proposal.selectedByDefault
                    isEnabled = proposal.action in setOf(BrainDumpAction.CREATE, BrainDumpAction.UPDATE)
                    setTextColor(accent)
                    typeface = Typeface.DEFAULT_BOLD
                }
                proposalChecks[proposal.stableId] = checkbox
                addView(checkbox)
                addView(text(proposal.title, 18f, CarbonPalette.white, bold = true).apply { setPadding(0, dp(5), 0, 0) })
                addView(text(
                    "${proposal.estimatedMinutes.takeIf { it > 0 }?.let { "$it min • " }.orEmpty()}${proposal.importance.uppercase()}\n${proposal.reason}",
                    12f,
                    CarbonPalette.muted,
                ).apply { setPadding(0, dp(6), 0, 0) })
                if (proposal.priorityChallenge.isNotBlank()) addView(
                    text("VIC: ${proposal.priorityChallenge}", 12f, CarbonPalette.amber, bold = true).apply { setPadding(0, dp(7), 0, 0) },
                )
            }, fullWidth().apply { topMargin = dp(8) })
        }
        reviewContainer.addView(button("APPLY CHECKED CHANGES", primary = true) { applyReview() }, fullWidth().apply { topMargin = dp(12) })
    }

    private fun recheckPriorities() {
        val answers = answerInputs.mapValues { it.value.text.toString().trim() }
        if (answers.values.any(String::isBlank)) {
            Toast.makeText(this, "Answer each priority question first.", Toast.LENGTH_SHORT).show()
            return
        }
        currentReview = BrainDumpModel.review(dumpInput.text.toString(), openTasks, answers)
        renderReview(currentReview!!, answers)
        statusView.text = "VIC challenged and re-ranked the review using your answers"
    }

    private fun applyReview() {
        val review = currentReview ?: return
        if (review.questions.isNotEmpty()) {
            Toast.makeText(this, "Answer VIC’s priority questions and tap Recheck first.", Toast.LENGTH_LONG).show()
            return
        }
        val selected = review.proposals.filter { proposalChecks[it.stableId]?.isChecked == true }
        if (selected.isEmpty()) {
            Toast.makeText(this, "No task changes are checked.", Toast.LENGTH_SHORT).show()
            return
        }
        statusView.text = "Applying ${selected.size} approved changes…"
        val remaining = AtomicInteger(selected.size)
        val failures = AtomicInteger(0)
        selected.forEach { proposal -> applyProposal(proposal) { success ->
            if (!success) failures.incrementAndGet()
            if (remaining.decrementAndGet() == 0) runOnUiThread {
                val succeeded = selected.size - failures.get()
                statusView.text = "$succeeded changes applied${if (failures.get() > 0) " • ${failures.get()} need retry" else ""}"
                statusView.setTextColor(if (failures.get() == 0) CarbonPalette.green else CarbonPalette.amber)
                VoiceWidgetProvider.refreshTasks(this)
                setResult(RESULT_OK)
                reviewContainer.addView(button("DONE • BACK TO OV", primary = true) { finish() }, fullWidth().apply { topMargin = dp(10) })
            }
        } }
    }

    private fun applyProposal(proposal: BrainDumpProposal, done: (Boolean) -> Unit) {
        val baseUrl = GatewaySettings.baseUrl(this)
        val token = DeviceCredentials.token(this)
        when (proposal.action) {
            BrainDumpAction.CREATE -> GatewayClient.createTask(
                baseUrl,
                proposal.title,
                proposal.observableOutcome,
                proposal.estimatedMinutes,
                null,
                null,
                proposal.importance,
                token,
            ) { result -> done(result.isSuccess) }
            BrainDumpAction.UPDATE -> {
                val taskId = proposal.existingTaskId
                if (taskId == null) {
                    done(false)
                    return
                }
                GatewayClient.recordTaskProgress(
                    baseUrl,
                    taskId,
                    "Brain dump update: ${proposal.spokenItem}",
                    token,
                ) { progress ->
                    if (progress.isFailure) {
                        done(false)
                    } else {
                        val existing = openTasks.firstOrNull { it.id == taskId }
                        GatewayClient.setTaskAttention(
                            baseUrl,
                            taskId,
                            proposal.importance,
                            existing?.dueAt,
                            token,
                        ) { attention -> done(attention.isSuccess) }
                    }
                }
            }
            BrainDumpAction.DUPLICATE, BrainDumpAction.IGNORE -> done(true)
        }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == REQUEST_MICROPHONE && grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) toggleCapture()
    }

    override fun onDestroy() {
        collecting = false
        speechRecognizer?.cancel()
        speechRecognizer?.destroy()
        speechRecognizer = null
        super.onDestroy()
    }

    private fun panel(accent: Int = CarbonPalette.line) = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(dp(15), dp(14), dp(15), dp(14))
        background = carbonControl(this@BrainDumpActivity, accent)
    }

    private fun button(label: String, primary: Boolean, action: () -> Unit) = Button(this).apply {
        text = label
        textSize = 12f
        typeface = Typeface.DEFAULT_BOLD
        setTextColor(if (primary) CarbonPalette.black else CarbonPalette.white)
        background = carbonControl(this@BrainDumpActivity, if (primary) CarbonPalette.teal else CarbonPalette.line, filled = primary)
        setOnClickListener { action() }
    }

    private fun text(value: String, size: Float, color: Int, bold: Boolean = false) = TextView(this).apply {
        text = value
        textSize = size
        setTextColor(color)
        if (bold) typeface = Typeface.DEFAULT_BOLD
        setLineSpacing(dp(2).toFloat(), 1.1f)
    }

    private fun fullWidth() = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    companion object {
        private const val REQUEST_MICROPHONE = 82
    }
}
