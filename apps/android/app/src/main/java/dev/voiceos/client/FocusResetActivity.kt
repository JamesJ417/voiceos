package dev.voiceos.client

import android.app.Activity
import android.content.Intent
import android.graphics.Typeface
import android.os.Bundle
import android.text.InputType
import android.view.Gravity
import android.view.ViewGroup
import android.view.WindowInsets
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

class FocusResetActivity : Activity() {
    private val passage by lazy { ScriptureResetModel.passageFor() }
    private lateinit var reflectionInput: EditText

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.statusBarColor = CarbonPalette.black
        window.navigationBarColor = CarbonPalette.black
        setContentView(buildView())
    }

    private fun buildView(): ScrollView {
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(18), dp(16), dp(36))
            background = CarbonBackgroundDrawable(this@FocusResetActivity)
            setOnApplyWindowInsetsListener { view, insets ->
                val bars = insets.getInsets(WindowInsets.Type.systemBars())
                view.setPadding(dp(16) + bars.left, dp(18) + bars.top, dp(16) + bars.right, dp(36) + bars.bottom)
                insets
            }
        }
        content.addView(text("OV • 10-MIN VIC RESET", 11f, CarbonPalette.teal, bold = true))
        content.addView(text("Settle. Read. Reflect. Choose.", 27f, CarbonPalette.white, bold = true).apply {
            setPadding(0, dp(7), 0, 0)
        })
        content.addView(panel().apply {
            addView(text("1 MIN • ARRIVE", 10f, CarbonPalette.teal, bold = true))
            addView(text(
                "Put both feet down. Take one slow breath. You do not need to organize everything before you begin.",
                14f,
                CarbonPalette.white,
            ).apply { setPadding(0, dp(7), 0, 0) })
        }, fullWidth().apply { topMargin = dp(16) })
        content.addView(panel(CarbonPalette.purple).apply {
            addView(text("3 MIN • READ OR LISTEN • CSB", 10f, CarbonPalette.purple, bold = true))
            addView(text(passage.reference, 25f, CarbonPalette.white, bold = true).apply { setPadding(0, dp(7), 0, 0) })
            addView(text(passage.theme, 14f, CarbonPalette.muted).apply { setPadding(0, dp(5), 0, 0) })
            addView(button("OPEN ${passage.reference.uppercase()} • CSB", primary = true) {
                startActivity(Intent(this@FocusResetActivity, AiReaderActivity::class.java).apply {
                    putExtra(AiReaderActivity.EXTRA_URL, passage.csbUrl)
                    putExtra(AiReaderActivity.EXTRA_TITLE, "${passage.reference} • CSB")
                    putExtra(AiReaderActivity.EXTRA_SOURCE, "Bible.com • CSB")
                    putExtra(AiReaderActivity.EXTRA_SUMMARY, "Read slowly and notice what draws your attention. Return to OV to reflect with VIC.")
                })
            }, fullWidth().apply { topMargin = dp(12) })
            addView(text(
                "The Scripture text opens from Bible.com in the Christian Standard Bible. Back to OV returns here.",
                11f,
                CarbonPalette.muted,
            ).apply { setPadding(0, dp(8), 0, 0) })
        }, fullWidth().apply { topMargin = dp(10) })
        content.addView(panel(CarbonPalette.cyan).apply {
            addView(text("3 MIN • NOTICE", 10f, CarbonPalette.cyan, bold = true))
            addView(text(passage.noticePrompt, 16f, CarbonPalette.white, bold = true).apply { setPadding(0, dp(8), 0, 0) })
            addView(text(passage.actionPrompt, 14f, CarbonPalette.muted).apply { setPadding(0, dp(8), 0, 0) })
            reflectionInput = EditText(this@FocusResetActivity).apply {
                hint = "Optional notes—leave blank if you want to talk it out"
                minLines = 4
                gravity = Gravity.TOP
                inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES or InputType.TYPE_TEXT_FLAG_MULTI_LINE
                setTextColor(CarbonPalette.white)
                setHintTextColor(CarbonPalette.muted)
                background = carbonControl(this@FocusResetActivity, CarbonPalette.line)
                setPadding(dp(13), dp(11), dp(13), dp(11))
            }
            addView(reflectionInput, fullWidth().apply { topMargin = dp(12) })
        }, fullWidth().apply { topMargin = dp(10) })
        content.addView(panel(CarbonPalette.teal).apply {
            addView(text("3 MIN • TALK + CHOOSE", 10f, CarbonPalette.teal, bold = true))
            addView(text(
                "VIC will ask one real follow-up about your thoughts. After you answer, connect the reflection to one priority and one small next action.",
                14f,
                CarbonPalette.white,
            ).apply { setPadding(0, dp(8), 0, 0) })
            addView(button("TALK WITH VIC ABOUT THIS", primary = true) { handOffToVic() }, fullWidth().apply { topMargin = dp(12) })
            addView(text(
                "Your notes are sent only to your private OV conversation when you tap this button.",
                11f,
                CarbonPalette.muted,
            ).apply { setPadding(0, dp(8), 0, 0) })
        }, fullWidth().apply { topMargin = dp(10) })
        content.addView(button("← BACK TO OV", primary = false) { finish() }, fullWidth().apply { topMargin = dp(16) })
        return ScrollView(this).apply { addView(content) }
    }

    private fun handOffToVic() {
        DailyCheckinScheduler.scheduleTomorrow(this)
        startActivity(Intent(this, MainActivity::class.java).apply {
            action = MainActivity.ACTION_SCRIPTURE_REFLECTION
            flags = Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
            putExtra(MainActivity.EXTRA_PASSAGE_REFERENCE, passage.reference)
            putExtra(MainActivity.EXTRA_SCRIPTURE_THOUGHTS, reflectionInput.text.toString().trim())
        })
        finish()
    }

    private fun panel(accent: Int = CarbonPalette.line) = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(dp(15), dp(14), dp(15), dp(14))
        background = carbonPanel(this@FocusResetActivity, accent)
    }

    private fun button(label: String, primary: Boolean, action: () -> Unit) = Button(this).apply {
        text = label
        textSize = 12f
        typeface = Typeface.DEFAULT_BOLD
        setTextColor(if (primary) CarbonPalette.black else CarbonPalette.white)
        background = carbonControl(this@FocusResetActivity, if (primary) CarbonPalette.teal else CarbonPalette.line, filled = primary)
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
}
