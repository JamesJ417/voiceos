package dev.voiceos.client

import android.annotation.SuppressLint
import android.app.Activity
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import android.view.Gravity
import android.view.ViewGroup
import android.view.WindowInsets
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import android.window.OnBackInvokedCallback
import android.window.OnBackInvokedDispatcher
import org.json.JSONTokener
import java.util.Locale

class AiReaderActivity : Activity(), TextToSpeech.OnInitListener {
    private lateinit var webView: WebView
    private lateinit var listenButton: Button
    private lateinit var pauseButton: Button
    private lateinit var stopButton: Button
    private lateinit var audioStatus: TextView
    private var backCallback: OnBackInvokedCallback? = null
    private var textToSpeech: TextToSpeech? = null
    private var ttsReady = false
    private var articleTitle = ""
    private var articleSource = ""
    private var articleSummary = ""
    private var speechText = ""
    private var speechChunks: List<String> = emptyList()
    private var currentChunk = 0
    private var speaking = false
    private var paused = false

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.statusBarColor = CarbonPalette.black
        window.navigationBarColor = CarbonPalette.black

        val url = intent.getStringExtra(EXTRA_URL).orEmpty()
        articleTitle = intent.getStringExtra(EXTRA_TITLE).orEmpty()
        articleSource = intent.getStringExtra(EXTRA_SOURCE).orEmpty()
        articleSummary = intent.getStringExtra(EXTRA_SUMMARY).orEmpty()
        if (!isAllowed(url)) {
            Toast.makeText(this, "OV blocked a link outside its official AI sources.", Toast.LENGTH_LONG).show()
            finish()
            return
        }

        speechText = ArticleListeningModel.prepare(articleTitle, articleSource, articleSummary, "")
        speechChunks = ArticleListeningModel.chunks(speechText)

        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(CarbonPalette.black)
            setOnApplyWindowInsetsListener { view, insets ->
                val bars = insets.getInsets(WindowInsets.Type.systemBars())
                view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
                insets
            }
        }
        val toolbar = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(14), dp(9), dp(14), dp(9))
            setBackgroundColor(CarbonPalette.surface)
        }
        toolbar.addView(Button(this).apply {
            text = "← BACK TO OV"
            textSize = 11f
            setTextColor(CarbonPalette.black)
            background = carbonControl(this@AiReaderActivity, CarbonPalette.teal)
            setOnClickListener { finish() }
        }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, dp(44)))
        toolbar.addView(LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(12), 0, 0, 0)
            addView(TextView(this@AiReaderActivity).apply {
                text = "OV • ${articleSource.uppercase()}"
                textSize = 10f
                setTextColor(CarbonPalette.teal)
            })
            addView(TextView(this@AiReaderActivity).apply {
                text = articleTitle
                textSize = 13f
                maxLines = 2
                setTextColor(CarbonPalette.white)
            })
        }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        root.addView(toolbar, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))

        val audioPanel = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(14), dp(10), dp(14), dp(11))
            setBackgroundColor(CarbonPalette.surface)
        }
        audioPanel.addView(TextView(this).apply {
            text = "OV ARTICLE AUDIO"
            textSize = 10f
            setTextColor(CarbonPalette.teal)
        })
        val controls = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
        }
        listenButton = audioButton("LISTEN", primary = true) { startOrResumeNarration() }.apply { isEnabled = false }
        pauseButton = audioButton("PAUSE") { pauseNarration() }.apply { isEnabled = false }
        stopButton = audioButton("STOP") { stopNarration() }.apply { isEnabled = false }
        controls.addView(listenButton, weightedControl())
        controls.addView(pauseButton, weightedControl().apply { marginStart = dp(7) })
        controls.addView(stopButton, weightedControl().apply { marginStart = dp(7) })
        audioPanel.addView(controls, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(46)).apply { topMargin = dp(7) })
        audioStatus = TextView(this).apply {
            text = "Preparing article audio…"
            textSize = 12f
            setTextColor(CarbonPalette.muted)
            setPadding(0, dp(7), 0, 0)
        }
        audioPanel.addView(audioStatus)
        root.addView(audioPanel, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))

        webView = WebView(this).apply {
            setBackgroundColor(Color.BLACK)
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            settings.allowFileAccess = false
            settings.allowContentAccess = false
            settings.mediaPlaybackRequiresUserGesture = true
            webChromeClient = WebChromeClient()
            webViewClient = object : WebViewClient() {
                override fun shouldOverrideUrlLoading(view: WebView?, request: WebResourceRequest?): Boolean {
                    val next = request?.url?.toString().orEmpty()
                    if (isAllowed(next)) return false
                    Toast.makeText(this@AiReaderActivity, "That link leaves OV’s official-source reader.", Toast.LENGTH_SHORT).show()
                    return true
                }

                override fun onPageStarted(view: WebView?, url: String?, favicon: android.graphics.Bitmap?) {
                    stopNarration()
                    audioStatus.text = "Loading the article for listening…"
                    audioStatus.setTextColor(CarbonPalette.muted)
                }

                override fun onPageFinished(view: WebView?, url: String?) {
                    extractReadableText(view ?: return)
                }
            }
            loadUrl(url)
        }
        root.addView(webView, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        setContentView(root)
        textToSpeech = TextToSpeech(this, this)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            backCallback = OnBackInvokedCallback { handleBack() }.also { callback ->
                onBackInvokedDispatcher.registerOnBackInvokedCallback(
                    OnBackInvokedDispatcher.PRIORITY_DEFAULT,
                    callback,
                )
            }
        }
    }

    override fun onInit(status: Int) {
        val engine = textToSpeech ?: return
        if (status != TextToSpeech.SUCCESS) {
            audioStatus.text = "Article audio is unavailable on this phone"
            audioStatus.setTextColor(CarbonPalette.red)
            return
        }
        val language = engine.setLanguage(Locale.US)
        if (language == TextToSpeech.LANG_MISSING_DATA || language == TextToSpeech.LANG_NOT_SUPPORTED) {
            audioStatus.text = "Install an English text-to-speech voice to listen"
            audioStatus.setTextColor(CarbonPalette.amber)
            return
        }
        engine.setSpeechRate(0.92f)
        engine.setPitch(1.0f)
        engine.setOnUtteranceProgressListener(object : UtteranceProgressListener() {
            override fun onStart(utteranceId: String?) {
                runOnUiThread { updateSpeakingStatus() }
            }

            override fun onDone(utteranceId: String?) {
                runOnUiThread {
                    if (!speaking || utteranceId != utteranceIdFor(currentChunk)) return@runOnUiThread
                    currentChunk += 1
                    if (currentChunk >= speechChunks.size) finishNarration() else speakCurrentChunk()
                }
            }

            @Deprecated("Deprecated in Java")
            override fun onError(utteranceId: String?) = narrationError()

            override fun onError(utteranceId: String?, errorCode: Int) = narrationError()
        })
        ttsReady = true
        updateReadyStatus()
    }

    private fun extractReadableText(view: WebView) {
        val script = """
            (function() {
                const original = document.querySelector('article') || document.querySelector('main') || document.body;
                if (!original) return '';
                const copy = original.cloneNode(true);
                copy.querySelectorAll('script,style,noscript,nav,header,footer,aside,form,button,svg').forEach(node => node.remove());
                return copy.innerText || '';
            })();
        """.trimIndent()
        view.evaluateJavascript(script) { encoded ->
            val pageText = runCatching { JSONTokener(encoded).nextValue() as? String }.getOrNull().orEmpty()
            speechText = ArticleListeningModel.prepare(articleTitle, articleSource, articleSummary, pageText)
            speechChunks = ArticleListeningModel.chunks(speechText)
            currentChunk = 0
            updateReadyStatus()
        }
    }

    private fun startOrResumeNarration() {
        if (!ttsReady || speechChunks.isEmpty()) {
            Toast.makeText(this, "Article audio is still preparing.", Toast.LENGTH_SHORT).show()
            return
        }
        if (!paused) currentChunk = 0
        paused = false
        speaking = true
        listenButton.text = "LISTENING"
        listenButton.isEnabled = false
        pauseButton.isEnabled = true
        stopButton.isEnabled = true
        speakCurrentChunk()
    }

    private fun speakCurrentChunk() {
        val chunk = speechChunks.getOrNull(currentChunk) ?: return finishNarration()
        val result = textToSpeech?.speak(chunk, TextToSpeech.QUEUE_FLUSH, null, utteranceIdFor(currentChunk))
        if (result == TextToSpeech.ERROR) narrationError()
    }

    private fun pauseNarration() {
        if (!speaking) return
        textToSpeech?.stop()
        speaking = false
        paused = true
        listenButton.text = "RESUME"
        listenButton.isEnabled = true
        pauseButton.isEnabled = false
        stopButton.isEnabled = true
        audioStatus.text = "Paused • Resume restarts the current paragraph"
        audioStatus.setTextColor(CarbonPalette.amber)
    }

    private fun stopNarration() {
        textToSpeech?.stop()
        speaking = false
        paused = false
        currentChunk = 0
        if (::listenButton.isInitialized) {
            listenButton.text = "LISTEN"
            pauseButton.isEnabled = false
            stopButton.isEnabled = false
            listenButton.isEnabled = ttsReady && speechChunks.isNotEmpty()
        }
    }

    private fun finishNarration() {
        speaking = false
        paused = false
        currentChunk = 0
        listenButton.text = "LISTEN AGAIN"
        listenButton.isEnabled = true
        pauseButton.isEnabled = false
        stopButton.isEnabled = false
        audioStatus.text = "Finished • Back to OV keeps you out of the endless feed"
        audioStatus.setTextColor(CarbonPalette.green)
    }

    private fun narrationError() {
        runOnUiThread {
            speaking = false
            paused = true
            listenButton.text = "RETRY"
            listenButton.isEnabled = ttsReady
            pauseButton.isEnabled = false
            stopButton.isEnabled = true
            audioStatus.text = "Audio paused because the phone’s speech engine stopped"
            audioStatus.setTextColor(CarbonPalette.amber)
        }
    }

    private fun updateReadyStatus() {
        if (!::listenButton.isInitialized || speaking || paused) return
        listenButton.isEnabled = ttsReady && speechChunks.isNotEmpty()
        val words = speechText.split(Regex("\\s+")).count(String::isNotBlank)
        val minutes = ((words + 164) / 165).coerceAtLeast(1)
        audioStatus.text = when {
            !ttsReady -> "Preparing the phone’s reading voice…"
            speechChunks.isEmpty() -> "This page did not provide readable text"
            else -> "Ready to listen • about $minutes min • audio starts only when you tap"
        }
        audioStatus.setTextColor(if (ttsReady && speechChunks.isNotEmpty()) CarbonPalette.green else CarbonPalette.muted)
    }

    private fun updateSpeakingStatus() {
        if (!speaking) return
        audioStatus.text = "Listening • part ${currentChunk + 1} of ${speechChunks.size}"
        audioStatus.setTextColor(CarbonPalette.teal)
    }

    @SuppressLint("GestureBackNavigation")
    @Deprecated("Deprecated in Java")
    override fun onBackPressed() = handleBack()

    override fun onDestroy() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            backCallback?.let(onBackInvokedDispatcher::unregisterOnBackInvokedCallback)
        }
        textToSpeech?.stop()
        textToSpeech?.shutdown()
        textToSpeech = null
        if (::webView.isInitialized) {
            webView.stopLoading()
            webView.destroy()
        }
        super.onDestroy()
    }

    private fun handleBack() {
        if (::webView.isInitialized && webView.canGoBack()) webView.goBack() else finish()
    }

    private fun isAllowed(url: String): Boolean {
        val uri = runCatching { android.net.Uri.parse(url) }.getOrNull() ?: return false
        if (uri.scheme != "https") return false
        val host = uri.host.orEmpty().lowercase()
        return ALLOWED_DOMAINS.any { host == it || host.endsWith(".$it") }
    }

    private fun audioButton(label: String, primary: Boolean = false, action: () -> Unit) = Button(this).apply {
        text = label
        textSize = 11f
        setTextColor(if (primary) CarbonPalette.black else CarbonPalette.white)
        background = carbonControl(this@AiReaderActivity, if (primary) CarbonPalette.teal else CarbonPalette.line, filled = primary)
        setOnClickListener { action() }
    }

    private fun weightedControl() = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f)
    private fun utteranceIdFor(index: Int) = "ov-article-$index"
    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    companion object {
        const val EXTRA_URL = "ai_update_url"
        const val EXTRA_TITLE = "ai_update_title"
        const val EXTRA_SOURCE = "ai_update_source"
        const val EXTRA_SUMMARY = "ai_update_summary"

        private val ALLOWED_DOMAINS = setOf(
            "openai.com",
            "deepmind.google",
            "huggingface.co",
            "youtube-nocookie.com",
            "bible.com",
        )
    }
}
